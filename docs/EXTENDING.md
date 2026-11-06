# 扩展指南：每种改动该动哪里

> 相关：[AGENTS.md](../AGENTS.md) · [架构](ARCHITECTURE.md) · [测试](TESTING.md) · [路线图](ROADMAP.md)

> 目的：让"加一个东西"变成填空题，而不是考古。每一节都给出**要改的文件清单**、**必须带的测试**，
> 以及**不要碰的地方**。
>
> 通用规则（`AGENTS.md`）：扩展走接缝、数据优先于代码、结构化结果、落盘带版本、把代码放在能拥有它的最低层。

---

## 1. 加一家 harness

**触发**：出现一个新的 coding agent，需要被 Metis 管理。

**改这些**：

1. `crates/harness/src/registry.rs` — 在注册表加一项，把这家接进系统。注册表是**一处**，
   不要在别处写 `if id == "xxx"`。
2. `crates/harness/src/<name>.rs` — 新建一个文件，实现 `Harness` trait：
   - `id()` / `display_name()`；
   - `detect()`：可执行名、常见安装路径、主目录环境变量、标记文件、版本命令、桌面 exe 名；
   - `targets()`：install 落点（文件、层、写入方式、托管块标记）、slash 产物、project 文件名；
   - `launch()`：交互式与一次性（oneshot）的参数生成，包括"参数不能塞进命令行"这类约束；
   - `hot_switch()`：`None` 或 `{kind, needs, how}`；不支持时给出原因。
3. `crates/harness/src/caps.rs` — 若这家有特殊的"为什么不能用"（文件落错层、没有 profile、
   桌面优先等），在这里加数据条目，**不要写在 `if` 里**。文案用键（i18n）。
4. `crates/paths/src/harness.rs` — 加环境变量与默认主目录。
5. `docs/HARNESS-MATRIX.md` — 在总览表与逐家说明里加一行。
6. `docs/PARITY.md` — 若基线里有这家，把对应条目状态推进。
7. `crates/harness/tests/<name>.rs` — 测试：
   - 探测的四种状态（已装 / 未装 / 只有桌面版 / 缺 profile）；
   - 每个通道的 `ok` 与 `why`（`why` 文案必须有断言）；
   - 启动参数形状（交互式 + oneshot + `off`）；
   - 落点路径（注入假 HOME 后逐条比对）。
8. 若这家有 `install` 落点：`crates/channel/tests/<name>.rs` — apply → 幂等 apply → revert 三步，
   断言磁盘**逐字节**还原；再补一条"外部修改后 revert 报冲突"的用例。
9. 若这家需要宿主侧的 JS/TS 产物：`crates/channel/assets/host/<name>.*` 模板 + golden 测试。

**不要碰**：`crates/channel/src/*` 的通用逻辑、`crates/engine/src/apply.rs`、任何 UI 文件。

---

## 2. 加一条通道

**触发**：出现一种新的注入方式（例如某个宿主的"环境变量注入"）。

**改这些**：

1. `crates/core/src/channel.rs` — 加枚举项与序列化值（顺序即 UI 展示顺序）。
2. `crates/channel/src/registry.rs` — 注册实现。
3. `crates/channel/src/<name>.rs` — 实现 `ChannelImpl`：`plan` / `apply` / `revert` / `status`。
   要求：
   - `apply` 幂等；
   - 写盘前先记 footprint；
   - `revert` 只回滚自己写过的字节；
   - 失败返回 `MetisError`，`remediation` 非空。
4. `crates/core/src/effect.rs` — 定义这条通道的生效时机（`next-turn` / `next-launch` / `next-session`）。
5. `docs/ARCHITECTURE.md` §6、`docs/DATA-FORMAT.md` §5（若引入新的托管块标记）、
   `docs/UI.md`（界面上的开关文案）。
6. 测试：`crates/channel/tests/<name>.rs` —— 三步（apply / 幂等 / revert）+ 冲突 + 权限失败 +
   目标不存在；再加一条 `crates/engine` 级别的"apply 报告里这条通道的 `effect` 正确"。

**不要碰**：`metis-harness` 里 15 家 harness 的文件（通道是通用机制）。

---

## 3. 加一个 CLI 命令 / 一个 GUI 动作

**规则：先有命令，再有界面。** 界面不得自己实现业务。

**改这些**：

1. `crates/engine/src/commands/<area>.rs` — 加一个实现 `Command` 的类型：
   ```rust
   pub struct UsePersona;
   impl Command for UsePersona {
       fn id(&self) -> &'static str { "persona.use" }
       fn title(&self) -> &'static str { "…" }          // i18n 键
       fn params_schema(&self) -> schemars::Schema { … } // 参数结构
       fn effects(&self) -> Effects { … }                // 会写哪些路径、是否起进程、是否联网
       fn run(&self, ctx: &EngineCtx, params: &Value) -> Result<CommandOutput, MetisError> { … }
   }
   ```
2. `crates/engine/src/commands/mod.rs` — 注册一行。
3. `apps/metis/src/cli.rs` — 加 clap 子命令，把它映射到命令 id 与参数。**不写业务分支。**
4. `crates/ui-egui/src/<screen>.rs` — 若要暴露到界面，调同一个命令 id。
5. `crates/ui-egui/src/lib.rs` — 在 `ACTIONS` 表里登记这个界面动作对应的命令 id（否则
   `cargo xtask layers` 的"UI 动作必须有 headless 对应物"检查会失败）。
6. `docs/PARITY.md` — 若对应基线行为，推进状态。
7. 测试：命令层的成功 + 失败各一条；CLI 的端到端一条（断言 stdout/退出码/磁盘）；
   若暴露到界面，一条 `egui_kittest` 交互测试。

---

## 4. 加一个 GUI 屏幕

**改这些**：

1. `crates/ui-egui/src/state.rs` — `Screen` 加一项。
2. `crates/ui-egui/src/<screen>.rs` — `pub fn show(app: &mut MetisApp, ui: &mut egui::Ui)`。
3. `crates/ui-egui/src/lib.rs` — 在 `show()` 的 `match` 里加一个分支（分派只在这一处）。
4. `crates/ui-egui/src/sidebar.rs` — 导航项（含图标名，图标要在 `USED_ICONS` 里）。
5. `crates/ui-egui/src/i18n/zh-Hans.tsv` + `en.tsv` — 新文案的键。
6. 测试：一条渲染 + 一条关键交互 + 一张快照（快照变动要在 PR 说明）。

**不要碰**：引擎的任何文件。屏幕里出现的数字与判定都必须来自命令的返回。

---

## 5. 加一种人设元数据字段

1. `crates/persona/src/meta.rs` — 加字段（`#[serde(default)]`，保证旧文件能读）。
2. `crates/persona/src/parse.rs` — 解析与序列化都要支持；`desc` 这类旧键走别名。
3. `crates/ui-egui/src/library.rs` — 编辑器加输入框。
4. `docs/DATA-FORMAT.md` §3 — 更新字段表。
5. 测试：带该字段 / 不带该字段 / 值类型错误 / 未知键保留 四种用例。

**必须记住**：保存时要保留表单未涉及的元数据。加字段不能破坏这条。

---

## 6. 加一种错误

1. `crates/core/src/error.rs` — `ErrorKind` 加一项 + 稳定编号 + `retryable()`。
2. i18n 表 — `message` 键与 `remediation` 键（`remediation` 必须能照做）。
3. `docs/ARCHITECTURE.md` §3 / `docs/PITFALLS.md`（如果它来自一次真实事故）。
4. 测试：至少一条用例触发它，断言 `kind`、`message` 键、`remediation` 非空。

---

## 7. 改一个落盘格式 / 加一次迁移

1. `docs/DATA-FORMAT.md` — 先改文档（我们按文档写代码）。
2. `crates/<crate>/src/versioned.rs` — 加版本常量与迁移步骤（每步一个函数，可单独测试）。
3. 迁移步骤必须**幂等**；迁移前把原文件复制到 `backups/`。
4. 测试：
   - 从旧版本文件迁移到新版本；
   - **连续跑两次**，第二次不改任何字节；
   - 损坏输入 → 原文件字节不变。

---

## 8. 加一种语言

1. `crates/ui-egui/src/i18n/<lang>.tsv` — 复制 `zh-Hans.tsv`，翻译全部键。
2. `crates/ui-egui/src/i18n.rs` — 注册语言并加进语言选择列表。
3. 运行 `cargo xtask i18n-coverage`：键集合双向核对，缺一个即失败。
4. 快照测试跑一遍该语言，确认没有截断与溢出。

---

## 9. 加一个工程检查

1. `xtask/src/main.rs` — 加子命令，并在 `cargo xtask ci` 的链条里插进正确位置。
2. `AGENTS.md` 的"完成前检查"清单加一行。
3. 该检查必须**能失败**：加一条自测（用一个故意的反例验证它会红）。
4. `docs/PITFALLS.md` — 写清它防的是哪个具体事故。

---

## 10. 加一个 xtask 之外的自动化

只有一条规则：**能被机器检查的规则，就不要只写在文档里**。
如果一条规则无法自动化（例如"不要复制上游文案"），把它写进 `PITFALLS.md` 并在 code review 里人工把关。
