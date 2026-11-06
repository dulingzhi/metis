# 路线图

> 相关：[AGENTS.md](../AGENTS.md) · [当前状态](STATUS.md) · [功能对齐](PARITY.md)

> 这是**唯一**的计划来源（见 `AGENTS.md`）。每条有一行标题、负责人（只能是角色）、状态与未完成项。
> 状态：`todo`（未开始）· `doing`（进行中）· `done`（完成）· `blocked`（被外部条件卡住）· `decision`（等维护者拍板）。
> 认领方式：在未完成项后写 `— claimed: <名字>, branch <分支>`，并尽早推送分支。

## 现状

M0 阶段：**只有文档**。任何"已实现"的说法都必须能在 `STATUS.md` 里找到对应代码位置，否则视为未实现。

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

---

## 未完成项汇总

`cargo xtask pending` 会列出本文件里所有 `- [ ]` 行。CI 会拒绝"有 `- [ ]` 但所属条目已标 done"的提交。

## 僵尸条目检查

每个里程碑收尾时（见 `AGENTS.md` 的体检规则）：确认没有"标了 doing 但两周没动静"的条目；
有就退回 `todo` 并说明；确认没有"done 但未完成项还在"。
