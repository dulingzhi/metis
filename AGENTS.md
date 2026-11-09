# AGENTS.md：Metis 的 AI 代理与贡献者指南

Metis 是一个**纯 Rust、原生、单可执行文件**的本地工具：一份人设（persona），统一管理 Codex、Claude Code、
OpenCode、pi、DSH 等 15 家 coding agent 的系统提示词。它同时提供 CLI（`metis`）和 egui 桌面控制台（`metis-gui`），
两者共用同一个 headless 引擎。功能基线是 **agent-purge v0.2.19**，逐项对齐记录在 `docs/PARITY.md`。

**独立实现原则**：本仓库不复制 agent-purge 的代码、脚本、文案、命名或资源。对齐的是**可观察行为**（命令语义、
磁盘格式、注入产物、错误判定），不是实现。任何一处不得直接粘贴上游代码。

**工程实践来源**：项目的 Rust/egui 工程约定（分层强制、错误契约、进程与锁、主题、i18n、测试夹具、质量门禁）
参考了同一作者的另一个 Rust 项目 `craft-anything` 的做法，并将其归纳进 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。
**同样只借鉴实践，不复制代码**；两者的业务域没有重叠。

---

## 工作区地图

```text
crates/
  core        L0  域模型与错误契约：HarnessId、Mode、Channel、PersonaId、Activation、MetisError。无 I/O、无依赖
  paths       L0  路径解析：~/.metis 布局、各 harness 主目录、环境变量覆盖；根可注入，测试不碰真实 HOME
  fs          L0  原子写、所有权凭据（footprint）、备份、控制锁、字节级比较
  persona     L1  人设库：front-matter 解析/序列化、列表、导入（目录 / GitHub）、内置模板
  harness     L1  执行器适配层：探测（detect）、能力表（caps）、启动参数（launch spec）、静态元数据
  channel     L1  四条通道的 apply / revert：install（persist）、shim、slash、project
  process     L2  外部进程：超时、stdin 策略、输出上限与并发排空、进程组终止
  engine      L2  状态与编排：state.json、activation、apply、journal、verification、armor、doctor、update
  ui-egui     L3  egui 桌面界面（唯一允许依赖 egui / eframe / winit / rfd 的 crate）
  testkit        测试夹具：临时 home、假 harness 可执行文件、golden 文件比对
apps/
  metis           CLI
  metis-gui       桌面应用（eframe 壳）
xtask/            cargo xtask：layers | fmt | clippy | test | coverage | secrets | pending | ci | version
```

分层由 `cargo xtask layers` 强制。新增 crate 必须在 `xtask/src/main.rs` 注册。

---

## 铁律（Golden rules）

1. **永不崩溃。** 我们处理的是别人机器上的真实配置文件和真实进程。非测试代码里禁止 `unwrap` / `expect` /
   `panic!` / `todo!` / `unimplemented!` / `unreachable!`，禁止对输入派生的下标直接索引；每个生产 crate 根
   挂 deny 属性。用受检算术、有限递归、有上限的分配。**每个崩溃修复都必须带回归测试。**
2. **一切皆命令。** 用户可见的行为 = 一个命令（`Command` trait 实现，位于 `engine/src/commands/<area>.rs`），
   有 id、标题、参数结构体（schemars）、效果声明、幂等性与 `run`。CLI、GUI 都按 id 分发；GUI 不得绕过引擎
   自己写业务逻辑。
3. **变更必须可逆且可追溯。** 每次写盘前先记录所有权凭据（原始字节 / 哈希 / 我们写入的块标记）；回滚只回滚
   自己写过的字节。**失败不得抹掉恢复材料。** 外部修改一律保留，冲突时明确报错，不静默覆盖。
   远端安装的目录（`skills/<slug>/`）以清单 `.metis-hub.json` 为**唯一**归属凭据；没有清单的目录一律不覆盖、不删除。
4. **区分"写入成功"与"运行时生效"。** 报告里两个字段分开：`written`（磁盘）与 `runtimeConfirmed`（本机实测）。
   任何 UI 文案、日志、测试都不得把前者说成后者。
5. **能力诚实。** 一条通道不能用，就在 UI 与 CLI 里说清**为什么不能用**，不给点了没有反应的开关。
   真相只有一处：`docs/HARNESS-MATRIX.md` 与 `harness` crate 的能力表。
6. **Headless 优先。** 任何能力都必须能不开窗口完成：engine → command → CLI。GUI 只是薄客户端。
   编排、持久化、进程控制永远不写在 `ui-egui` 或 `apps/*` 里。
7. **UI 是数据，不是逻辑。** UI 状态是 serde 数据；颜色、圆角、间距来自主题 token；不在 UI 里做判定，
   不把业务规则编码进字符串匹配。
8. **不联网、不泄密。** 仓库是公开的：禁止 API Key、token、私钥、`.env`、真实个人路径进入代码、测试、日志、
   截图或提交信息，只用明显的假值（`sk-test`、`C:\Users\example`）。`cargo xtask secrets` 检查将要提交的内容。
   默认不监听端口、不发送遥测。远端 Hub 只读公开数据；导入只写本地，不自动 `apply`；
   `verified=false` 的条目必须二次确认。
9. **只用安全 Rust。** 全仓库 `unsafe_code = "forbid"`，无例外。依赖必须能在安全代码中使用（`deny.toml` 把关）。
10. **不乱锁、不锁死。** 只用 `metis-fs` 提供的控制锁与平台同步原语，不用裸 `std::sync::Mutex` 做跨进程/跨线程
    契约。长时间持有的锁必须具名（`guard.label(id)`），等待方不会误判死锁。
11. **外部程序只有一个入口。** 任何外部命令（harness、node、git、浏览器、更新下载）都必须经由
    `metis-process`，不得直接 `std::process::Command`：
    - stdin 关闭或接管道，绝不继承（否则对面停在交互提示上）；
    - 每次执行都有截止时间（墙钟 + 空闲），输出有上限并在独立线程排空；
    - 超时、取消、drop 都要终止子进程**及其进程组**；
    - 程序缺失、非零退出、输出异常、超时都变成带**补救建议**的 `MetisError`，不是 panic，也不是挂死；
    - 任何慢操作都不得跑在 UI 线程上。

---

## 工作规则（对每个改动强制生效）

### 角色与归属

- `docs/ROADMAP.md` 是唯一的计划来源。每个条目有 id（`M<里程碑>.<序号>`）、**负责人**、状态和未完成项（`- [ ]` 行）。
- 负责人只能是角色：**maintainers**（项目维护者；代理可以领取其中未认领且非 **decision** 的项）或 **open**（任何人）。
  标了 **decision** 的条目必须等人类维护者拍板。
- **先认领再动手**：在未完成项后写上 `— claimed: <名字>, branch <分支>`，并尽早推送分支或开 draft PR。
- 按 `ROADMAP.md` 的依赖顺序工作；依赖未完成的条目不开始。

### 动手之前

- 只从 `ROADMAP.md` 取活。不碰已认领、`blocked`、等待 **decision** 的条目。
- 读你要改的代码，对照 `docs/STATUS.md` 与 `docs/PARITY.md` 找差距。发现新的缺口或依赖，**加进路线图**，
  不要顺手扩大本次改动。
- 涉及某个 harness 的行为，先看 `docs/HARNESS-MATRIX.md` 确认它到底支持什么。

### Git 流程

- 永不直接提交 `main`。从最新的 `main` 开分支，前缀：`feature/`、`bugfix/`、`hotfix/`、`refactor/`、`docs/`、
  `test/`、`chore/`，后面接路线图 id（例：`feature/m2.3-shim-channel`）。
- 一个 PR 只做一条路线图条目。不要顺手重构、不要顺带格式化。
- 提交信息用中文，祈使句，说清"改了什么、为什么"。PR 用 squash 合并，PR 标题即提交信息。
- 每次提交/推送前先 `git branch --show-current`，按名字推送。
- 并行代理各自用独立 git worktree 与独立 `CARGO_TARGET_DIR=target/agent-<name>`。

### 架构规则

- **扩展走接缝。** 新增 harness = 实现 `Harness` trait 并在一处注册；新增通道 = 实现 `Channel` trait；不要往
  封闭枚举上再加 `match` 分支。
- **数据优先于代码。** 能力表、路径表、文案表、模板都是带类型的数据表，不是 `if` 链，也不是散落的字符串。
- **结构化结果。** 判定依据是类型化字段，不是英文/中文文本匹配。
- **持久化文件一律带版本。** 任何落盘结构都有 `v` 字段、schema 与迁移路径（见 `docs/DATA-FORMAT.md`）。
- 把代码放在能拥有它的最低层（`cargo xtask layers` 会拒绝越层依赖）。

### 完成定义（Definition of Done）

- "完成前检查"一节里的命令全部通过（针对你改动的 crate）。
- 新行为有测试；崩溃修复有回归测试；对齐基线里的一条行为，必须在 `docs/PARITY.md` 里把状态改成"已对齐"并附证据。
- 同一个 PR 里更新文档：勾选路线图条目、更新 `STATUS.md`、改了 harness 行为就更新 `HARNESS-MATRIX.md`。
- 报告你**验证了什么**、**没验证什么**。没真跑过的事不要说跑过了。
- 犯了一个值得记住的错误 → 同一个 PR 里写进 `docs/PITFALLS.md`。
- 发现有活没干完 → 同一个 PR 里在对应路线图条目下加一条 `- [ ]`。

---

## 自我迭代协议（本仓库的强制机制）

Metis 要求**每完成一个任务就回写一次仓库自身**，让下一个人（或下一个代理）不必重新踩坑。这不是可选建议。

每次任务结束前，逐条确认：

1. **回写路线图**：本条目的状态、未完成项、依赖变化已经在 `docs/ROADMAP.md` 里（`cargo xtask pending` 能列出全部未完成项）。
2. **回写状态**：`docs/STATUS.md` 里"现在真的能用什么"必须与代码一致。不允许"计划已写、状态没动"。
3. **回写坑**：本次踩到的坑（git、测试、构建、egui、跨平台、上游行为）写成 `docs/PITFALLS.md` 里的一条规则，
   格式：`现象 → 原因 → 规则`。
4. **回写本地开发日志**：在 `log/devlog.md` 追加一行流水（做了什么、数字、遗留）。`log/` 不进版本库；
   需要别人知道的写进 PR、`STATUS.md` 或 `ROADMAP.md`。
5. **回写对齐基线**：功能对齐状态只在 `docs/PARITY.md` 一处维护；发现上游行为与文档不符时，先改文档再改代码。
6. **回写 AGENTS.md 自身**：出现下列任一情况，必须在同一个 PR 里修改本文件，而不是只在别处记一笔——
   - 一条铁律被证明不完整、有反例，或需要新的例外；
   - 出现了新的、会反复出现的架构不变式（新的分层规则、新的"唯一真相来源"）；
   - 完成前检查清单需要增删命令；
   - 有规则长期无人遵守（说明规则写得不清楚，改规则，别加更多检查）。
7. **体检（每 10 个合并的 PR 或每个里程碑收尾时做一次）**：删掉已经过时的规则与文档段落；
   合并重复条目；确认每条铁律都能指出对应的自动化检查或测试；确认 `ROADMAP.md` 里没有僵尸条目。
   体检结果写进 `log/devlog.md`，必要时提交一个 `docs/` 分支的 PR。

**反模式（明确禁止）**：只在聊天里解释而不落文档；把知识留在个人分支；为了让检查通过而放宽检查；
把"计划"写成"已完成"。

---

## 单测全覆盖契约

**目标：`crates/*` 与 `apps/*` 的可执行代码，行覆盖率 ≥ 90%（`cargo xtask coverage` 强制）、分支覆盖率 ≥ 80%（`cargo llvm-cov` 报告里人工检查 —— 该工具没有分支门槛参数）**（`cargo xtask coverage` 统计，
`ui-egui` 的纯绘制代码单独统计但不得低于 70%）。覆盖率不是目的，**未覆盖的分支必须在 PR 里说明原因**。

必测清单（每个 crate 的"不可协商"测试面）：

- `core`：模式/通道/harness id 的解析与序列化、非法值拒绝、`Activation` 的全部分支（全局暂停、单家 off、
  `disabledHarnesses`、profile off、未武装、显式启动）。
- `paths`：Windows / macOS / Linux 三套路径解析；注入根时不得读到真实 `HOME`；环境变量覆盖优先级。
- `fs`：原子写在中途失败后原文件完好；所有权凭据回滚**逐字节**还原；外部修改被检测并拒绝覆盖；
  并发锁的串行化与超时。
- `persona`：front-matter 解析（缺字段、旧键 `desc` 兼容、CRLF、UTF-8 BOM、超大正文、只有正文无头）；
  导入去重与非法文件名；`active` 保留标识冲突。
- `harness`：每家 harness 的探测（在 PATH / 不在 PATH / 只有桌面版）与能力表输出；**能力表的每个 `why` 文案
  都要有一条测试**，因为它就是 UI 上给用户看的解释。
- `channel`：四条通道各自的 apply → 幂等 apply → revert 三步，断言磁盘**逐字节**回到初始状态；
  写入失败时恢复材料仍在。
- `process`：超时终止、输出上限截断、stdin 不继承、非零退出映射到 `MetisError`、子进程与进程组被清理
  （用 `testkit` 提供的假程序，不依赖真实 harness）。
- `engine`：`state.json` 的迁移链、`apply` 的全部分支与结果报告、失败重试（`onlyFailed`）、
  journal 与 epoch 语义、verification 报告的读写。
- `ui-egui`：用 `egui_kittest` 做无头界面测试（关键屏幕渲染 + 交互），业务逻辑不上移到 UI，所以 UI 测试只覆盖
  "渲染与交互"，判定逻辑仍由 engine 测试覆盖。
- CLI：`apps/metis` 的每个子命令至少一条端到端测试（`assert_cmd` 风格），断言 stdout/stderr 与退出码。

测试纪律：

- 测试**不得触碰真实 `$HOME`**：一律经由 `testkit::temp_home()`；CI 上跑一次"真实 HOME 只读"断言。
- 测试**不得调用真实 harness**：用 `testkit` 的假可执行文件与假 provider。需要真机验证的用 `#[ignore]`
  并在 `docs/TESTING.md` 登记。
- 不写只断言 `is_ok()` 的测试、不写互相依赖的测试、不写依赖执行顺序或墙钟的测试（用注入时钟）。
- 每个修复的 bug 都先写一个失败的测试再修。

---

## 功能对齐契约

- `docs/PARITY.md` 是**唯一**的对齐基线：agent-purge 的每条命令、每个开关、每条通道、每个状态字段，
  对应 Metis 的哪一项、当前状态（未做 / 已对齐 / 有意不同 / 不需要）。
- 有意不同的必须写明理由（例如：Web 控制台 → egui 窗口；`state.json` 位置 `~/.metis/` 且非 `~/.agent-purge/`）。
- **对齐不是"看起来像"**：磁盘格式、托管块标记、注入产物、错误判定都要有测试钉住。
- 上游（agent-purge）发布新版本时，先更新 `PARITY.md` 与 `HARNESS-MATRIX.md`，再改代码。

---

## 完成前检查

```sh
cargo fmt --all
cargo test -p <你改动的 crate>
cargo clippy -p <你改动的 crate> --all-targets -- -D warnings
cargo xtask layers      # 分层没有被破坏
cargo xtask pending     # 路线图中的未完成项
cargo xtask secrets     # 将要提交的内容里没有密钥和本机路径
cargo xtask remediation # 每个面向用户的错误都有补救建议
cargo xtask i18n        # 两张文案表键一致、无重复
cargo xtask coverage    # 覆盖率门槛（改动涉及 crates/ 时）
cargo xtask snapshot    # 界面快照与基线一致（改动涉及 ui-egui 时；无 GPU 用 METIS_SKIP_SNAPSHOT=1）
cargo xtask ci          # 本地干跑一遍 CI 的关键作业
```

然后按"自我迭代协议"回写文档。

### 落地时已经踩过的坑（先读，能省一天）

完整清单在 [`docs/PITFALLS.md`](docs/PITFALLS.md) 的 G 节（每条都是真撞上并修掉的，附回归测试）。
最容易再犯的几条：

- **`new()` 与 `Default::default()` 互相调用**会把栈打穿，症状是"测试集体挂起"而不是失败。
- **含 `Vec` 的表不能放 `static`**，必须 `LazyLock` + `fn all() -> &'static [T]`。
- **UI 测试不得引入需要 GPU 的测试框架**：没有显示设备时它不是失败，是永久挂起。
  无头渲染直接 `egui::Context::run_ui(...)`，跑完 `output.textures_delta.clear()`。
- **clippy 的 disallowed 规则需要显式例外**：在确实是那个唯一入口的文件里写
  `#![allow(clippy::disallowed_methods)]` 并附一行理由。
- **"暂停 / 关闭通道"必须同时改状态和撤磁盘**。只改 `state.json` 会留下撤不掉的残留块。
- **旗标解析失败必须报错**。`filter_map(parse)` + "空就取默认值" 会把用户的拼写错误变成另一种命令。
- **需要逐字节还原的写入必须存原字节**，重新序列化不算还原。

---

## 桌面 GUI 测试：用 `agent-desktop`

本机已安装 **`agent-desktop`**（Windows 平台，accessibility 权限已授予）。
它通过 **OS 无障碍树**读写界面，不需要截图猜坐标 —— 这是验证 GUI 的**首选**手段。

**首次使用前必须读技能文档**（就是这三条命令，别跳过）：

```sh
agent-desktop status                 # 平台、权限、可用 surface
agent-desktop skills get desktop     # 通用协议：观察-动作-验证循环、ref 规则、错误码
agent-desktop skills get platform    # 本平台差异（Windows 的坑都在这）
```

### 什么时候用它

| 场景 | 用它 |
|---|---|
| 验证界面**真的**能点、能填、能切换 | ✅ 首选 |
| 验证控件**存在**且有正确的角色/名字 | ✅ `snapshot -i` |
| 验证视觉（布局、颜色、留白） | ❌ 用 `PrintWindow` 截图（见下） |
| 无头渲染测试（`cargo test`） | ❌ 不联网、不起窗口，更快 |

**两者互补**：无障碍树证明"控件在且可操作"，截图证明"看起来对"。
缺任何一个都可能漏 bug —— 本项目已经各踩过一次（截图全白骗过一次，快捷键被焦点守卫挡死骗过一次）。

### 核心用法（PowerShell 注意引号）

```powershell
# PowerShell 会把裸 @token 当成 splatting，ref 必须加引号
agent-desktop snapshot --skeleton --app "metis-gui" -i --compact
agent-desktop click '@s8f3k2p9:e12'
agent-desktop snapshot --root '@s8f3k2p9:e3' -i --compact   # 验证：重读同一区域
agent-desktop find --role button --name "应用变更"
```

- **ref 只在一个快照内有效**；界面一变就要重新快照。
- 动作后**用 `get`/`is`/新快照验证**，不要凭"命令成功了"下结论。
- `disposition.retry` 只有 `not_delivered` 才是安全的；其它情况可能已经落地，
  重复点击会点两次（`toggle` 甚至会撤销自己）。
- 同一窗口里有**同名控件**（比如多行都有"删除"）时 ref 会 `AMBIGUOUS_TARGET`
  —— **首选**把行标识写进 accessible name（`详情：<名称>`），实在不行才用
  `--headed mouse-click --xy` 作兜底。

### 什么时候还需要截图

`agent-desktop` 读的是**语义树**，读不出"这张图好看吗"。视觉验收仍然用：

```powershell
# PW_RENDERFULLCONTENT：不走桌面合成，虚拟显示适配器下也能抓到
[WinCap]::PrintWindow($hwnd, $hdc, 2)
```

**不要用 `CopyFromScreen`** —— 这台机器上它抓到的是全白
（`docs/PITFALLS.md` G27）。判断"界面是不是空白"看截图字节数：
全白约 5 KB，有内容 150 KB 以上。

### 三套 GUI 测试的分工（都要保留）

| 脚本 | 层 | 证明什么 | 需要 GPU | 需要 agent-desktop |
|---|---|---|---|---|
| `cargo test -p metis-ui-egui` | 逻辑 | 组件/屏幕渲染出东西、状态派生正确 | ❌ | ❌ |
| `cargo xtask snapshot` | 视觉 | 像素与基线一致（改 2px 就红） | ✅ | ❌ |
| **`scripts/ui-test.ps1`** | **交互** | **控件真的能点、点了真的会变、键盘真的生效** | ✅ | ✅ |

第三层是前两层覆盖不到的：渲染测试里的"点击"是合成事件，快照测试根本不交互。
`scripts/ui-test.ps1` 驱动**真实窗口**，用无障碍树点按钮、按快捷键，每一步回读状态验证。

```powershell
pwsh scripts/ui-test.ps1              # 12 项交互用例
pwsh scripts/ui-test.ps1 -KeepOpen    # 失败时留窗口人工看
```

**验证过它真的能检出 bug**：故意把 `Esc` 的修复注释掉，脚本立刻报
"Esc 关不掉面板（用户报过的 bug 复发了）"；恢复后自动变绿。

### 手绘控件必须自己登记无障碍语义

`ui.allocate_exact_size()` + `ui.painter().text()` 画出来的控件**不会自动出现**在无障碍树里，
`ui.painter()` 的文字也不构成语义节点。**必须显式登记**：

```rust
response.ctx.accesskit_node_builder(response.id, |node| {
    node.set_role(egui::accesskit::Role::Button);
    node.set_label(label);
    node.set_description(hint);
    node.set_selected(selected);
    node.add_action(egui::accesskit::Action::Click);
});
```

不登记的实际后果（用 `agent-desktop` 查过）：侧栏四个导航项里**只有一个**能被读到，
而且被当成 `statictext` 而不是按钮 —— 对读屏用户来说**整个侧栏不存在**。
这是 `M6.11` 里"accesskit 语义"那条的真实含义，比"加个 feature 开关"要具体得多。

## 并行代理

- 各自的 `CARGO_TARGET_DIR=target/agent-<name>`，各自的文件。
- 共享文件（`engine/src/commands/mod.rs`、各 `lib.rs`、工作区 `Cargo.toml`）只做外科手术式小改；
  改之前重新读一遍。
- 清单文件原子写入（先在 `crates/` 之外建临时文件，再 `mv`）。
- 别人的改动把构建弄坏了：等待并重试，不要顺手改别人的代码。
