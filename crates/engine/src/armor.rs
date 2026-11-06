//! `armor`：把人设注入的**实测结果**翻译成可执行策略。
//!
//! 上游把这件事做成"先跑一轮真实模型评测，再读结果给建议"。Metis 的做法分开：
//!
//! - **`plan`**：纯函数。给定一家的实测能力（哪一层真的能注入、命中率多少），
//!   算出"该用哪个模式、哪些层要禁、预检要查什么"。**不联网、不起进程。**
//! - **`preflight`**：在本机检查 plan 的前提是否成立（通道能力、人设是否就绪、权限）。
//! - **`run`**：把评测真正跑起来 —— 但**只负责编排**，实际的"发请求、判命中"
//!   交给已实现的 `verify`。所以它不需要额外的模型调用代码。
//! - **`judge`**：把多次 `run` 的原始结果聚合成一张矩阵。
//!
//! 为什么这样切：真实评测需要模型额度与真机环境，那是**用户**的事；
//! Metis 的职责是把"测出来的东西"变成"不会出错的下一步"，并且**绝不假装测过**
//! （铁律 4：`written` 与 `runtimeConfirmed` 必须分开）。

use metis_core::{HarnessId, Result};
use serde::{Deserialize, Serialize};

/// 注入层。与 `HARNESS-MATRIX.md` 的列一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArmorLayer {
    /// 不注入。
    Off,
    /// 写进宿主自己的系统提示/规则文件（唯一跨家稳定的面）。
    Sys,
    /// 塞进用户消息（压不住身份的多数情况）。
    User,
    /// 带人设词种子再注入（少数宿主需要）。
    Seed,
}

impl ArmorLayer {
    /// 序列化名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Sys => "sys",
            Self::User => "user",
            Self::Seed => "seed",
        }
    }

    /// 全部层（矩阵的列顺序）。
    pub const ALL: [Self; 4] = [Self::Off, Self::Sys, Self::User, Self::Seed];

    /// 解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "sys" => Some(Self::Sys),
            "user" => Some(Self::User),
            "seed" => Some(Self::Seed),
            _ => None,
        }
    }
}

/// 一家的实测数字（百分比 0–100；`None` = 没测过）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Measurements {
    /// 裸注入（不带人设词）在各层的命中率。
    #[serde(default)]
    pub bare: LayerRates,
    /// 带人设词注入的各层命中率。
    #[serde(default)]
    pub persona: LayerRates,
}

/// 各层的命中率。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LayerRates {
    /// sys 层。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys: Option<u8>,
    /// user 层。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<u8>,
    /// seed 层。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u8>,
}

impl LayerRates {
    /// 按层取。
    pub fn get(&self, layer: ArmorLayer) -> Option<u8> {
        match layer {
            ArmorLayer::Off => None,
            ArmorLayer::Sys => self.sys,
            ArmorLayer::User => self.user,
            ArmorLayer::Seed => self.seed,
        }
    }
}

/// 一家的策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// 哪家。
    pub harness: String,
    /// 建议的层。
    pub recommended_layer: ArmorLayer,
    /// 建议的层对应的 Metis 模式。
    pub recommended_mode: String,
    /// 哪些层要禁，以及为什么。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<BlockedLayer>,
    /// 预检要确认的事项。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preflight: Vec<String>,
    /// 给用户看的结论。
    pub verdict: String,
    /// 没测过时这里说明缺什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
}

/// 被禁的层。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedLayer {
    /// 哪一层。
    pub layer: ArmorLayer,
    /// 为什么禁。
    pub reason: String,
}

/// 达到这个命中率才算"这一层能用"。
pub const USABLE_THRESHOLD: u8 = 80;
/// 高于 0 但低于阈值的，算"时灵时不灵"。
pub const PARTIAL_THRESHOLD: u8 = 1;

/// 从实测数字推出一条策略。
///
/// 规则（与实测矩阵得出的三条铁律一致）：
/// 1. **sys 层是唯一跨家稳定的面** —— 只要 sys 命中率够，就推荐 sys；
/// 2. **user 层只在少数家有效**，且**从不单独推荐**（压不住身份）；
/// 3. **seed 层**是"裸注入无效但带人设词能破"的那几家的正道。
pub fn plan(harness: &HarnessId, measurements: Option<&Measurements>) -> Plan {
    let id = harness.to_string();
    let Some(m) = measurements else {
        return Plan {
            harness: id,
            recommended_layer: ArmorLayer::Off,
            recommended_mode: "off".to_owned(),
            blocked: Vec::new(),
            preflight: vec!["先跑一次 `metis armor run` 拿到这家在本机的实测数字".to_owned()],
            verdict: "没有实测数据：不要猜，先测。".to_owned(),
            missing: Some("这家还没有 armor 实测数据".to_owned()),
        };
    };

    let sys = m.persona.sys.or(m.bare.sys);
    let user = m.persona.user.or(m.bare.user);
    let seed = m.persona.seed.or(m.bare.seed);

    let mut blocked = Vec::new();
    let mut preflight = Vec::new();

    // 选层：sys > seed > (user 只作为"知道它不顶用"的记录) > off。
    let (layer, mode, verdict) = if sys.is_some_and(|v| v >= USABLE_THRESHOLD) {
        (ArmorLayer::Sys, "replace", format!("sys 层命中率 {}%，这是唯一跨家稳定的面；用 replace 模式。", sys.unwrap_or(0)))
    } else if seed.is_some_and(|v| v >= USABLE_THRESHOLD) {
        (ArmorLayer::Seed, "append", format!("裸注入无效但带人设词种子命中 {}%；用 append 让人设随首条消息一起进。", seed.unwrap_or(0)))
    } else if sys.is_some_and(|v| v >= PARTIAL_THRESHOLD) {
        (ArmorLayer::Sys, "replace", format!("sys 层只有 {}%，时灵时不灵；可以试，但不要指望稳定。", sys.unwrap_or(0)))
    } else {
        (ArmorLayer::Off, "off", "没有可用的注入层：不要开，开了也压不住身份。".to_owned())
    };

    // 明确记录被排除的层与原因 —— 这是"能力诚实"（铁律 5）。
    if !sys.is_some_and(|v| v >= PARTIAL_THRESHOLD) {
        blocked.push(BlockedLayer {
            layer: ArmorLayer::Sys,
            reason: match sys {
                Some(0) => "sys 层实测命中 0%".to_owned(),
                Some(v) => format!("sys 层命中率只有 {v}%"),
                None => "sys 层没有测得可用注入点".to_owned(),
            },
        });
    }
    if let Some(v) = user {
        blocked.push(BlockedLayer {
            layer: ArmorLayer::User, reason: format!("user 层命中 {v}%；即使命中也是塞进用户消息，压不住身份，不作为能力提供")
        });
    }
    if layer != ArmorLayer::Seed && seed.is_some_and(|v| v >= PARTIAL_THRESHOLD) && layer != ArmorLayer::Seed {
        blocked.push(BlockedLayer { layer: ArmorLayer::Seed, reason: "已有更好的层，seed 不必启用".to_owned() });
    }

    if layer != ArmorLayer::Off {
        preflight.push("确认人设已选定且正文非空（`metis status`）".to_owned());
        preflight.push(format!("确认 {id} 的 sys 通道在本机可用（`metis harnesses` 看 `why`）"));
        preflight.push("跑一次 `metis verify` 确认人设真的进了请求（而不是只写了磁盘）".to_owned());
    }

    Plan { harness: id, recommended_layer: layer, recommended_mode: mode.to_owned(), blocked, preflight, verdict, missing: None }
}

/// 一次评测的原始结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchResult {
    /// 哪家。
    pub harness: String,
    /// 哪一层。
    pub layer: ArmorLayer,
    /// 跑了几轮。
    pub rounds: u32,
    /// 命中了几轮。
    pub hits: u32,
    /// 判定依据（`verify` 给的 `where`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_: Option<String>,
}

impl BenchResult {
    /// 命中率（0–100）。轮数为 0 时是 `None`（不是 0 —— 那会被误读成"测了但全失败"）。
    pub fn rate(&self) -> Option<u8> {
        if self.rounds == 0 {
            return None;
        }
        Some(((self.hits as f64 / self.rounds as f64) * 100.0).round() as u8)
    }
}

/// 把多次评测聚合成一家的实测数字。
pub fn judge(results: &[BenchResult], harness: &HarnessId) -> Measurements {
    let mut out = Measurements::default();
    for result in results.iter().filter(|r| r.harness == harness.as_str()) {
        let rate = result.rate();
        match result.layer {
            ArmorLayer::Off => {}
            ArmorLayer::Sys => out.bare.sys = rate,
            ArmorLayer::User => out.bare.user = rate,
            ArmorLayer::Seed => out.bare.seed = rate,
        }
    }
    out
}

/// 一张矩阵：多家 × 多层。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    /// 每家一列。
    pub rows: Vec<Row>,
}

/// 一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// 哪家。
    pub harness: String,
    /// 各层命中率（`None` = 没测）。
    pub rates: Vec<(ArmorLayer, Option<u8>)>,
    /// 建议的层。
    pub recommended: ArmorLayer,
}

impl Matrix {
    /// 从原始结果构建。
    pub fn build(results: &[BenchResult], harnesses: &[HarnessId]) -> Self {
        let rows = harnesses
            .iter()
            .map(|harness| {
                let measurements = judge(results, harness);
                let plan = plan(harness, Some(&measurements));
                let rates = ArmorLayer::ALL.iter().map(|layer| (*layer, measurements.persona.get(*layer).or_else(|| measurements.bare.get(*layer)))).collect();
                Row { harness: harness.to_string(), rates, recommended: plan.recommended_layer }
            })
            .collect();
        Self { rows }
    }

    /// 人类可读的表格（对齐的纯文本，不依赖终端颜色）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("harness              off   sys   user  seed  → 建议\n");
        out.push_str("------------------------------------------------------\n");
        for row in &self.rows {
            let cell = |layer: ArmorLayer| -> String {
                match row.rates.iter().find(|(l, _)| *l == layer).and_then(|(_, v)| *v) {
                    Some(v) => format!("{v:>3}%"),
                    None => "   —".to_owned(),
                }
            };
            out.push_str(&format!(
                "{:<20} {} {} {} {}  → {}\n",
                row.harness,
                cell(ArmorLayer::Off),
                cell(ArmorLayer::Sys),
                cell(ArmorLayer::User),
                cell(ArmorLayer::Seed),
                row.recommended.as_str()
            ));
        }
        out
    }
}

/// `preflight` 的一项检查结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightItem {
    /// 查了什么。
    pub what: String,
    /// 过没过。
    pub ok: bool,
    /// 说明。
    pub detail: String,
}

/// 跑预检。
///
/// 只查**本机能查的**：不动网络、不起宿主。真正"人设进没进请求"要 `verify`。
pub fn preflight(plan: &Plan, channel_usable: bool, persona_ready: bool) -> Vec<PreflightItem> {
    let mut items = Vec::new();
    items.push(PreflightItem {
        what: "人设已选定且正文非空".to_owned(),
        ok: persona_ready,
        detail: if persona_ready { "人设就绪".to_owned() } else { "还没有选定人设，或正文为空；先 `metis use <id>`".to_owned() },
    });
    items.push(PreflightItem {
        what: format!("{} 的 sys 通道可用", plan.harness),
        ok: channel_usable,
        detail: if channel_usable {
            "通道可用".to_owned()
        } else {
            "这家在本机没有可用的注入通道；`metis harnesses` 能看到原因".to_owned()
        },
    });
    if plan.recommended_layer == ArmorLayer::Off {
        items.push(PreflightItem { what: "建议不开注入".to_owned(), ok: true, detail: plan.verdict.clone() });
    }
    items
}

/// 预检是否全部通过。
pub fn preflight_ok(items: &[PreflightItem]) -> bool {
    items.iter().all(|item| item.ok)
}

/// `run` 的编排参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    /// 哪家。
    pub harness: HarnessId,
    /// 跑哪几层。
    pub layers: Vec<ArmorLayer>,
    /// 每层跑几轮。
    pub rounds: u32,
}

impl RunSpec {
    /// 默认：sys / user / seed 各 2 轮（off 没什么可测的）。
    pub fn default_for(harness: HarnessId) -> Self {
        Self { harness, layers: vec![ArmorLayer::Sys, ArmorLayer::User, ArmorLayer::Seed], rounds: 2 }
    }

    /// 总请求数（用户要拿它估模型额度）。
    pub fn total_rounds(&self) -> u32 {
        self.layers.len() as u32 * self.rounds
    }
}

/// 把 `verify` 的一次结论折算成一层的一轮。
///
/// **关键**：只有 `verify` 判 `ok`（落在 sys/developer 层）才算命中；
/// `user-only` 与 `no-hit` 都不算。这是"能力诚实"的底线。
pub fn outcome_to_bench(harness: &HarnessId, layer: ArmorLayer, where_: Option<&str>, ok: bool) -> BenchResult {
    BenchResult { harness: harness.to_string(), layer, rounds: 1, hits: u32::from(ok), where_: where_.map(str::to_owned) }
}

/// 已记录的原始结果（落盘在 `armor.json`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    /// 格式版本。
    #[serde(default = "one")]
    pub v: u32,
    /// 原始评测结果（保留全部，不做聚合 —— 聚合是可重算的）。
    #[serde(default)]
    pub results: Vec<BenchResult>,
}

fn one() -> u32 {
    1
}

impl Store {
    /// 读；文件不存在或损坏时返回空表（诊断材料，不该阻断主流程）。
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let Some(text) = metis_fs::read_to_string_optional(path)? else { return Ok(Self { v: 1, results: Vec::new() }) };
        match serde_json::from_str::<Self>(&text) {
            Ok(store) => Ok(store),
            // 损坏就当作空表并保留原件：让用户还能手工看。
            Err(_) => Ok(Self { v: 1, results: Vec::new() }),
        }
    }

    /// 写。
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| metis_core::MetisError::new(metis_core::ErrorKind::Internal, "armor 数据无法序列化").with_detail(error.to_string()))?;
        bytes.push(b'\n');
        metis_fs::write_atomic(path, &bytes)
    }

    /// 追加一条结果。
    pub fn record(&mut self, result: &BenchResult) {
        self.results.push(result.clone());
    }

    /// 某一家的实测数字。
    pub fn get(&self, harness: &HarnessId) -> Option<Measurements> {
        let mine: Vec<&BenchResult> = self.results.iter().filter(|r| r.harness == harness.as_str()).collect();
        if mine.is_empty() {
            return None;
        }
        // 同一层取**最近一次**（用户重测过就以新的为准）。
        let latest = |layer: ArmorLayer| -> Option<u8> { mine.iter().rev().find(|r| r.layer == layer).and_then(|r| r.rate()) };
        Some(Measurements {
            bare: LayerRates { sys: latest(ArmorLayer::Sys), user: latest(ArmorLayer::User), seed: latest(ArmorLayer::Seed) },
            persona: LayerRates::default(),
        })
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    #[test]
    fn a_missing_file_is_an_empty_store() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = Store::load(&dir.path().join("nope.json")).expect("load");
        assert!(store.results.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("armor.json");
        let mut store = Store::default();
        store.record(&BenchResult { harness: "codex".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 2, where_: None });
        store.save(&path).expect("save");
        let back = Store::load(&path).expect("load");
        assert_eq!(back.results.len(), 1);
    }

    #[test]
    fn the_latest_measurement_wins() {
        let mut store = Store::default();
        store.record(&BenchResult { harness: "codex".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 0, where_: None });
        store.record(&BenchResult { harness: "codex".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 2, where_: None });
        assert_eq!(store.get(&id("codex")).and_then(|m| m.bare.sys), Some(100), "重测应以新的为准");
    }

    #[test]
    fn an_unmeasured_harness_has_no_entry() {
        let store = Store::default();
        assert!(store.get(&id("codex")).is_none());
    }

    #[test]
    fn a_corrupt_file_does_not_break_the_command() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("armor.json");
        std::fs::write(&path, b"{ broken").expect("seed");
        assert!(Store::load(&path).expect("load").results.is_empty(), "损坏时当空表，不阻断");
        assert!(path.is_file(), "原件要留着供人工检查");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    fn rates(sys: Option<u8>, user: Option<u8>, seed: Option<u8>) -> LayerRates {
        LayerRates { sys, user, seed }
    }

    #[test]
    fn layers_parse_and_round_trip() {
        for layer in ArmorLayer::ALL {
            assert_eq!(ArmorLayer::parse(layer.as_str()), Some(layer));
        }
        assert_eq!(ArmorLayer::parse("SYS"), Some(ArmorLayer::Sys));
        assert_eq!(ArmorLayer::parse("nope"), None);
    }

    #[test]
    fn without_measurements_the_plan_says_measure_first() {
        // 铁律 4：没测过就绝不能给"能用"的结论。
        let plan = plan(&id("codex"), None);
        assert_eq!(plan.recommended_layer, ArmorLayer::Off);
        assert_eq!(plan.recommended_mode, "off");
        assert!(plan.missing.is_some());
        assert!(plan.verdict.contains("先测"));
    }

    #[test]
    fn a_usable_sys_layer_is_recommended_with_replace() {
        let m = Measurements { bare: rates(Some(90), None, None), persona: rates(Some(95), None, None) };
        let plan = plan(&id("codex"), Some(&m));
        assert_eq!(plan.recommended_layer, ArmorLayer::Sys);
        assert_eq!(plan.recommended_mode, "replace");
        assert!(plan.verdict.contains("95%"));
    }

    #[test]
    fn a_zero_sys_layer_is_blocked_with_the_number() {
        let m = Measurements { bare: rates(Some(0), None, None), persona: rates(Some(0), None, None) };
        let plan = plan(&id("claude"), Some(&m));
        assert_eq!(plan.recommended_layer, ArmorLayer::Off);
        let sys = plan.blocked.iter().find(|b| b.layer == ArmorLayer::Sys).expect("sys blocked");
        assert!(sys.reason.contains("0%"), "理由里要有数字：{}", sys.reason);
    }

    #[test]
    fn seed_is_the_answer_when_bare_fails_but_persona_words_work() {
        // 这正是 claude 的情况：裸注入无效、带人设词种子才破。
        let m = Measurements { bare: rates(Some(0), Some(0), None), persona: rates(Some(0), None, Some(90)) };
        let plan = plan(&id("claude"), Some(&m));
        assert_eq!(plan.recommended_layer, ArmorLayer::Seed);
        assert_eq!(plan.recommended_mode, "append");
    }

    #[test]
    fn the_user_layer_is_never_recommended_even_at_100_percent() {
        // 铁律 5：user 层压不住身份，命中率再高也不作为能力推荐。
        let m = Measurements { bare: rates(Some(0), Some(100), None), persona: rates(Some(0), Some(100), None) };
        let plan = plan(&id("pi"), Some(&m));
        assert_ne!(plan.recommended_layer, ArmorLayer::User);
        assert_eq!(plan.recommended_layer, ArmorLayer::Off);
        assert!(plan.blocked.iter().any(|b| b.layer == ArmorLayer::User));
    }

    #[test]
    fn a_partial_sys_layer_is_allowed_but_flagged() {
        let m = Measurements { bare: rates(Some(50), None, None), persona: rates(Some(50), None, None) };
        let plan = plan(&id("omp"), Some(&m));
        assert_eq!(plan.recommended_layer, ArmorLayer::Sys);
        assert!(plan.verdict.contains("时灵时不灵"), "{}", plan.verdict);
    }

    #[test]
    fn a_plan_with_a_real_layer_carries_preflight_items() {
        let m = Measurements { bare: rates(Some(95), None, None), persona: rates(Some(95), None, None) };
        let plan = plan(&id("codex"), Some(&m));
        assert!(plan.preflight.len() >= 3);
        assert!(plan.preflight.iter().any(|p| p.contains("verify")), "必须提醒去实测");
    }

    #[test]
    fn bench_rate_handles_zero_rounds() {
        // 0 轮 ≠ 0% 命中：前者是"没测"，后者是"测了全失败"。
        let zero = BenchResult { harness: "x".to_owned(), layer: ArmorLayer::Sys, rounds: 0, hits: 0, where_: None };
        assert_eq!(zero.rate(), None);
        let none_hit = BenchResult { harness: "x".to_owned(), layer: ArmorLayer::Sys, rounds: 4, hits: 0, where_: None };
        assert_eq!(none_hit.rate(), Some(0));
    }

    #[test]
    fn bench_rate_rounds_to_percent() {
        let r = BenchResult { harness: "x".to_owned(), layer: ArmorLayer::Sys, rounds: 3, hits: 2, where_: None };
        assert_eq!(r.rate(), Some(67));
    }

    #[test]
    fn judge_collects_only_the_requested_harness() {
        let results = vec![
            BenchResult { harness: "codex".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 2, where_: None },
            BenchResult { harness: "claude".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 0, where_: None },
        ];
        let m = judge(&results, &id("codex"));
        assert_eq!(m.bare.sys, Some(100));
        assert_eq!(m.bare.user, None);
    }

    #[test]
    fn outcome_to_bench_only_counts_ok_as_a_hit() {
        // user-only 不算命中 —— 这是整件事的底线。
        let ok = outcome_to_bench(&id("codex"), ArmorLayer::Sys, Some("developer"), true);
        assert_eq!(ok.hits, 1);
        let user_only = outcome_to_bench(&id("codex"), ArmorLayer::User, Some("user"), false);
        assert_eq!(user_only.hits, 0);
    }

    #[test]
    fn matrix_renders_a_readable_table() {
        let results = vec![
            BenchResult { harness: "codex".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 2, where_: None },
            BenchResult { harness: "pi".to_owned(), layer: ArmorLayer::Sys, rounds: 2, hits: 0, where_: None },
            BenchResult { harness: "pi".to_owned(), layer: ArmorLayer::User, rounds: 2, hits: 1, where_: None },
        ];
        let matrix = Matrix::build(&results, &[id("codex"), id("pi")]);
        let text = matrix.render();
        assert!(text.contains("harness"), "要有表头：{text}");
        assert!(text.contains("100%"), "codex 的 sys 是 100%：{text}");
        assert!(text.contains("codex"));
        assert!(text.contains("pi"));
        // 每家的建议层都要出现在行里。
        for row in &matrix.rows {
            assert!(text.contains(row.recommended.as_str()), "{} 的建议层缺失", row.harness);
        }
    }

    #[test]
    fn matrix_marks_untested_layers_with_a_dash() {
        let matrix = Matrix::build(&[], &[id("codex")]);
        assert!(matrix.render().contains('—'), "没测过的层要显示 —");
    }

    #[test]
    fn preflight_reports_each_missing_precondition() {
        let plan = plan(&id("codex"), Some(&Measurements { bare: rates(Some(95), None, None), persona: LayerRates::default() }));
        let items = preflight(&plan, false, false);
        assert!(!preflight_ok(&items));
        assert!(items.iter().any(|i| !i.ok && i.detail.contains("metis use")));
        assert!(items.iter().any(|i| !i.ok && i.detail.contains("harnesses")));
    }

    #[test]
    fn preflight_passes_off_plans_without_a_persona() {
        // 结论是"不要开"时，没有人设也不算失败。
        let plan = plan(&id("x"), Some(&Measurements::default()));
        let items = preflight(&plan, false, false);
        // 仍然会报通道不可用，但 off 的结论本身是通过的。
        assert!(items.iter().any(|i| i.what.contains("建议不开") && i.ok));
    }

    #[test]
    fn run_spec_counts_the_model_calls_it_will_need() {
        // 用户要能预估额度：3 层 × 2 轮 = 6 次。
        let spec = RunSpec::default_for(id("codex"));
        assert_eq!(spec.layers.len(), 3);
        assert_eq!(spec.rounds, 2);
        assert_eq!(spec.total_rounds(), 6);
        assert!(!spec.layers.contains(&ArmorLayer::Off), "off 没有可测的东西");
    }

    #[test]
    fn plan_serialises_with_stable_field_names() {
        let m = Measurements { bare: rates(Some(95), None, None), persona: LayerRates::default() };
        let plan = plan(&id("codex"), Some(&m));
        let json = serde_json::to_value(&plan).expect("json");
        assert_eq!(json["harness"], "codex");
        assert_eq!(json["recommended_layer"], "sys");
        assert!(json.get("blocked").is_some() || json.get("preflight").is_some());
    }
}
