//! 进程内假 provider：只绑 `127.0.0.1`、随机端口，用来抓宿主的真实请求。
//!
//! 为什么需要它：`verify` 要回答"人设到底进没进 system 层"。
//! 唯一可靠的证据是**宿主自己发出去的请求体** —— 配置写成功、文件存在、
//! 命令跑通，都不能证明这一点。
//!
//! 三条硬约束：
//! 1. **只绑回环**，不监听外网（`AGENTS.md` 铁律 8）；
//! 2. **随机端口**，用完即关，不留残留；
//! 3. **只保存判定所需的最小信息**（角色、长度、是否命中标记），
//!    绝不把完整请求体写进报告或日志（人设是用户私密内容）。

// 这里用裸 Mutex 是刻意的：它保护的是**测试夹具内部**的两小块共享状态（抓到的请求、当前标记），
// 不参与任何跨模块锁序，也不会与 metis-fs 的控制锁形成环。
#![allow(clippy::disallowed_types)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 一次被抓到的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRequest {
    /// 请求路径。
    pub path: String,
    /// 请求体里的角色分布：`(role, 字符数, 该角色文本里命中标记的次数)`。
    pub roles: Vec<RoleHit>,
    /// 请求体总字节数。
    pub bytes: usize,
}

/// 一个角色里的命中情况。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleHit {
    /// 角色名（`system` / `developer` / `user` / `unknown`）。
    pub role: String,
    /// 该角色文本的字符数。
    pub chars: usize,
    /// 标记在这个角色的文本里出现了几次。
    pub marker_count: usize,
}

impl CapturedRequest {
    /// 标记落在哪个角色里（取命中次数最多的那个）。
    pub fn hit_role(&self) -> Option<&str> {
        self.roles.iter().filter(|role| role.marker_count > 0).max_by_key(|role| role.marker_count).map(|role| role.role.as_str())
    }

    /// 有任何角色命中吗。
    pub fn hit(&self) -> bool {
        self.roles.iter().any(|role| role.marker_count > 0)
    }
}

/// 假 provider 的句柄。
#[derive(Debug)]
pub struct FakeProvider {
    port: u16,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    stop: Arc<AtomicBool>,
    /// 标记串：出现在请求体里就算"人设到了"。
    marker: Arc<Mutex<String>>,
}

impl FakeProvider {
    /// 起一个只绑 `127.0.0.1` 的随机端口服务。
    ///
    /// 端口用 `0` 让系统分配，避免"端口被占"这类与环境相关的偶发失败。
    pub fn start(marker: impl Into<String>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let marker = Arc::new(Mutex::new(marker.into()));

        let thread_requests = requests.clone();
        let thread_stop = stop.clone();
        let thread_marker = marker.clone();
        std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // **每条连接独立线程**。
                        //
                        // 单线程串行处理时，一次 `read_line` 阻塞（客户端连上但不发数据、
                        // 或发了半截）就会把 accept 循环卡住，后续连接全部失败
                        // （实测 5 次请求里第 5 次直接 `000`，node 侧表现为 `fetch failed`）。
                        let marker = thread_marker.clone();
                        let list = thread_requests.clone();
                        std::thread::spawn(move || {
                            if let Some(request) = handle(stream, &marker)
                                && let Ok(mut guard) = list.lock()
                            {
                                guard.push(request);
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    // **不要因为一次瞬时错误就退出。**
                    //
                    // Windows 上客户端可能在 accept 之间放弃连接，让 `accept` 报
                    // `ConnectionAborted` / `ConnectionReset`。旧代码在这里 `break`，
                    // 于是监听线程死掉、端口还在但没人 accept ——
                    // 后续所有请求都变成 `fetch failed`（实测偶发复现，
                    // 是 `verify` 链路唯一的不稳定来源）。
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            }
        });

        // 不在这里做"就绪探测"：额外一次连接会与真实请求抢 accept，
        // 反而让 `fetch` 更容易失败（实测失败率从 30% 升到 80%）。
        // `TcpListener::bind` 成功后内核就在排队连接，只要 accept 循环不因瞬时错误退出即可。
        Ok(Self { port, requests, stop, marker })
    }

    /// 端口号。
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 给宿主的 base URL。
    ///
    /// 末尾带 `/v1`：多数 OpenAI 兼容客户端会自己拼 `/chat/completions`。
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    /// 换一个标记（每次验证换新的，避免上一次的残留被误判成命中）。
    pub fn set_marker(&self, marker: impl Into<String>) {
        if let Ok(mut current) = self.marker.lock() {
            *current = marker.into();
        }
    }

    /// 已经抓到的请求。
    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().map(|list| list.clone()).unwrap_or_default()
    }

    /// 抓到的请求数。
    pub fn count(&self) -> usize {
        self.requests.lock().map(|list| list.len()).unwrap_or(0)
    }

    /// 关掉服务。
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for FakeProvider {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 处理一条连接：读请求头与请求体，解析出角色分布，回一个最小可用的响应。
fn handle(mut stream: TcpStream, marker: &Arc<Mutex<String>>) -> Option<CapturedRequest> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        // 空连接：可能是 `start()` 的就绪探测，也可能是客户端提前断开。
        // 都不该被记成一次请求。
        return None;
    }
    let Some(path) = request_line.split_whitespace().nth(1) else {
        // 不是合法的 HTTP 请求行（就绪探测、端口扫描等）——同样忽略。
        return None;
    };
    let path = path.to_owned();

    // 读头：找 Content-Length。
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
            && let Ok(parsed) = value.trim().parse::<usize>()
        {
            content_length = parsed;
        }
    }

    // 读体：有上限，别被异常输入拖住。
    let mut body = vec![0_u8; content_length.min(4 * 1024 * 1024)];
    if !body.is_empty() {
        reader.read_exact(&mut body).ok()?;
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    let pattern = marker.lock().map(|m| m.clone()).unwrap_or_default();

    let roles = extract_roles(&text, &pattern);

    // 回一个最小的 OpenAI 兼容响应，让客户端正常收尾。
    let payload = r#"{"id":"metis-verify","object":"chat.completion","created":0,"model":"capture","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", payload.len(), payload);
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();

    Some(CapturedRequest { path, roles, bytes: body.len() })
}

/// 从请求体里抽出"角色 → (字符数, 命中次数)"。
///
/// 不依赖具体 schema：OpenAI 是 `messages[].role`，Anthropic 是顶层 `system` +
/// `messages[]`。用最小 JSON 遍历，认不出的形状就记成 `unknown`。
fn extract_roles(body: &str, marker: &str) -> Vec<RoleHit> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        // 不是 JSON（比如流式 chunk）：退化成整体匹配。
        let count = if marker.is_empty() { 0 } else { body.matches(marker).count() };
        return vec![RoleHit { role: "unknown".to_owned(), chars: body.chars().count(), marker_count: count }];
    };

    let mut hits: Vec<RoleHit> = Vec::new();
    // Anthropic 风格：顶层 `system` 可以是字符串或块数组。
    if let Some(system) = value.get("system") {
        let text = flatten(system);
        hits.push(RoleHit { role: "system".to_owned(), chars: text.chars().count(), marker_count: count_marker(&text, marker) });
    }
    // OpenAI 风格：`messages[]`。
    if let Some(messages) = value.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            let role = message.get("role").and_then(|r| r.as_str()).unwrap_or("unknown").to_owned();
            let text = message.get("content").map(flatten).unwrap_or_default();
            let chars = text.chars().count();
            let marker_count = count_marker(&text, marker);
            // 同角色出现多次时累加，避免只看到第一条。
            if let Some(existing) = hits.iter_mut().find(|hit| hit.role == role) {
                existing.chars += chars;
                existing.marker_count += marker_count;
            } else {
                hits.push(RoleHit { role, chars, marker_count });
            }
        }
    }
    // Codex 等把内容放在 `input[]`。
    if let Some(input) = value.get("input").and_then(|m| m.as_array())
        && hits.is_empty()
    {
        for item in input {
            let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("unknown").to_owned();
            let text = item.get("content").map(flatten).unwrap_or_default();
            hits.push(RoleHit { role, chars: text.chars().count(), marker_count: count_marker(&text, marker) });
        }
    }
    if hits.is_empty() {
        let text = flatten(&value);
        hits.push(RoleHit { role: "unknown".to_owned(), chars: text.chars().count(), marker_count: count_marker(&text, marker) });
    }
    hits
}

/// 把任意 JSON 结构摊平成文本（只取字符串叶子）。
fn flatten(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items.iter().map(flatten).collect::<Vec<_>>().join("\n"),
        serde_json::Value::Object(map) => map.values().map(flatten).collect::<Vec<_>>().join("\n"),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => String::new(),
    }
}

fn count_marker(text: &str, marker: &str) -> usize {
    if marker.is_empty() { 0 } else { text.matches(marker).count() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(port: u16, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    fn wait_for(provider: &FakeProvider, expected: usize) -> Vec<CapturedRequest> {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(5) {
            let requests = provider.requests();
            if requests.len() >= expected {
                return requests;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        provider.requests()
    }

    #[test]
    fn binds_only_loopback_on_a_random_port() {
        let provider = FakeProvider::start("MARK").expect("start");
        assert!(provider.port() > 0, "必须是真实端口");
        assert!(provider.base_url().starts_with("http://127.0.0.1:"), "只能绑回环：{}", provider.base_url());
        assert!(!provider.base_url().contains("0.0.0.0"));
    }

    #[test]
    fn captures_a_system_message() {
        let provider = FakeProvider::start("CANARY").expect("start");
        let body = r#"{"messages":[{"role":"system","content":"you are CANARY"},{"role":"user","content":"hi"}]}"#;
        let response = post(provider.port(), "/v1/chat/completions", body);
        assert!(response.contains("200 OK"), "应回 200：{response}");

        let requests = wait_for(&provider, 1);
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.hit_role(), Some("system"));
        assert!(request.hit());
    }

    #[test]
    fn captures_a_developer_role() {
        // Codex 实际用的是 developer 角色，不是 system —— 报告要能如实反映。
        let provider = FakeProvider::start("CANARY").expect("start");
        post(provider.port(), "/v1/responses", r#"{"messages":[{"role":"developer","content":"CANARY here"},{"role":"user","content":"hi"}]}"#);
        let requests = wait_for(&provider, 1);
        assert_eq!(requests.first().and_then(|r| r.hit_role()), Some("developer"));
    }

    #[test]
    fn captures_anthropic_top_level_system() {
        let provider = FakeProvider::start("CANARY").expect("start");
        post(provider.port(), "/v1/messages", r#"{"system":"CANARY identity","messages":[{"role":"user","content":"hi"}]}"#);
        let requests = wait_for(&provider, 1);
        let request = requests.first().expect("request");
        assert_eq!(request.hit_role(), Some("system"));
    }

    #[test]
    fn reports_no_hit_when_the_marker_is_absent() {
        let provider = FakeProvider::start("CANARY").expect("start");
        post(provider.port(), "/v1/chat/completions", r#"{"messages":[{"role":"system","content":"host prompt only"}]}"#);
        let requests = wait_for(&provider, 1);
        let request = requests.first().expect("request");
        assert!(!request.hit(), "没有标记时不能报命中");
        assert_eq!(request.hit_role(), None);
    }

    #[test]
    fn marker_can_be_changed_between_runs() {
        // 每次验证用新标记：上一次的残留不能被误判成本次命中。
        let provider = FakeProvider::start("OLD").expect("start");
        post(provider.port(), "/v1/chat/completions", r#"{"messages":[{"role":"system","content":"OLD"}]}"#);
        wait_for(&provider, 1);

        provider.set_marker("NEW");
        post(provider.port(), "/v1/chat/completions", r#"{"messages":[{"role":"system","content":"OLD"}]}"#);
        let requests = wait_for(&provider, 2);
        assert!(requests[0].hit(), "第一次用 OLD 应命中");
        assert!(!requests[1].hit(), "换了标记之后旧的 OLD 不算命中");
    }

    #[test]
    fn non_json_body_still_gets_a_response() {
        let provider = FakeProvider::start("CANARY").expect("start");
        let response = post(provider.port(), "/v1/chat/completions", "not json at all");
        assert!(response.contains("200 OK"), "非 JSON 也要正常收尾：{response}");
        let requests = wait_for(&provider, 1);
        assert_eq!(requests.first().map(|r| r.roles.len()), Some(1));
    }

    #[test]
    fn counts_marker_occurrences_in_one_role() {
        let provider = FakeProvider::start("X").expect("start");
        post(provider.port(), "/v1/chat/completions", r#"{"messages":[{"role":"system","content":"X and X and X"}]}"#);
        let requests = wait_for(&provider, 1);
        assert_eq!(requests[0].roles[0].marker_count, 3);
    }

    #[test]
    fn multiple_messages_of_the_same_role_are_accumulated() {
        let provider = FakeProvider::start("X").expect("start");
        post(provider.port(), "/v1/chat/completions", r#"{"messages":[{"role":"system","content":"X"},{"role":"system","content":"X"}]}"#);
        let requests = wait_for(&provider, 1);
        assert_eq!(requests[0].roles.len(), 1, "同角色应合并成一条");
        assert_eq!(requests[0].roles[0].marker_count, 2);
    }

    #[test]
    fn stop_is_safe_to_call_twice() {
        let provider = FakeProvider::start("M").expect("start");
        provider.stop();
        provider.stop();
    }
}
