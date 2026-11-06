# 开发指南：怎么搭一个 Metis 这样的 egui 应用

> 相关：[架构](ARCHITECTURE.md) · [界面规格](UI.md) · [测试](TESTING.md) · [坑](PITFALLS.md) · [扩展指南](EXTENDING.md)

> 本文件的工程实践来自对参考项目 `craft-anything` 的只读研究（详见 `.research/craft-anything-conventions.md`
> 与 `.research/craft-anything-ui-egui-patterns.md`）。**只借鉴工程实践，不复制任何代码**（见 `AGENTS.md` 开头）。
> 遇到与本文件冲突的实测结论，**以实测为准，并在同一个 PR 里更新本文件**（自我迭代协议第 6 条）。

## 1. 版本与依赖选型

选型原则：**纯 Rust、能用安全代码调用、离线可构建、单一可执行文件**。

| 依赖 | 版本 | 用途 | 备注 |
|---|---|---|---|
| `eframe` | `0.36`（`default-features = false`） | 桌面壳 | 应用侧启用 `["default_fonts", "wgpu", "accesskit", "wayland", "x11", "persistence"]` |
| `egui` | `0.36` | 界面 | |
| `egui_extras` | `0.36`（`default-features = false`） | 表格、SVG | `ui-egui` 侧启用 `["svg"]` |
| `egui_kittest` | `0.36` | 无头界面测试 | dev-dependency，`features = ["wgpu", "snapshot", "eframe"]` |
| `rfd` | `0.17` | 原生文件对话框、错误框 | 只在 worker 线程或窗口起不来时调用 |
| `serde` / `serde_json` | `1` | 一切持久化 | `features = ["derive", "rc"]` |
| `schemars` | `1` | 命令参数 schema | 供 GUI 自动表单与文档生成 |
| `toml` | `0.9` | 改宿主的 `config.toml` | 需要保留格式时要小心（见 `PITFALLS.md`） |
| `process-wrap` | `10` | 外部进程 | `features = ["std", "process-group", "job-object", "kill-on-drop", "creation-flags"]` |
| `ureq` | `3` | 更新检查 | 纯 Rust TLS，避免原生依赖 |
| `dirs` | `6` | 系统目录 | 只用于默认值；所有路径必须可注入 |
| `thiserror` | `2` | 错误类型 | |
| `log` + 自实现 file logger | `0.4` | 日志 | 不引入异步运行时的日志栈 |

**`links` feature 必须显式打开**：workspace 关掉了 eframe 的默认 feature，不开 `links` 时 `ctx.open_url` 是
静默 no-op（表现为"点链接没反应"）。

工作区设置：

```toml
[workspace]
resolver = "3"
members = ["crates/*", "apps/*", "xtask"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
rust-version = "1.97"
repository = "https://github.com/<org>/metis"

[workspace.lints.rust]
unsafe_code = "forbid"

[profile.dev.package."*"]
opt-level = 2      # 依赖用 O2，自己用 O1：egui 在 debug 下太慢，但自己的代码要能快速增量

[profile.dev]
opt-level = 1

[profile.release]
lto = "thin"
codegen-units = 1
```

`rustfmt.toml`：`max_width = 160`、`use_small_heuristics = "Max"`（宽行 + 紧凑小项；与参考项目同风格）。
`clippy.toml`：测试里放行 `unwrap`/`expect`/`panic`/索引；**用 `disallowed-types` 禁 `std::sync::Mutex`**。
`deny.toml`：`yanked = "deny"`、`wildcards = "deny"`、许可白名单（MIT/Apache/BSD/ISC/Zlib/…），
MPL 之类要逐 crate 豁免；绝不链接 copyleft。

## 2. 分层由工具强制，不靠自觉

`xtask/src/main.rs` 里维护一张 `LAYERS` 表（每个 crate 一行）。`cargo xtask layers` 至少检查：

1. **未注册的 crate** → 失败（新增 crate 必须进表）。
2. **依赖只能向下**：高层 crate 不能依赖低层没有的层（`metis-core` 依赖任何内部 crate 即失败）。
3. **UI 独占依赖**：`egui` / `eframe` / `winit` / `rfd` / `egui_extras` / `egui_kittest` 只允许出现在
   `metis-ui-egui` 与 `apps/metis-gui`。
4. **外部进程唯一入口**：`std::process::Command` 只允许出现在 `metis-process`、`xtask` 与测试代码里。
5. **UI 无副作用**：`crates/ui-egui/src/**` 的非测试代码里不得出现写文件、起进程的调用。
6. **UI 动作有 headless 对应物**：`impl MetisApp` 里每个 `&mut self` 动作方法，要么调用一个已注册命令，
   要么在 `ACTIONS` 表里标注 `UiOnly`/`Pending` 并写理由（防止"界面能点、CLI 不能做"的功能漂移）。

这些检查读源码（`syn` 解析或文本扫描），**不靠人记**。这是"自我迭代"能落地的前提：规则要么能被机器检查，
要么写进 `PITFALLS.md`。

## 3. crate 骨架

每个生产 crate 的 `src/lib.rs` **第一行**：

```rust
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
```

每个 manifest 的固定头部：

```toml
[package]
name = "metis-<x>"
description = "…"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[lints]
workspace = true

[dependencies]
metis-core = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
log = { workspace = true }

[dev-dependencies]
metis-testkit = { workspace = true }
tempfile = "3"
```

新增 crate 的四步：建目录 → `[workspace.dependencies]` 加 `metis-<x> = { path = "crates/<x>" }` →
`LAYERS` 加一行 → 建 `Cargo.toml` 与 `src/lib.rs`。漏任何一步，`cargo xtask layers` 会拦住。

## 4. eframe 应用骨架（`apps/metis-gui/src/main.rs`）

按下面的顺序写，每一步都有它存在的理由：

```rust
//! Metis 桌面控制台。
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]  // 发布版不弹控制台窗口
```

1. **`--version` / `-V` 早退**：打印 `metis-gui {CARGO_PKG_VERSION}`，直接 `return Ok(())`，不开窗口。
   （打包脚本与用户都靠它核对版本。）
2. **文件日志**：写到数据目录下的 `logs/gui.log`，上限 512 KiB，超过就重建；装 `std::panic::set_hook`
   把 panic 也写进去。**日志是崩溃现场唯一的证据**。
3. **启动守卫**：数据目录里放一个 `starting` 标记文件（内容 = pid）。`begin()` 写、窗口真正显示后
   `opened()` 删。下一次启动看到它还活着 → 说明上次启动没到窗口（典型原因是显卡驱动原生崩溃，Rust 侧
   捕获不到）→ 本次退化到软件渲染重试一次，并把这件事写进日志与界面提示。
4. **引擎**：`Engine::open(paths)`（读 `~/.metis`）。引擎构造不得做慢操作。
5. **Viewport**：
   ```rust
   let viewport = egui::ViewportBuilder::default()
       .with_title("Metis")
       .with_app_id("dev.metis.console")
       .with_inner_size([1280.0, 860.0])
       .with_min_inner_size([900.0, 620.0])
       .with_resizable(true)
       .with_drag_and_drop(true);
   ```
6. **窗口图标**：非 macOS 用 `eframe::icon_data::from_png_bytes(metis_ui_egui::APP_ICON_PNG)`；
   macOS 传空图标（用 bundle 里那张）。
7. **GPU 选择**（这一条是踩坑踩出来的，不要省）：
   - **Windows 默认只启用 DX12**（含 WARP）。启用 GL 会加载 `ig9icd64.dll`、启用 Vulkan 会加载
     `igvk64.dll`，两者都在 Intel 机器上原生崩过 —— 而原生崩溃没有栈、没有日志，用户只会说"打不开"。
   - shader compiler 固定 `Fxc`：wgpu 的 `Auto` 会从 `PATH` 上捡到没有 `dxil.dll` 的 `dxcompiler.dll`。
   - **Linux 选"驱动屏幕的那块 GPU"**：默认实现倾向独显，而笔记本面板常接在核显上，结果是开窗十几秒
     且整个桌面卡顿。
   - 允许 `METIS_RENDERER=software` 走软件渲染兜底。
8. **`catch_unwind` 包住 `eframe::run_native`**：无法配置 surface 时 eframe 会 panic。
   规则：窗口**显示之前**的 panic → 转成 `eframe::Error`；显示**之后**的 panic → `resume_unwind` 原样抛出
   （显示之后的 panic 是应用逻辑 bug，不该被吞）。
9. **起不来时说人话**：用 `rfd::MessageDialog` 弹一个错误框，正文包含具体建议（例如
   `METIS_RENDERER=software`）与**日志文件路径**。一个什么都不显示的程序是最糟的失败形态。
10. **`App::save`**：持久化 UI 偏好（面板宽度、折叠状态、上次屏幕、语言）。窗口几何交给 eframe 的
    `persistence` feature。

> **帧入口的名字**：eframe `0.36` 的应用 trait 用 `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`
> 作为每帧入口（不是旧版的 `update(&mut self, ctx, frame)`）。按你实际使用的 eframe 版本的 trait 定义来写，
> 不要照抄记忆里的旧签名 —— 这一点在 `PITFALLS.md` 里有一条。

## 5. `metis-ui-egui` 的内部结构

扁平单层文件 + 一个 `i18n/` 目录。**不要**再分子目录（参考项目证明扁平结构在 20+ 文件时仍然好导航，
而深层嵌套会让"这个组件放哪"变成每日争论）。

| 文件 | 角色 |
|---|---|
| `lib.rs` | `MetisApp`、`impl eframe::App`、每帧的 `tick()`/`show()`、`guard`/`guard_ui`、`APP_ICON_*`、`ACTIONS` 表 |
| `state.rs` | UI 状态：`Screen`、`Draft`、`Modal`、`Prefs`（纯 serde 数据） |
| `jobs.rs` | `Jobs<T>` 后台任务：线程 + mpsc + 每帧一次 `poll()` + 取消令牌 + 进度 |
| `theme.rs` | 主题 token、字体装载、字号尺度、`Tokens::get(ctx)` |
| `widgets.rs` | 共用控件：主按钮、卡片、分段控件、开关、`named()`（无障碍名）、空态、加载态 |
| `icons.rs` + `icon_data.rs` | SVG 图标光栅化与 tint；`include_bytes!` 清单 |
| `screens/`（或扁平 `console.rs`/`library.rs`/`diagnostics.rs`/`about.rs`） | 每个屏幕一个文件 |
| `model.rs` | **纯函数**的列表筛选/排序/分组；不依赖 egui 类型，因此可直接单测 |
| `i18n.rs` + `i18n/*.tsv` | 文案表与查表 |
| `build.rs` | 可选：生成字体清单（`METIS_FONTS_DIR`） |
| `examples/snapshot.rs` | 离屏渲染各屏 PNG（无窗口，CI 可用） |
| `tests/` | `egui_kittest` 无头界面测试 + 快照 |

**为什么 `model.rs` 要独立**：界面里有大量"筛掉没装的""按名字排序""哪些通道该显示"的逻辑。
把它放进不依赖 egui 的纯函数模块，就能用普通单元测试覆盖，而不是被"必须开窗口才能测"绑架。

### 5.1 界面模式清单（照这些形状写，别自己发明）

**屏幕分派只有一处**：`enum Screen { Console, Library, Diagnostics, About }`，`match` 只出现在
`MetisApp::show()` 里。每个屏幕是一个 `pub fn show(app: &mut MetisApp, ui: &mut egui::Ui)` 自由函数，
它需要的控制器方法写在同一文件的 `impl MetisApp` 块里。

**异步填充用一个穷尽的状态机**，不要用一堆布尔量：

```rust
pub enum Fill<T> {
    Idle,                     // 还没开始
    Loading,                  // 正在取
    Unavailable(String),      // 本机没有（例如没装这个 harness）
    Blocked(String, Option<String>), // 有但不允许（例如通道不可用），第二个是补救建议
    Error(String, Option<String>),
    Ready(T),
}
```

界面上的"加载中 / 空 / 失败 / 有数据"四种形态都由它派生，不需要额外的 `is_loading`、`has_error`。

**列表一律虚拟化手绘**：用 `egui::ScrollArea::show_rows`，不要用 `egui_extras::TableBuilder`
（参考项目实测下来手绘行更好控：行高、可达性、点击收集）。两条硬规则：

- 循环体里用 `let Some(row) = rows.get(i) else { continue };`，**不在循环里改状态**；
  把点击/展开收集到循环外的变量，循环结束后统一应用。
- 行高要自己算上 `item_spacing`，否则长列表会慢慢错位。

**自己实现 Spinner**：`egui::Spinner` 每帧都请求立即重绘，一个跑一分钟的任务能把 CPU 打满。
写一个只在需要时重绘的进度指示，并加一条源码扫描测试禁止别处用 `egui::Spinner`。

**toast 规则**：确认类 4 秒自动消失；错误**不自动消失**（必须用户关掉）；最多同时 4 条，
超出先丢确认类；相同文案的重复提示直接合并。

**表单校验的三种形状**（按场景选，不要混）：

1. 纯函数校验：`fn url_problem(s: &str) -> Option<&'static str>`，渲染时直接显示。
2. 提交时校验：返回带字段名的错误，定位到具体输入框。
3. "还没填完"：用 `Option`，按钮变灰并用 hover 文案说明**缺什么**（不能只变灰不说原因）。

**对话框一律在 worker 线程弹**：`rfd::AsyncFileDialog` + 阻塞等待，结果通过任务结果回传。
绝不在帧里弹模态文件框。

**没有系统托盘、没有原生通知**：需要提醒用户时用 `ViewportCommand::RequestUserAttention`（闪任务栏）
或应用内 toast。不要为了"看起来专业"引入托盘，它会带来一堆平台差异。

**自绘标题栏的代价**：`with_decorations(false)` 之后，窗口拖动、边缘缩放、最小/最大/关闭按钮、
1px 边框都得自己实现。Metis 默认**用系统装饰**（`with_decorations(true)`），少一类平台 bug；
如果以后要自绘，先读 `PITFALLS.md` 里对应的条目。

**异步结果的过期保护**：每次异步请求带一个自增请求号，回来时对不上就丢弃。
否则"快速切换两次人设，先发的慢请求后到"会把界面覆盖成旧数据。

## 6. UI 永不阻塞：照抄这一套

参考项目最强的工程约束就是这条，具体机制：

```rust
/// 后台任务表：一帧只轮询一次，绝不在帧内等待。
pub struct Jobs<T> { /* 线程句柄 + mpsc::Receiver + HashMap<JobId, JobInfo> */ }
impl<T: Send + 'static> Jobs<T> {
    pub fn spawn(&mut self, label: impl Into<String>, work: impl FnOnce(JobCtx) -> Result<T, MetisError> + Send + 'static) -> JobId;
    pub fn poll(&mut self) -> Vec<Finished<T>>;   // 每帧调一次
    pub fn cancel(&self, id: JobId);
    pub fn is_running(&self, id: JobId) -> bool;
}
```

规则：

- **慢操作一律进 `Jobs`**：探测 15 家 harness、apply、verify、update、导入。
- **取快照不阻塞**：`Engine::try_snapshot()` 返回 `Option<Snapshot>`，拿不到就沿用上一帧的视图，
  绝不在帧内等锁。
- **重绘节奏**：有任务在跑时 `ctx.request_repaint_after(Duration::from_millis(100))`，空闲时 1 秒。
- **不要用每帧都 `request_repaint` 的 spinner**：那会让应用在空闲时仍然满帧跑，笔记本风扇会转。
  用"按需重绘 + 静态进度条"。
- **每个易错动作包一层 `guard`**：`catch_unwind` 之后把 panic 转成界面上的错误条，而不是让整个应用挂掉。
- **取消**：`CancelToken` 传进 `metis-process`，超时/取消/drop 三条路径都要能杀掉进程组。

## 7. 主题

- 所有颜色/圆角/间距是 token，存在 egui 的 temp data 里，组件一律 `Tokens::get(ctx)` 取。
  **禁止**在组件里写 `Color32::from_rgb(...)` 字面量。
- 字号尺度集中定义（`TITLE / SECTION / CARD / LEAD / BODY / SMALL / CAPTION / TINY`），
  并**写一个扫描测试**：除 `theme.rs` 外，任何小于 12pt 的字号字面量直接失败（防止有人偷偷塞 9pt 小字）。
- 字体：优先系统 CJK 字体（`Microsoft YaHei UI` / `PingFang SC` / `Noto Sans CJK SC`），内置一份等宽字体
  用于路径与产物预览。字体文件与许可证一起进仓库（`assets/fonts/` + `ATTRIBUTION.md`）。
  字体加载顺序：用户本地 > CJK > Sans > 其它 > Serif。
- 图标：Lucide（ISC）SVG，`include_bytes!` 进来，运行时把 `currentColor` 替换成主题色、
  `stroke-width` 调细，按屏幕像素光栅化（高 DPI 不模糊）。
- 深色主题：token 结构现在就留出，M1 只交付浅色。

## 8. i18n

- TSV 表：`键 \t 文案`，每种语言一个文件（`zh-Hans.tsv`、`en.tsv`），编译期 `include_str!`。
- 查表函数 `tr(key)`，缺失回退 `zh-Hans`；debug 构建下缺键直接断言失败。
- **引擎产出的用户可见字符串也走同一套键**：能力表的 `why`、错误的 `message`/`remediation` 都是键，
  不是硬编码中文。这样 CLI 与 GUI 文案永远一致，也才可能翻译。
- 覆盖率检查：`cargo xtask i18n-coverage` 扫描代码里用到的键与 TSV 里的键，双向核对。

## 9. 错误与 panic 兜底

```rust
pub struct MetisError { kind: ErrorKind, message: String, remediation: Option<String>, cause: Option<Box<MetisError>> }
```

- 每个 `ErrorKind` 有稳定编号（写进文档），`retryable()` 明确回答"能不能重试"，
  `remediation` 是"照做就能修"的具体动作（命令、路径、开关），**不是**"请检查配置"。
- panic 兜底四层：命令调用、后台任务、每帧动作、应用启动。每一层都把 panic 变成用户能读的错误。
- **不崩**的具体手段：不 `unwrap`、不做输入派生的下标、受检算术（`checked_*`）、有限递归、
  有上限的读取（单个文件 >4 MiB 直接报错）。

## 10. 进程与锁

- 外部程序只有 `metis-process` 一个入口：stdin 默认关闭、双截止时间（墙钟 + 空闲）、
  输出上限（stdout 16 MiB / stderr 1 MiB / 单行 16 MiB，超限只留头尾并标记截断）、
  独立线程排空、Unix 进程组 / Windows Job Object + `CREATE_NO_WINDOW`。
- 命令行有长度上限（Windows 约 32767 字符）：**长内容一律走 stdin 或临时文件**，不塞进命令行参数。
- **一个持久化文件只能有一个写入者**：`state.json` 只由 `metis-engine` 写；
  账本只由 `metis-fs` 写；宿主配置只由 `metis-channel` 写。跨模块"顺手写一下"是事故来源。
- 锁：跨进程用 `~/.metis/.apply.lock`（`O_EXCL` + pid + token + 可重入 + RAII 释放）；
  进程内用可诊断的互斥原语，长任务持锁要具名，等待方不会误判死锁。

## 11. 测试 egui 应用

```rust
// crates/ui-egui/tests/console.rs
let mut harness = egui_kittest::Harness::builder()
    .with_size(egui::Vec2::new(1280.0, 860.0))
    .build_eframe(|cc| MetisApp::for_test(cc));   // 用假引擎，绝不碰真实 HOME
harness.run();
harness.get_by_label("应用变更").click();
harness.run();
```

要点（都是踩出来的）：

- **有后台任务时用 `harness.wait()`，不要用固定帧数 `run_steps(n)`**：固定帧数在慢机器上会偶发失败。
- **影响全进程状态的测试单独放一个 test 程序**（例如改了渲染后端或全局环境变量），否则会污染同进程的其它
  用例。
- **需要真实子进程时**，把自己作为子进程重跑，并给子进程注入隔离的数据目录（`METIS_HOME`），
  不要在同进程里改环境变量。
- 快照测试用 `snapshot` feature；快照变了必须在 PR 里说明原因。
- 界面测试**不测业务判定**：那只证明"画出来了、点得动"。判定由 engine 的单测覆盖。

## 12. 从零搭一个 egui 应用的清单

1. 定名与分层：`metis-*` 前缀，先画层表再建目录。
2. 建工作区：`Cargo.toml`、`rustfmt.toml`、`clippy.toml`、`deny.toml`、`.cargo/config.toml`（`xtask` alias）、
   `.gitattributes`（`* text=auto eol=lf`）、`.gitignore`。
3. 建最小 crate 集合：`core`（域模型 + 错误）、`paths`、`fs`、`engine`、`ui-egui`、`testkit`、
   `apps/metis-gui`、`xtask`。每个 crate 带 deny 头与 `[lints] workspace = true`。
4. 写 `xtask` 的 `LAYERS` 与 `layers` 子命令，**先让它能红**（未注册就失败），再开始写业务。
5. 写 eframe 壳：viewport、图标、日志、启动守卫、GPU 选择、`catch_unwind`、错误框。
6. 写主题 token 与字体，再写第一个屏幕（控制台）。
7. 建 `Jobs<T>` 与命令注册表，把第一个慢操作（探测）接到界面上。
8. 加 `egui_kittest` 冒烟测试与一条快照。
9. 接 CI（三平台 test、fmt、clippy、coverage、layers、secrets）与 `.githooks/pre-commit`。
10. 写文档五件套（`AGENTS.md`、`README.md`、`ROADMAP.md`、`STATUS.md`、`PITFALLS.md`），
    然后立刻按自我迭代协议维护它们。

## 13. 已经踩过的坑（直接当规则用）

**构建与运行**

- 只有被启用的图形后端才会杀死你：Windows 默认只启 DX12。
- shader compiler 固定 `Fxc`：`Auto` 会捡到不完整的 `dxcompiler.dll`。
- Linux 要选"驱动屏幕的那块 GPU"，否则开窗十几秒、桌面卡顿。
- 原生显卡崩溃不留栈：用 `starting` 标记文件在**下次**启动发现问题。
- 命令行有长度上限：长内容走 stdin 或文件。
- Windows 上 `.cmd` 垫片要靠 `PATHEXT` 查找，不能硬编码 `.exe`。
- 杀进程要 `kill_and_wait`：只调 `kill()` 是置位，进程可能还活着。
- 按行截断会切碎结构化输出（JSON-RPC、JSON 行）：截断必须按记录边界。

**测试**

- 用 `wait()` 而不是固定帧数。
- 影响全局状态的用例放独立 test 程序。
- 测试不得读写真实用户目录；用 `TempHome` 并在 CI 上跑一条"真实 HOME 只读"断言。
- 需要真机的用例标 `#[ignore]` 并登记，不要留在默认 CI 里制造偶发失败。

**设计**

- 一个持久化文件只能有一个写入者。
- 判定用字段，展示用键；不要用文本匹配做判定。
- 文本文档里的数字（工具数、版本、文件清单）会漂移：能被机器核对的就写成检查。
  （参考项目就出现了"文档说 2、代码是 3"和四处工具数不一致。）
- 规则要么能被机器检查，要么写进 `PITFALLS.md`；只写在聊天里的规则等于不存在。
