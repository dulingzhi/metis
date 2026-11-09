# 测试策略与全覆盖契约

> 相关：[AGENTS.md 的单测契约](../AGENTS.md#单测全覆盖契约) · [架构](ARCHITECTURE.md) · [路线图](ROADMAP.md)

> 状态：设计已定，代码未落地（M0）。基线是 agent-purge 的 **28 个顶层 `test/*.test.mjs`**
> + `test/magpie/` 2 个 + `test/browser/console.spec.mjs` 1 个（Playwright，3 浏览器 × 含 axe 无障碍）。
> 本文件说明这些契约在 Rust 里怎么落地、怎么**一个都不丢**地覆盖。
>
> 注意：覆盖率门禁（`cargo xtask coverage`）是 **Metis 新增**的，参考项目没有对应实现可借鉴 ——
> 它由本文件定义，M0.5 实现。

## 1. 门槛

| 指标 | 门槛 | 统计方式 |
|---|---|---|
| `crates/*` 行覆盖率 | ≥ 90% | `cargo xtask coverage`（cargo-llvm-cov） |
| `crates/*` 分支覆盖率 | ≥ 80% | **人工检查**：`cargo llvm-cov` 报告里有 Branches 列，但 cargo-llvm-cov **没有** `--fail-under-branches`（只有 lines / functions / regions），所以这一项无法自动设门槛 |
| `apps/metis`（CLI）行覆盖率 | ≥ 85% | 同上（含端到端用例） |
| `crates/ui-egui` 行覆盖率 | ≥ 70% | 同上，纯绘制分支单独排除并登记 |
| 每个 `Command` 实现 | 至少 1 条成功 + 1 条失败测试 | 由 `xtask ci` 用注册表核对 |
| 每个能力表 `why` 文案 | 至少 1 条断言 | 由 `xtask ci` 核对键集合 |

**未覆盖的分支必须在 PR 里逐条说明原因**（例如"仅在 Windows 上可达，由 `#[cfg(windows)]` 用例覆盖"）。
放宽门槛需要 maintainer 明确同意，并在 `ROADMAP.md` 里留一条 decision 记录。

## 2. 测试分层

| 层 | 位置 | 允许 | 禁止 |
|---|---|---|---|
| 单元 | 各 crate 的 `#[cfg(test)] mod tests` 或 `tests/` | 纯函数、解析、状态机、路径解析 | 真实 `$HOME`、真实 harness、网络 |
| 集成 | `crates/<crate>/tests/` | 临时目录里的完整文件操作、假 harness 可执行文件 | 真实用户配置 |
| Golden | `crates/channel/tests/golden/`、`crates/persona/tests/golden/` | 逐字节比对生成的产物与解析结果 | 手工改 golden 而不说明 |
| 无头 UI | 各 crate 的 `#[cfg(test)]`（直接用 `egui::Context::run_ui`） | 渲染不 panic、绘制不改状态 | 需要 GPU 的测试框架（没有显示设备时会挂死）、业务判定 |
| 端到端 | `apps/metis/tests/` | 子进程跑 CLI，断言 stdout/stderr/退出码/磁盘结果 | 真实 harness |
| 真机（可选） | `#[ignore]` + `docs/TESTING.md` 登记表 | 真实 harness、真实抓包 | 在 CI 默认运行 |

## 3. `metis-testkit`：测试夹具

```rust
/// 临时数据根：自动清理，自动注入 METIS_HOME，保证测试碰不到真实用户目录。
pub struct TempHome { /* Drop 时清理 */ }
impl TempHome {
    pub fn new() -> Self;
    pub fn path(&self) -> &Path;
    pub fn env(&self) -> Vec<(String, String)>;   // 给子进程用
    pub fn state(&self) -> StatePath;
    pub fn personas(&self) -> PersonaDir;
    pub fn write_state(&self, json: &str) -> Result<()>;
}

/// 假 harness：在临时目录生成一个真能跑的可执行文件（Windows 用 .cmd + 一个小 exe，Unix 用 shell 脚本），
/// 行为由参数决定：打印、睡觉、读 stdin、返回退出码、写一个标记文件。
pub struct FakeHarness { /* … */ }
impl FakeHarness {
    pub fn sleep(ms: u64) -> Self;          // 用于超时测试
    pub fn echo(text: &str) -> Self;        // 用于输出排空与上限测试
    pub fn exit(code: i32) -> Self;
    pub fn read_stdin_forever() -> Self;    // 用于"stdin 不被继承"测试
    pub fn install_into(&self, dir: &Path) -> PathBuf;  // 放进假的 PATH
}

/// 假 provider：进程内 HTTP 服务，只绑 127.0.0.1:0，记录收到的请求体。
pub struct FakeProvider { /* … */ }
impl FakeProvider {
    pub fn start() -> Self;
    pub fn base_url(&self) -> String;
    pub fn requests(&self) -> Vec<CapturedRequest>;
}

/// Golden 比对：失败时打印统一 diff，并给出 UPDATE_GOLDEN=1 的重建提示。
pub fn assert_golden(path: &Path, actual: &[u8]);

/// 注入时钟：让"保留 6 个月""轮转""超时"之类的逻辑可确定性测试。
pub struct TestClock { /* … */ }
```

要求：

- `TempHome` 必须在 `Drop` 时清理；清理失败只警告，不 panic。
- 夹具自身也要有测试（它在 `crates/` 里，受同一个覆盖率门槛约束）。
- 夹具不得依赖真实 harness、不得依赖网络、不得依赖系统时区。

## 4. 基线的测试文件 → 覆盖映射（一个都不能丢）

| 基线文件 | 钉住的行为 | Metis 落点 |
|---|---|---|
| `activation.test.mjs` | 激活解析的短路顺序、off 的优先级、来源判定 | `core::activation` 单元 + `engine::activation` 集成 |
| `apply.test.mjs` | apply 的分支、报告字段、`partial`、先发布 off | `engine::apply::tests` |
| `lifecycle.test.mjs` | 选择→暂停→恢复→撤销的完整生命周期 | `engine::lifecycle` 集成 |
| `launch.test.mjs` | 启动参数生成、`--dry-run`、`--` 透传 | `harness::launch` + `apps/metis/tests/cli.rs` |
| `backends.test.mjs` | 每家 harness 的启动参数形状 | `harness::launch::tests`（每家一组用例） |
| `harness-adapters.test.mjs` | 适配器契约与真实参数形状 | `harness::adapters` 集成 + 真机登记 |
| `regressions.test.mjs` | 历史回归集合 | 拆成 `crates/*/tests/regressions.rs` 对应条目 |
| `hot-switch.test.mjs` | 热切语义（下一轮 vs 需重开会话） | `harness::caps` 的 `hot` 分支测试 |
| `spawn.test.mjs` | 进程启动、参数、环境、退出码 | `process::tests` |
| `private-server.test.mjs` | 私有服务/端口占用处理 | `process::provider` 测试 |
| `footprints.test.mjs` | 账本幂等、所有者匹配、路径大小写 | `fs::footprint` 单元 |
| `restore.test.mjs` | 交换/恢复算法、外部修改检测、journal 回收 | `fs::swap` 单元（覆盖 §6 的每一步） |
| `safety.test.mjs` | 不能让环境变量/token 直接授予许可；不覆盖用户修改 | `engine::safety` 集成 |
| `project.test.mjs` | 项目级托管块、登记目录、逐字节还原 | `channel::project` 集成 |
| `locate.test.mjs` | 各 harness 的定位与 PATH 解析 | `harness::detect` + `paths` |
| `detect.test.mjs` | 探测结果结构、版本解析、未安装 | `harness::detect` 集成 |
| `persona-import.test.mjs` | 批量导入、去重、非法文件、覆盖开关 | `persona::import` 集成 |
| `update.test.mjs` | 版本比较、更新检查、下载、失败路径 | `engine::update`（假 HTTP） |
| `verification.test.mjs` | 验证报告的读写与判定 | `engine::verification` |
| `capture.test.mjs` | 抓包判定：marker 落在哪层、`off` 时为零命中 | `process::provider` + `engine::verify` |
| `web.test.mjs` | Web API 的每个端点与错误码 | **有意不迁移**：改为 GUI 命令的集成测试（见 §6） |
| `window.test.mjs` | 启动窗口/终端行为（Windows `windowsHide`） | `process::launch` 的 `#[cfg(windows)]` 用例 |
| `release.test.mjs` | 打包清单、版本一致性 | `xtask ci` 的版本一致性作业 |
| `repository.test.mjs` | 仓库结构约束（文件清单、无大文件、无进程名） | `xtask ci` 的仓库检查作业 |
| `kilocode.test.mjs` | Kilo 的 `code.md` 与 `AGENTS.md` 两处落点 | `channel` + `harness::kilo` 集成 |
| `pi-desktop.test.mjs` | PI-Desktop 的 `SYSTEM.md` 与 imported 插件 | `channel::persist::pi_desktop` |
| `workbuddy.test.mjs` | WorkBuddy `settings.json` 字段与用户目录 | `channel::persist::workbuddy` |
| `zcode-real.test.mjs` | zcode 真机行为 | `#[ignore]` 真机登记（Metis 不提供二进制补丁，见 `PARITY.md`） |
| `magpie/hooks.test.mjs` | magpie 插件的钩子 | **有意不迁移**：magpie 是第三方网关注解，见 `PARITY.md` |
| `magpie/page.test.mjs` | magpie 插件的页面 | 同上 |
| `browser/console.spec.mjs`（Playwright） | 控制台端到端与无障碍 | `crates/ui-egui/src/screens.rs` 的无头渲染测试（4 个屏幕 + 状态稳定性） |

**核对机制**：`cargo xtask ci` 会读取 `docs/TESTING.md` 里这张表，确认每一行的落点文件真实存在且包含测试；
缺失即 CI 失败。这样"漏迁一条契约"不可能悄悄发生。

## 5. 必测的边界清单（跨 crate）

- **路径**：Windows 大小写不敏感、UNC 路径、超长路径（`\\?\`）、含空格/中文/emoji 的目录、符号链接、
  `PATH` 里同名命令有多个。
- **文本**：CRLF、BOM、无末尾换行、块标记不独立成行、文件里已有相似但不属于我们的块、超长行。
- **并发**：两个进程同时 apply（第二个拿到 `LockHeld` 并给出 owner pid）、交换 journal 同时被两个进程看到、
  死锁文件被正确回收。
- **崩溃**：写入中途 kill -9 → 下次启动回收 journal；无需人工干预。
- **能力**：每家 harness 在"已装/未装/只有桌面版/没有 profile"四种状态下，四条通道的 `ok` 与 `why`。
- **错误**：每个 `ErrorKind` 至少一条用例，断言 `remediation` 非空且包含可操作内容。

## 6. 有意不迁移的测试（必须在 `PARITY.md` 有对应条目）

| 不迁移 | 理由 | 由什么替代 |
|---|---|---|
| `web.test.mjs` 的 HTTP 端点 | Metis 不提供本地服务 | GUI 命令层的集成测试 + `metis --json` 的机器可读输出测试 |
| Playwright 浏览器用例 | 没有浏览器端 | 无头渲染测试（`egui::Context::run_ui`） |
| `magpie/*` | magpie 是第三方网关的插件形态，与 Metis 的产品边界无关 | 无（在 `PARITY.md` 标"不需要"） |
| `release.test.mjs` 的 npm 打包部分 | 没有 npm 包 | `xtask ci` 的产物清单与版本一致性检查 |

## 7. CI 矩阵

| 作业 | 内容 |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `cargo test --workspace`（矩阵：`windows-latest`、`ubuntu-latest`、`macos-latest`；stable + MSRV） |
| `coverage` | `cargo xtask coverage`，未达门槛失败；上传报告 |
| `layers` | `cargo xtask layers` |
| `secrets` | `cargo xtask secrets`（含历史提交） |
| `docs` | 每条铁律都有对应检查/测试（`xtask ci --docs`）、`ROADMAP` 无僵尸条目、TESTING 映射表可解析 |
| `msrv` | 用 `rust-version` 指定的版本编译 |

平台差异用例用 `#[cfg(windows)]` / `#[cfg(unix)]` 标注，不靠运行时跳过。

## 8. 真机验证登记表

需要真实 harness 的验证**不放进 CI**，用 `#[ignore]` 标注并在本表登记。每次跑完把结果填进 `STATUS.md`。

| 用例 | 需要 | 最近结果 |
|---|---|---|
| `hub::tests::live_hub_list_and_detail_match_the_contract` | 能访问 `https://n.kkpt.tech:8443` | **2026-11-09 通过**：真实列表非空、详情 prompt 非空；默认 CI 用注入式假 fetch，不联网 |
| `hub_import::tests::live_import_of_an_expert_with_skills` | 能访问 `https://n.kkpt.tech:8443` | **2026-11-09 通过**：真实导入一个带关联技能的 expert，人设与 `skills/<slug>/SKILL.md` 都落到临时 home |
| `verify` 抓包（每家在用的 harness） | 本机已装该 harness | 未跑（M0） |
| `armor run/judge` | 本机已装该 harness + 模型可用 | 未跑（M0） |
| PI-Desktop 的 imported 插件热切 | PI-Desktop 便携版 | 未跑（M0） |
| DSH 三个 profile 的插件注入 | DSH 已初始化 profile | 未跑（M0） |

## 9. 测试纪律（违反即 review 不通过）

- 不写只断言 `is_ok()` 的测试；要断言**具体值**。
- 测试之间不得有顺序依赖；每个测试自带完整前置。
- 不得依赖墙钟、时区、随机数（需要时注入固定种子/时钟）。
- 不得把"期望值"写成从被测代码算出来的值（同义反复）。
- 新增行为必须**先**有一条会失败的测试（至少在 PR 描述里说明失败现象）。
- 修 bug 必须带回归测试，并指向 issue 或 `PITFALLS.md` 条目。
- 测试失败信息要能直接定位：带路径、期望值、实际值。
