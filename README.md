# Metis

**一份人设，统一管理多个 coding agent。**

Metis 是一个纯 Rust 写的本地工具：它把你写的系统提示词（「人设」）注入到各家 coding agent
里，让你在 Codex、Claude Code、OpenCode、pi、DSH 等 15 家工具里都用同一套工作方式。

单可执行文件，不需要 Node.js 之类的运行时；不联网、不上传任何数据；配置台是一个真正的
桌面窗口，不是本地网页。

```
┌─────────────────────────────────────────────────────────┐
│  Metis        你写一份人设                               │
│                  ↓                                       │
│              15 家 coding agent 都用同一份               │
└─────────────────────────────────────────────────────────┘
```

---

## 它解决什么问题

你在 Codex 里调好了一套工作习惯——中文回答、先给结论、改代码前先读现有实现。
换到 Claude Code，这些全没了，得重新说一遍。

各家 agent 都有「系统提示词」或「全局指令」的机制，但落点、格式、生效时机都不一样。
Metis 把这些差异收在一处：你只维护一份人设，剩下的它来做。

**它不是什么**：不是提示词模板库，不是对话记录管理，不接管宿主进程，不代理 API 请求。
它只写你机器上的配置文件，而且**每一次写入都可以逐字节撤回**。

---

## 安装

从 [Releases](https://github.com/dulingzhi/metis/releases) 下载对应平台的包，解压即用。

| 平台 | 文件 |
|---|---|
| Windows x64 | `metis-<版本>-windows-x64.zip` |
| Linux x64 | `metis-<版本>-linux-x64.tar.gz` |
| macOS ARM64 | `metis-<版本>-macos-arm64.tar.gz` |

免安装、不写注册表、不需要管理员权限。每个包都有对应的 `.sha256`：

```sh
sha256sum -c metis-0.1.0-linux-x64.tar.gz.sha256
```

**从源码构建**：

```sh
git clone https://github.com/dulingzhi/metis
cd metis
cargo build --release
# 产物：target/release/metis（CLI）、target/release/metis-gui（桌面控制台）
```

需要 Rust **1.98+**（用了 edition 2024 与 let-chains）。

---

## 快速上手

```sh
metis doctor                                  # 1. 看看本机装了哪些 agent
metis persona concise --body "用简体中文协作。先给结论，再给依据。" \
                      --name "简洁协作"        # 2. 写一份人设
metis use concise                             # 3. 选它
metis arm codex                               # 4. 选择要接管的 agent
metis apply                                   # 5. 真的写进去
```

> `persona` 的第一个参数是 **id**，只能用字母、数字、`.`、`-`、`_`；
> 显示名用 `--name` 指定（可以写中文）。

或者从官方人设库导入现成的：

```sh
metis import dulingzhi/metis-personas   # 5 份起步人设
metis ls                                # 看看有哪些
metis use concise                       # 用其中一份
```

也可以直接浏览远端人设库（worldd / KK Hub）：

```sh
metis hub list                          # 分页浏览 expert
metis hub show expert/<slug>            # 看详情（prompt、knowledge、关联技能）
metis hub import expert/<slug>          # 导入人设；关联 skill 落到 ~/.metis/skills/
```

`metis hub` 默认指向内置的公开 Hub；用 `--url` 或 `METIS_HUB_URL` 可以换成自建 worldd。
导入只写本地人设与技能目录，**不会自动 apply**。

图形界面：

```sh
metis-gui
```

人设库屏可以直接浏览/导入远端 Hub（worldd / KK Hub）；导入只写本地，不会自动 apply。

---

## 核心概念

### 人设（persona）

一份人设就是一个 Markdown 文件，带 YAML front-matter：

```markdown
---
id: concise
name: 简洁协作
description: 中文、先结论、改代码前先读现有实现
harness: ["*"]
model: ["*"]
order: 10
---

用简体中文协作。先给结论，再给依据。

修改代码前先读现有实现；不确定的地方明确说明，不要猜。
```

`---` 下面是**正文**，也就是真正被注入的内容。front-matter 只用于管理。

`id` 只能用字母、数字、`.`、`-`、`_`（要能当文件名）；`name` 可以写中文，是给人和界面看的。

`harness` / `model` 声明这份人设适配哪些执行器与模型，`["*"]` 表示不限。

### 模式（mode）

三档，决定注入的「强度」：

| 模式 | 做什么 | 什么时候用 |
|---|---|---|
| `replace` | 优先替换身份行 | 默认。想让 agent「变成」这个人设 |
| `strict` | 替换可控入口中的宿主文本 | 宿主自己的话太多，盖过你的设定 |
| `append` | 追加一份，保留宿主原文 | 只想加约束，不想覆盖宿主的规则 |

另有 `off`：暂停这一家，但不改变其他设置。

### 四条通道（channel）

同一份人设可以有四种注入方式，能力和副作用各不相同：

| 通道 | 怎么生效 | 特点 |
|---|---|---|
| `install` | 写入 agent 自己的全局配置 | 最可靠；新会话读取，部分宿主需重启 |
| `shim` | 在 PATH 前置一层同名包装脚本 | 接管终端裸命令；不影响已运行会话 |
| `slash` | 装成宿主扩展，用 `/purge on` 开启 | 会话内热切换，不用重开 |
| `project` | 写进当前目录的 `AGENTS.md` 之类 | 只影响这个项目 |

不是每家都支持全部四条——`metis harnesses` 会列出每家支持什么、不支持的**原因**
是什么。

### 激活（activation）

一份人设写在磁盘上，不等于当前会话已经用它。Metis 把这两件事分开报告：

- **`written`**：配置写进了磁盘
- **`runtimeConfirmed`**：本机实测确认请求里真的带上了人设

`metis verify` 会启动一个本地假 provider，抓宿主的**真实请求**，判定人设到底落在哪一层
（system / developer / 项目上下文）。这是唯一能确认「真的生效了」的手段。

---

## 常用命令

### 日常操作

| 命令 | 作用 |
|---|---|
| `metis status` | 当前配置全貌（加 `--json` 机器可读） |
| `metis doctor` | 探测本机装了哪些 agent（15 家） |
| `metis harnesses` | 每家的能力表：支持哪些通道、为什么不可用 |
| `metis ls` | 列出人设，`*` 标当前 |
| `metis use <id>` | 换人设 |
| `metis mode <m>` | 换模式 |
| `metis arm <h>` / `metis disarm <h>` | 开启 / 关闭某家的通道 |
| `metis apply` | 应用当前配置 |
| `metis pause` / `metis resume` | 暂停 / 恢复注入 |

### 人设管理

| 命令 | 作用 |
|---|---|
| `metis persona <id> --body "..."` | 新建或修改 |
| `metis rm <id>` | 删除 |
| `metis import <目录\|owner/repo>` | 从本地或 GitHub 导入 |
| `metis hub list\|show\|import` | 浏览远端 Hub、看详情、导入（含关联技能） |
| `metis use` | 不带参数：打印当前人设与全部分家例外 |

### 验证与维护

| 命令 | 作用 |
|---|---|
| `metis verify [h…]` | 实测：人设进没进 system 层 |
| `metis report` | 读最近一次验证报告 |
| `metis clean [--dry-run]` | 撤回全部接管（保留人设与状态） |
| `metis uninstall <h>` | 撤回某一家并删掉已写入的内容 |
| `metis self-uninstall` | 撤回写入**并删除数据目录**（不可逆） |
| `metis backups usage\|prune` | 备份体积管理 |
| `metis update` | 检查新版本（默认只检查，不改文件） |

### 其他

| 命令 | 作用 |
|---|---|
| `metis launch <h> [--exec]` | 启动宿主（默认只打印命令，`--exec` 才真跑） |
| `metis ask <h> <prompt>` | 一次性问答 |
| `metis armor …` | 从实测结果推导策略 |
| `metis journal [--month]` | 变更日志 |

全局旗标（写在子命令**之前**）：

```sh
metis --home /path/to/data status    # 覆盖数据目录
metis --lang en status               # 界面语言
```

---

## 分家配置

全局选一份，但某一家想用别的：

```sh
metis use strict --for codex         # 只有 codex 用 strict
metis mode append --for claude       # 只有 claude 用 append
metis use                            # 查看当前人设与全部例外
```

取消例外：

```sh
metis use --for codex follow         # codex 回到跟随全局
```

---

## 数据在哪

默认 `~/.metis/`：

```
~/.metis/
  state.json            全局状态（人设、模式、武装的通道、分家例外）
  personas/             你的人设
  footprints.json       所有权账本：我们改过哪个文件的哪些字节
  backups/              整文件回滚用的原件
  journal/              交换日志（崩溃恢复用）
  verification/         验证报告
```

覆盖位置：`metis --home <dir> …`，或环境变量 `METIS_HOME`。

**`footprints.json` 是撤回能力的核心**：每次写盘前先记下原始字节，撤回时逐字节还原，
不依赖「重新生成一份看起来一样的」。外部改动会被检测到并拒绝覆盖，而不是静默丢弃。

---

## 卸载

**先撤销接管，再删目录。**

```sh
metis clean              # 撤掉写进各处配置的内容，保留人设
metis self-uninstall     # 撤接管 + 删除数据目录（不可逆）
```

直接删掉数据目录**不会**撤销已经写进各 agent 配置里的内容——那些文件在别的地方。

---

## 安全与边界

| 项目 | 做法 |
|---|---|
| **不联网** | 只有 `metis import owner/repo`、`metis hub` 与 `metis update` 会访问网络，且都可预期；`metis hub` 只读公开数据，不发送遥测 |
| **不上传** | 没有遥测、没有统计、没有「匿名使用数据」 |
| **不接管宿主** | 不 hook 进程、不代理 API、不注入内存 |
| **可撤销** | 每次写盘前记录原始字节；撤回逐字节还原 |
| **不覆盖外部改动** | 检测到别人改过就报错，让人来决策 |
| **永不崩溃** | 生产代码零 `unwrap`/`panic`；每个错误都带补救建议 |
| **安全 Rust** | 全仓库 `unsafe_code = "forbid"`，无例外 |

---

## 目录结构

```
crates/
  core        L0  域模型与错误契约：HarnessId、Mode、Channel、Activation、MetisError
  paths       L0  路径解析：~/.metis 布局、各家主目录、环境变量覆盖
  fs          L0  原子写、所有权账本、备份、控制锁、逐字节比较
  persona     L1  人设库：front-matter 解析、列表、导入、内置模板
  harness     L1  执行器适配：探测、能力表、启动参数、静态元数据
  channel     L1  四条通道的 apply / revert
  process     L2  外部进程：超时、stdin 策略、输出上限、进程组终止
  engine      L2  状态与编排：state.json、activation、apply、journal、verification
  ui-egui     L3  egui 桌面界面（唯一允许依赖 egui 的 crate）
  testkit        测试夹具：临时 home、假 harness、golden 比对
apps/
  metis          CLI
  metis-gui      桌面应用（eframe 壳）
xtask/           工程门禁与工具链
assets/          应用图标
docs/            设计文档与规格
scripts/         打包、图标生成、GUI 自动化测试
```

**分层是强制的**：`cargo xtask layers` 会拒绝跨层依赖。规则很简单——代码放在能拥有它的
最低层，UI 只是薄客户端。

---

## 贡献

### 开发环境

```sh
cargo --version      # 需要 1.98+
cargo xtask test     # 跑全部测试
cargo xtask ci       # 本地干跑 CI 的关键作业
```

### 提交前必须过的门禁

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask layers       # 分层没被破坏
cargo xtask docs         # 文档引用与路线图状态一致
cargo xtask secrets      # 没有密钥/本机路径
cargo xtask remediation  # 每个错误都有补救建议
cargo xtask i18n         # 两张文案表键一致
cargo xtask coverage     # 行覆盖率 ≥ 90%
cargo xtask snapshot     # 界面快照与基线一致（改动涉及 ui-egui 时）
```

界面的交互测试用 `agent-desktop` 驱动真实窗口（`pwsh scripts/ui-test.ps1`）——
它读的是系统无障碍树，不是截图猜坐标。用法见 [`AGENTS.md`](AGENTS.md)。

### 加一家新的 agent

1. 在 `crates/harness/src/spec.rs` 的注册表里加一条
2. 实现能力表：支持哪些通道、不支持时**说明原因**（那句话会直接显示给用户）
3. 在 `docs/HARNESS-MATRIX.md` 补一行
4. 测试：探测（装了/没装/只有桌面版）+ 每条通道的 apply → revert 逐字节还原

### 加一份人设到官方源

官方人设库在 [dulingzhi/metis-personas](https://github.com/dulingzhi/metis-personas)，
提 PR 即可。收录标准写在那个仓库的 README 里。

### 代码风格

- **不写 `unwrap` / `panic`**：这是处理用户真实配置文件的工具，崩一次就是别人的损失
- **错误要带补救建议**：告诉用户「现在该做什么」，不只是「哪里错了」
- **区分「写入了」与「生效了」**：任何界面文案、日志、测试都不许混为一谈
- **变更必须可逆**：写盘前先记原始字节
- **测试不碰真实环境**：不读真实 `$HOME`、不调真实 harness、不依赖墙钟
- 提交信息用中文祈使句，说清「改了什么、为什么」

更多细节见 [`AGENTS.md`](AGENTS.md)（强制规则与自我迭代协议）和
[`docs/PITFALLS.md`](docs/PITFALLS.md)（踩过的坑，每条都附回归测试）。

---

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | 分层、数据流、关键设计决策 |
| [`docs/DATA-FORMAT.md`](docs/DATA-FORMAT.md) | `state.json`、账本、journal 的格式与迁移 |
| [`docs/HARNESS-MATRIX.md`](docs/HARNESS-MATRIX.md) | 15 家 × 四条通道的能力真相（**唯一来源**） |
| [`docs/UI.md`](docs/UI.md) | 界面规格、主题 token、无障碍要求 |
| [`docs/TESTING.md`](docs/TESTING.md) | 测试策略与夹具 |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | 工程约定、质量门禁 |
| [`docs/EXTENDING.md`](docs/EXTENDING.md) | 怎么加 harness / 通道 / 命令 |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | 计划与状态 |
| [`docs/PITFALLS.md`](docs/PITFALLS.md) | 已经踩过的坑 |
| [`AGENTS.md`](AGENTS.md) | 给 AI 代理与贡献者的强制规则 |

---

## 许可

**MIT OR Apache-2.0**，任选其一。详见 [`LICENSE-MIT`](LICENSE-MIT) 与
[`LICENSE-APACHE`](LICENSE-APACHE)。

第三方依赖与字体的许可见 [`ATTRIBUTION.md`](ATTRIBUTION.md)。
