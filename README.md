# Metis

**一份人设，统一管理多个 coding agent。原生 Rust + egui 实现。**

Metis 把 [agent-purge](https://github.com/ZiChuanLan/agent-purge) 的能力重做成一个纯 Rust 应用：一个 CLI（`metis`）、
一个 egui 桌面控制台（`metis-gui`），共用同一个 headless 引擎。它管理 Codex、Claude Code、OpenCode、pi、DSH、Kilo、
Hermes、Grok、omp 等 15 家执行器的系统提示词：选一份人设、定一个模式、打开要接管的通道，之后无论从命令行、
桌面图标还是插件入口启动，都是你的身份。

> 状态：**v0.1.0 —— 功能已对齐，可以日常使用**。15 家执行器、四条通道、CLI + 桌面控制台全部可用；
> 打包与发布流程已就位。功能基线是 agent-purge v0.2.19，逐项对齐见 `docs/PARITY.md`。

## 安装

从 [Releases](https://github.com/dulingzhi/metis/releases) 下载对应平台的包，解压即用：

| 平台 | 文件 |
|---|---|
| Windows x64 | `metis-<版本>-windows-x64.zip` |
| Linux x64 | `metis-<版本>-linux-x64.tar.gz` |
| macOS ARM64 | `metis-<版本>-macos-arm64.tar.gz` |

免安装、不写注册表、不需要管理员权限。校验和在同目录的 `.sha256` 文件里。

```sh
metis --help                              # 全部命令
metis doctor                              # 看看本机装了哪些 agent
metis import dulingzhi/metis-personas     # 导入官方人设库
metis-gui                                 # 图形控制台
```

**卸载**：先 `metis clean` 撤销接管，再删目录 —— 直接删目录不会撤销已写入各 agent 配置的内容。

## 为什么要有 Metis

agent-purge 是一套完整的、用 Node.js 写的工具，能力完备但受两条限制：

1. **运行时依赖**：需要 Node.js 20+（或用自带的独立 exe 打包），安装器、薄包、`npx` 三条路径各有边界。
2. **界面形态**：配置台是本地 Web 页面（`http://127.0.0.1:7788`）。它能用，但每个平台都要处理端口占用、浏览器
   生命周期、页面刷新退出倒计时、CSRF 同源校验这些与产品目标无关的问题。

Metis 用 Rust 换掉这两条：单个原生可执行文件，不需要外部运行时；配置台是一个真正的桌面窗口（egui），
没有端口、没有浏览器、没有"刷新把服务杀了"。

**不做的事**：不引入网络服务、不接管宿主进程、不发送任何数据。Metis 只在你自己的文件系统和自己的机器上工作。

## 与 agent-purge 的关系

| 维度 | 约定 |
|---|---|
| 功能基线 | agent-purge **v0.2.19**，逐项对齐，见 `docs/PARITY.md` |
| 代码 | **完全独立实现**。不复制 agent-purge 的代码、命名、文案；只对齐可观察行为 |
| 数据目录 | **独立**：`~/.metis/`（Windows 为 `%USERPROFILE%\.metis\`），不读写 `~/.agent-purge/` |
| 迁移 | 提供 `metis import agent-purge` 把原有人设与 state 导入 `~/.metis/`，原目录只读不动 |
| 注入产物 | 与基线一致：宿主需要的是 JS/TS 插件或配置文件时，Metis 生成同样的产物（Metis 自身不运行 JS） |
| 网络 | 与原版一致：只监听本机的能力（验证用假 provider）默认关闭，更新检查可关 |

## 能力概览

- **一份配置，多种入口**：CLI 与桌面控制台共用 `state.json`，人设、模式、通道选择只有一份真相。
- **按执行器独立设置**：默认跟随全局，也能为某一家单独选人设或模式（`overrides`）。
- **四条通道**：`install`（写宿主配置/插件）、`shim`（PATH 前置同名命令）、`slash`（会话内 `/purge` 扩展）、
  `project`（项目根的 `AGENTS.md` / `CLAUDE.md` 托管块）。
- **四种模式**：`replace` / `strict` / `append` / `off`，并区分"配置已写"与"当前会话已生效"。
- **可逆**：每次写入都留所有权凭据（footprint），只回滚自己写过的字节；外部修改保留并明确报错。
- **可验证**：本地假 provider 抓真实请求体，证明人设到底进没进 system 层。
- **能力诚实**：每家 harness 的每条通道能不能用、为什么不能用，都在 UI 里说清楚，不摆点了没反应的开关。

完整的命令面、通道语义、每家 harness 的注入方式见 `docs/PARITY.md` 与 `docs/HARNESS-MATRIX.md`。

## 目录结构

```text
crates/
  metis-core        L0  域模型与错误契约（无 I/O）
  metis-paths       L0  路径解析（可注入根，便于测试）
  metis-fs          L0  原子写、所有权凭据、锁、备份
  metis-persona     L1  人设库（front-matter、导入、模板）
  metis-harness     L1  执行器适配层：探测、能力表、启动参数
  metis-channel     L1  四条通道的 apply / revert
  metis-process     L2  外部进程执行（超时、stdin、杀进程组）
  metis-engine      L2  状态、激活解析、应用编排、验证、更新
  metis-ui-egui     L3  egui 桌面界面（唯一允许依赖 egui 的 crate）
  metis-testkit         测试夹具（临时 home、假 harness、golden）
apps/
  metis             CLI
  metis-gui         桌面应用
xtask/              工程门禁：layers / fmt / clippy / test / coverage / secrets / ci / pending / version / docs / i18n / remediation / golden / icons / snapshot
```

## 文档索引

| 文档 | 内容 |
|---|---|
| [`AGENTS.md`](AGENTS.md) | **先读这个**。AI 代理与贡献者的强制规则、自我迭代协议、完成门禁 |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | 工作区分层、crate 职责、数据流、关键不变式 |
| [`docs/PARITY.md`](docs/PARITY.md) | 功能对齐矩阵：agent-purge 的每个命令/通道/开关 → Metis 的归属与状态 |
| [`docs/HARNESS-MATRIX.md`](docs/HARNESS-MATRIX.md) | 15 家执行器的探测、落点、通道能力、热切能力 |
| [`docs/UI.md`](docs/UI.md) | egui 界面规格：屏幕、面板、交互、主题、无障碍 |
| [`docs/DATA-FORMAT.md`](docs/DATA-FORMAT.md) | `~/.metis/` 磁盘格式、版本与迁移、导入规则 |
| [`docs/TESTING.md`](docs/TESTING.md) | 单测全覆盖契约、测试夹具、覆盖率门槛、CI 门禁 |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | 从零搭一个 egui 应用的实操指南（工程实践来自 craft-anything） |
| [`docs/EXTENDING.md`](docs/EXTENDING.md) | 加一家 harness / 一条通道 / 一个 CLI 命令要动哪些文件 |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | 里程碑、每项的归属与状态、未完成项清单 |
| [`docs/STATUS.md`](docs/STATUS.md) | 当前真实进度（已完成 / 未开始） |
| [`docs/PITFALLS.md`](docs/PITFALLS.md) | 已经踩过的坑与对应规则 |

## 许可

MIT OR Apache-2.0。
