# 路线图

> 相关：[AGENTS.md](../AGENTS.md) · [当前状态](STATUS.md) · [功能对齐](PARITY.md)

> 这是**唯一**的计划来源（见 `AGENTS.md`）。每条有一行标题、负责人（只能是角色）、状态与未完成项。
> 状态：`todo`（未开始）· `doing`（进行中）· `done`（完成）· `blocked`（被外部条件卡住）· `decision`（等维护者拍板）。
> 认领方式：在未完成项后写 `— claimed: <名字>, branch <分支>`，并尽早推送分支。

## 现状

M0–M10 的闭环已落地；当前没有进行中的条目。每项"能用"的证据以 [`STATUS.md`](STATUS.md) 为准。

## 顺序与依赖

| 里程碑 | 依赖 | 能并行的部分 |
|---|---|---|
| M1 地基 | M0 | core / paths / fs 三块彼此独立，可三个代理并行 |
| M2 最小闭环 | M1 | persona 与 state 可并行；apply 依赖两者 |
| M3 四条通道 | M2 | 四条通道彼此独立（各自一个代理） |
| M4 执行器全量 | M2（install 通道就绪） | 15 家 harness 可按"落点类型"分组并行 |
| M5 验证与观测 | M2 | verify / armor / update 彼此独立 |
| M6 egui 控制台 | M2（命令注册表就绪） | 屏幕可并行，但主题与 i18n 必须先完成 |
| M7 打包发布 | M6 | — |
| M8 打磨 | M6 | 各项独立 |

---

## M0 文档与脚手架 — owner: maintainers

先把"我们到底要造什么、怎么算做完"写清楚，再写代码。

- [x] M0.1 文档集：`AGENTS.md`、`README.md`、`docs/ARCHITECTURE.md`、`UI.md`、`DATA-FORMAT.md`、`TESTING.md`
- [x] M0.2 路线图、状态、坑、自我迭代协议
- [x] M0.3 `docs/PARITY.md` 与 `docs/HARNESS-MATRIX.md` 完成到"每条基线行为都有归属"
- [x] M0.3b 拍板 `PARITY.md` §8 的 7 条开放决策（2026-11-06：**不与基线字节兼容**；已由 `channel::blocks::brand_tests` 钉住）
- [x] M0.4 工作区骨架：`Cargo.toml`（workspace、edition 2024、MSRV）、10 个 crate + 2 个 app + `xtask` 的空壳、`rust-toolchain.toml`
- [x] M0.5 `xtask`：`layers` / `fmt` / `clippy` / `test` / `coverage` / `secrets` / `pending` / `ci` / `version`
- [x] M0.6 质量配置：`rustfmt.toml`、`clippy.toml`、`deny.toml`、`.cargo/config.toml`、`.githooks/pre-commit`
- [x] M0.7 CI：`ci.yml`（fmt、clippy、test 三平台、coverage、layers、secrets、docs 一致性、msrv）
- [x] M0.8 `docs/DEVELOPMENT.md`：从零搭 egui 应用的实操清单（工程实践来源见该文件开头的说明）
- [x] M0.9 `log/` 与 `devlog.md` 约定 + `.gitignore`
- [x] M0.10 许可与归属：`LICENSE-MIT`、`LICENSE-APACHE`、字体与图标资产的许可说明（`ATTRIBUTION.md`）

## M1 地基（core / paths / fs） — owner: open

- [x] M1.1 `metis-core`：`HarnessId`、`Mode`、`Channel`、`PersonaId`、`Activation`、`Effect`、`MetisError`
      （依赖 M0.4）
- [x] M1.2 `metis-paths`：`METIS_HOME` 解析、15 家 harness 主目录矩阵、shim 目录、临时 journal 目录；
      根**必须可注入**（依赖 M0.4）
- [x] M1.3 `metis-fs`：原子写、`0600`、备份、逐字节比较、路径规范化（Windows 大小写）（依赖 M1.1）
- [x] M1.4 `metis-fs`：所有权账本 `footprints.json`（幂等、所有者匹配、损坏不销账）（依赖 M1.3）
- [x] M1.5 `metis-fs`：交换与恢复 journal（`DATA-FORMAT.md` §6 的六步，含死锁回收、外部修改检测）（依赖 M1.3）
- [x] M1.6 `metis-fs`：控制锁（`O_EXCL` + pid 存活 + token + 同进程可重入 + RAII guard）（依赖 M1.3）
- [x] M1.7 `metis-testkit`：`TempHome`、`FakeHarness`、`FakeProvider`、`assert_golden`、`TestClock`（依赖 M1.3）
- [x] M1.8 `metis-process`：`process-wrap` 封装、超时/输出上限/stdin 策略/进程组终止（依赖 M1.7）
- [x] M1.9 崩溃安全：apply 中途被强杀后，下次启动能回收 journal 且不留半写文件（依赖 M1.5）

## M2 最小闭环（人设 / 状态 / 激活 / 应用 / CLI） — owner: open

- [x] M2.1 `metis-persona`：front-matter 解析与序列化、列表、`active.md` 物化、内置模板（依赖 M1.3）
- [x] M2.2 人设导入：本地目录 + GitHub `owner/repo`、按内容去重、覆盖开关（依赖 M2.1）
- [x] M2.3 `metis-engine`：`state.json` 读写 + 迁移链（幂等）+ 未知字段保留（依赖 M1.4）
- [x] M2.4 `metis-engine`：`resolve_configured` / `resolve_activation`（九步短路，顺序固定）（依赖 M2.3）
- [x] M2.5 `metis-engine`：命令注册表 `Command` trait + 参数 schema + effects（依赖 M1.1）
- [x] M2.6 `metis-engine`：apply 编排 + `ApplyReport` + 失败重试（`onlyFailed`）（依赖 M2.4、M3.1）
- [x] M2.7 `metis-engine`：`JobRunner`（后台任务、进度、取消、事件流）（依赖 M2.5）
- [x] M2.8 `apps/metis`：CLI 骨架（clap）、命令映射、`--json` 机器可读输出、退出码约定（依赖 M2.5）
- [x] M2.9 CLI 与 GUI 走同一实现的一致性测试（I9 不变式）：`crates/ui-egui/tests/consistency.rs`（9 条）

## M3 四条通道 — owner: open

- [x] M3.1 `metis-channel`：`ChannelImpl` trait + `plan/apply/revert/status` 通用骨架（依赖 M2.5）
- [x] M3.2 `install`：托管块写入/切除（Markdown / TOML），块外一个字节不改（依赖 M3.1）
- [x] M3.3 `install`：结构化字段写入（`settings.json`、`config.toml`）（依赖 M3.2）
- [x] M3.4 `project`：项目级托管块、登记绝对路径、按目录逐格报告、逐字节还原（依赖 M3.2）
- [x] M3.5 `shim`：Windows `.cmd` 与 Unix 脚本包装、PATH 提示、幂等（依赖 M1.3）
- [x] M3.4a `project` 的 CLI 入口：`metis project list|add|remove`（摘除时**先撤盘再清状态**）
- [x] M3.6 `slash`：宿主扩展产物生成（pi / omp / pi-desktop / opencode v1+v2 / dsh）+ 独立 JS 运行时 + node 行为测试 + **golden 测试（5 份，`cargo xtask golden` 重建）**（依赖 M3.1）
- [x] M3.7 撤销族：`pause` / `resume` / `disarm` / `clean` / `uninstall` / `self-uninstall` 全部落地（依赖 M2.6）
- [x] M9.7 GUI 接入 GitHub 导入：人设库屏「从 GitHub 导入」卡片（默认官方源、预览/导入、后台任务、结果计数）。真机验证：点一下导入 5 份
- [x] M3.10 官方人设源：`dulingzhi/metis-personas`（5 份人设 + 索引 + 自动更新 workflow）；客户端改走 **codeload tarball**（1 个请求，不打 API）
- [x] M3.9 CLI 补齐：`use`（无 id 打印）、`sync [--failed]`、`install`、`slash status|on|off`
- [x] M3.8 `metis import-agent-purge`：只读导入、冲突上报、检测基线残留块（依赖 M2.1、M2.3）

## M4 执行器全量对齐（15 家） — owner: open

- [x] M4.1 `metis-harness`：`Harness` trait + 注册表 + 落点表数据结构（依赖 M1.2）
- [x] M4.2 探测 `detect`：PATH、已知安装目录、版本、标记文件（依赖 M4.1）
- [x] M4.3 能力表 `caps`：每条通道的 `ok` 与 `why`，每个 `why` 有测试（依赖 M4.1、M3.1）
- [x] M4.4 启动参数 `launch`：15 家的参数形状 + 默认 dry-run + 参数脱敏（依赖 M4.1）
- [x] M4.5 `oneshot`（`--ask`）：12 家有参数形状，其余明确拒绝并给替代（依赖 M4.4）
- [x] M4.6 插件类：`opencode` v1/v2 产物 + DSH 多 profile 自动展开 + **`cordis.patch.yml` 挂载块**（安装挂上、撤销摘掉，逐字节还原）（依赖 M3.6）
- [x] M4.7 配置类：`pi`、`pi-desktop`、`grok`、`omp`、`hermes`、`codex`、`kilo`、`workbuddy`（依赖 M3.3）
- [x] M4.8 启动参数类：`claude`、`codebuddy`、`grok`、`zcode`（`zcode` 给人工配方）（依赖 M4.4）
- [x] M4.9 `antigravity`：明确标为"不支持注入"并说明原因（依赖 M4.3）
- [x] M4.10 `doctor`：本机探测报告（装了什么、版本、主目录、标记文件、可用通道）

## M5 验证与观测 — owner: open

- [x] M5.1 `metis-process`：进程内假 provider（只绑 `127.0.0.1`，随机端口）（依赖 M1.8）
- [x] M5.2 `verify`：按 harness 抓真实请求、判定命中角色、写 `verification/latest.json`（依赖 M5.1、M4.4）
- [x] M5.3 隐私约束测试：报告里不出现完整人设正文（依赖 M5.2）
- [x] M5.4 `journal/*.jsonl` 追加日志 + 保留策略 + 轮转（依赖 M1.3）
- [x] M5.5 `armor`：`plan` / `preflight` / `record` / `judge` / `adopt` + 矩阵表格输出（依赖 M5.2）。`run` 的编排留给用户跑真实评测后 `record`
- [x] M5.6 `update`：GitHub Release 检查（真版本比较 + 可注入 fetch）（依赖 M1.8）。仍未做：就地替换 exe（Windows 正在运行的 exe 不能覆盖，需 update-on-exit）
- [x] M5.7 `matrix`：能力矩阵的机器可读与人类可读两种输出（`metis harnesses` / `--json`）（依赖 M4.3）

## M6 egui 桌面控制台 — owner: open

- [x] M6.1 `apps/metis-gui`：eframe 壳（viewport、启动守卫、**真机开窗验证**）（依赖 M0.4）
- [x] M6.2 `metis-ui-egui`：主题 token + **系统 CJK 字体加载**（缺了就是满屏方框）（依赖 M6.1）
- [x] M6.3 i18n：TSV 表（`zh-Hans` / `en`，各 25 键）、缺键检测（`cargo xtask i18n`）、`--lang` 切换
- [x] M6.4 通用组件：按钮/卡片/列表/表格/徽标/提示/空态/加载态（依赖 M6.2）
- [x] M6.5 控制台屏幕：状态条、人设与模式、执行器列表与详情、默认启动（依赖 M2.7、M4.3）
- [x] M6.6 人设库屏幕：列表/搜索/编辑器/脏状态/删除确认（依赖 M2.1）
- [x] M6.7a 导入对话框（关于屏的"预览导入/从 agent-purge 导入"按钮已接上命令）。仍未做：更新对话框（依赖 M5.6）
- [x] M6.8 诊断屏幕：最近应用报告、重试失败项、验证结果、诊断信息（依赖 M2.6、M5.2）
- [x] M6.9 关于屏幕：版本、许可、导入 agent-purge 数据、打开数据目录、卸载数据（依赖 M3.8）
- [x] M6.10 无头界面测试：`egui_kittest` 渲染 + 交互 + 快照（依赖 M6.5）
- [x] M6.11 无障碍：键盘可达（`Alt+1..4`/`Ctrl+R`/`Ctrl+Enter`/`Ctrl+K`/`Esc`）、焦点加宽可见、减少动效（系统环境变量 + 应用内开关）。**未做**：真正的 accesskit 语义标注（egui 的集成需额外 feature）
- [x] M6.12 布局验证：真机在 `900×620`（最小窗）与 `900×1400`（窄高）下截图确认 —— 无横向溢出、标签不被截断、可滚动。**未做**：150%/200% DPI
- [x] M6.13 本地人设详情与列表宽度：`persona.show` 命令（正文 + 元数据，缺失 id 退出 2）+ `metis show <id>`（人类可读 / `--json`）；GUI 列表每行「详情」（无障碍名带行标识）打开**固定宽度 680** 的独立浮层，正文按 Markdown 渲染、长说明/路径自动换行、`Esc`/背景关闭；列表说明改为单独一行并截断，不再撑出卡片。证据：`persona_show_*`（2）、CLI `show_*`（2）、`a_long_description_stays_inside_the_row_width`、`a_long_description_does_not_widen_the_persona_overlay`、`a_wide_markdown_table_does_not_widen_the_persona_overlay`、`the_persona_detail_overlay_*`（2）、`the_persona_detail_renders_loading_and_error_states`、`an_empty_persona_body_is_explained_instead_of_blank`、app 详情加载/错误（2）；真机 agent-desktop 验证浮层宽度、换行与宽表格横向滚动
- [x] M6.14 窗口可缩到 `640×480`：`MIN_WINDOW` 下调（原 `900×620`）；顶栏与状态条改 `horizontal_wrapped`；控制台人设卡片在窄宽度时动作按钮换到第二行、人设下拉截断；侧栏仍低于 900 自动折叠。证据：`topbar_wraps_without_losing_actions_at_the_minimum_width`、`every_screen_renders_inside_the_minimum_content_width`、`every_screen_fits_the_minimum_window_width`（完整三面板）；真机 640 宽截图确认顶栏换行、四个屏幕无横向溢出

## M7 打包与发布 — owner: stabilize

- [x] M7.1 应用图标与 `VERSIONINFO`：`scripts/gen-icon.py` 程序化生成（1024 PNG + 多尺寸 ICO），两个 exe 都用 `winresource` 嵌入产品名/版本/公司/许可。**macOS bundle id 未做**（没在 macOS 上验证过）
- [x] M7.2 便携包：`scripts/package.ps1` → zip + SHA256，包内说明写清"先 clean 再删目录"。**安装器未做**（便携包不需要管理员权限，等有用户要求再说）
- [x] M7.3 版本一致性：`Cargo.toml` 是唯一真相；`xtask version` 读它，`package.ps1` 打包前校验 exe 自报版本，`release.yml` 校验 tag 与它一致
- [x] M7.4 发布工作流：`.github/workflows/release.yml`（tag 触发，校验 tag ≡ Cargo.toml ≡ 二进制版本，三平台产物 + SHA256 + Release notes）
- [x] M7.5 首个公开版本：**v0.1.0 已发布** —— https://github.com/dulingzhi/metis/releases/tag/v0.1.0
  - Windows x64 zip 8.93MB / Linux x64 tar.gz 10.43MB / macOS ARM64 tar.gz 7.67MB
  - 每个都有 `.sha256`；已实测下载 → 校验 → 解压 → `metis --version`

## M9 GUI 重构（借鉴 craft-anything 的设计系统）— owner: maintainers

- [x] M9.1 P1：全新 token 体系（27 个字段、5 层背景、WCAG AA 全对校验）+ 组件库（19 个组件）+ Lucide 图标（71 个）+ DM Sans 内嵌 + 组件画廊（`--example gallery`）
- [x] M9.2 P2：侧栏重构（248px / 折叠 56px、图标行、选中态、窄窗口自动折叠、`chevron-left` 图标补齐）
- [x] M9.3 P3：四个屏幕按新设计重写（`screens/mod.rs`：卡片分层、选择瓦片、状态胶囊、空状态、主/次/危险按钮；旧 `screens.rs` 只留顶栏与状态条 → `chrome.rs`）
- [x] M9.4 P4：命令面板（`Ctrl+K`、子序列匹配、`↑↓` 循环、`Enter` 执行、`Esc` 关闭、11 个动作全可键盘触达）。**未做**：自绘标题栏（系统标题栏已可拖动/缩放，收益低于风险）
- [x] M9.6 UI 自动化测试（`scripts/ui-test.ps1`，12 项交互用例，用 `agent-desktop` 驱动真实窗口）+ 侧栏无障碍语义修复
- [x] M9.5 P5：快照基线：5 张（4 屏 + 深色）、`cargo xtask snapshot [--update]`、**渲染完全确定**（fixture 固定路径/清空提示条）、实测能抓到 2px 改动

## M8 打磨 — owner: open

- [x] M8.1 深色主题：`Tokens::DARK` + WCAG AA 对比度校验 + 顶栏切换按钮 + `METIS_DARK` 环境变量
- [x] M8.2a 快捷键：`Ctrl+1..4` 切屏、`Ctrl+R` 刷新、`Ctrl+Enter` 应用、`Ctrl+L` 切主题（有焦点时让路）。仍未做：命令面板
- [x] M8.3 探测并行化：`detect_all` 用 scoped threads，保持注册表顺序
- [x] M8.6 覆盖率门禁修正：`--fail-under-branches` 不存在，导致门禁从第一天起从未跑通；改为 `--fail-under-lines 90 --fail-under-functions 85`
- [x] M8.4 错误文案统一：每个面向用户的错误都有补救建议，由 `cargo xtask remediation` 强制（`Internal` 豁免）
- [x] M8.5 备份目录体积管理：`backups usage` / `backups prune` + 关于屏三个按钮（默认预览，`--yes` 才删）

## M10 远端人设库（worldd / KK Hub） — owner: maintainers

> 对端：`https://n.kkpt.tech:8443`（`/hub` 是网页；API 在 origin 的 `/v1/hub/*`）。
> 契约：worldd 的 `/v1/hub/*`（参考项目 habblaud 的 `docs/world/hub-api.md`）。

- [x] M10.1 Hub 客户端：列表分页（cursor/limit）、详情、ETag 缓存、注入式网络、URL 校验（依赖 M2.1）。31 条单测 + 1 条 `#[ignore]` 真机；2026-11-09 对 `https://n.kkpt.tech:8443` 实测通过
- [x] M10.2 Hub 导入：expert → 人设映射、provenance front-matter、关联 skill 落到本地并写回路径、去重/覆盖、dry-run（依赖 M10.1、M2.2）。`hub_import` / `hub_skills` / `commands_hub` 共 25 条单测 + 1 条真机；技能目录带 manifest，只删自己写过的文件；2026-11-09 对 `https://n.kkpt.tech:8443` 实际导入并落技能通过
- [x] M10.3 CLI：`metis hub list|show|import|skill`（依赖 M10.2）。34 条 CLI e2e（含 hub 帮助与坏输入不联网）；2026-11-09 真机 `list` / `show` / `import` / `skill remove` 通过
- [x] M10.4 GUI：远端库卡片、分页（上一页 + 页码）、详情面板、skill 勾选、未校验二次确认（依赖 M10.1、M6.6）。人设库屏新增远端库卡片/内联详情；3 条无头渲染测试 + 快照基线重建；真实窗口 agent-desktop：加载 → 列表 → 详情通过
- [x] M10.5 文档与真机验证：DATA-FORMAT、STATUS、PITFALLS、真实 worldd 调用记录（依赖 M10.4）。文档审计完成；STATUS 增加真机记录；PARITY §10 标注为 Metis 独有扩展；当前无未完成项
- [x] M10.6 GUI 修正：详情改独立浮层；结果列表最大高度 480px + 列表内滚动；详情内容按 Markdown 渲染（依赖 M10.4）。无头浮层/列表高度测试；真实窗口 agent-desktop 验证浮层打开、Markdown 标题可见。补：嵌套滚动区加 `min_scrolled_height`，列表与详情正文不再被父容器剩余空间压扁（回归测试 2 条；真机验证详情浮层正文完整、Markdown 标题可见）。
- [x] M10.7 单可执行文件：`metis` 合并 CLI 与 GUI 壳；无参数在同一进程打开 GUI，有子命令走 CLI；Windows 链接为 GUI 子系统（双击无控制台，CLI 靠继承的 stdio 句柄）；`metis-gui` 改为库；打包/CI/脚本/文档只发一个 exe。证据：`launch_tests::*`（2 条）+ 34 条 CLI 管道 e2e；真机无参数开窗无控制台、`--version` / `status` 正常、管道 `$LASTEXITCODE` 正确；Windows zip 只有 `metis.exe`
- [x] M10.8 接管命令（shim）链路修复：脚本转发带 `--exec`；`launch --exec` 继承终端（`run_interactive`）并回传宿主退出码；CLI 模式 `AttachConsole(ATTACH_PARENT_PROCESS)` 让宿主在当前控制台运行（不另开窗口）；解析宿主时跳过 shim 目录避免递归；`apply`/GUI 提示 shim 目录必须在 PATH 最前面；GUI 通道开关**打开即落盘**，单通道切换不再误伤其他通道。证据：shim/which/app/model 单测 + 端到端假 harness（参数透传、stdin 继承、退出码 7 穿透、无递归、父子同一控制台句柄）
- [x] M10.9 GUI 启动自动配置 shim PATH：已武装 shim 且目录不在 PATH 时，调用 `channel.shim-path` 写**用户级** PATH（保留原值类型、广播 WM_SETTINGCHANGE、幂等、把目录移到最前）；只写 HKCU、不写系统级。证据：脚本生成/转义/dryRun 单测；dryRun 不触注册表

---

## 未完成项汇总

`cargo xtask pending` 会列出本文件里所有 `- [ ]` 行。CI 会拒绝"有 `- [ ]` 但所属条目已标 done"的提交。

## 僵尸条目检查

每个里程碑收尾时（见 `AGENTS.md` 的体检规则）：确认没有"标了 doing 但两周没动静"的条目；
有就退回 `todo` 并说明；确认没有"done 但未完成项还在"。
