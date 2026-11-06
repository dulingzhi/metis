# 执行器能力矩阵

> 相关：[功能对齐](PARITY.md) · [架构](ARCHITECTURE.md) · [扩展指南](EXTENDING.md) · [坑](PITFALLS.md)

> 状态：设计已定，代码未落地（M0）。本文件是"每家 harness 到底能做什么、为什么不能做什么"的**唯一**真相来源
> （`AGENTS.md` 铁律 5）。`metis-harness` 的能力表与 `metis-ui-egui` 的界面文案都必须与本表一致，
> `cargo xtask ci` 会核对本表的 harness 集合与代码里的注册表。
>
> 基线：agent-purge v0.2.19 的 `matrix.json`（15 家）、`lib/caps.mjs`、`lib/persist.mjs`。
> 原始调研见 `.research/agent-purge-spec.md` 第 5 节。

## 0. 读法

- **层**：`sys` = 真的进系统提示词；`user` = 只进上下文块/用户消息，**压不住身份**。
  Metis 只把 `sys` 层当作可用能力；`user` 层落点一律标为"不可用"并说明原因（基线的做法，是对的）。
- **通道**：`install` 常驻配置/插件 · `shim` 裸命令接管 · `slash` 会话内 `/purge` · `project` 项目规则文件。
- **热切**：同会话内换人设是否在下一轮生效。分 `command`（会话里打 `/purge`）与 `state`（改设置下一轮生效）。
- **抓包**：能否用本地假 provider 抓到真实请求以验证命中层。

## 1. 总览（15 家）

| harness | 类别 | 可注入 | 热切 | install 落点（层） | shim | slash | project | 抓包 |
|---|---|---|---|---|---|---|---|---|
| `pi` | 原生扩展 | ✔ | ✔ next-turn | `AGENTS.md`（sys） | ✔ | ✔ sys | ✔ | ✔ |
| `pi-desktop` | 配置 + 插件 | ✔ | ✔ next-turn | `SYSTEM.md`（sys） | ✘ 桌面优先 | ✔ sys（imported 插件） | ✔ | ✘ |
| `omp` | 启动参数 + 扩展 | ✔ | ✔ next-turn | `agent/AGENTS.md`（sys） | ✔ | ✔ sys | ✔ | ✔ |
| `opencode` | 插件 | ✔ | ✔ next-turn | `plugins/*.ts`（sys） | ✔ | ✘ 隐藏（user 层无效） | ✔ | ✔ |
| `opencode2` | 插件 v2 | ✔ | ✔ next-turn | `plugins/*-v2.ts`（sys） | ✔ | — | ✔ | ✔ |
| `dsh` | cordis 插件 | ✔ | ✔ next-turn | `profiles/*/node_modules/…`（sys） | ✘ 只能起 headless | ✔（需 install 配合） | — | ✔（需 `--profile headless` 预处理） |
| `codex` | 启动参数 | ✔ | ✘ | `config.toml` 顶层键（sys） | ✔ | ✘ 隐藏 | ✔ | ✔（实际角色 `developer`） |
| `claude` | 启动参数（+ 基线另有二进制补丁） | ✔ | ✘ | `CLAUDE.md`（**user**，不可用） | ✔ | ✘ 隐藏 | ✔ | ✔ |
| `grok` | 启动参数 | ✔ | ✘ | `AGENTS.md`（sys，≤9600 字符） | ✔ | — | ✔ | ✔ |
| `codebuddy` | 启动参数 | ✔ | ✘ | 无落点 | ✔ | — | — | ✔ |
| `hermes` | 文件交换 | ✔ | ✘ | `SOUL.md`（sys） | ✔ | — | — | ✘ |
| `kilo` | 配置 | ✔ | ✘ | `agent/code.md`（sys）+ `AGENTS.md`（append） | ✔ | — | ✔ | ✔ |
| `workbuddy` | 配置 | ✔ | ✘ | `settings.json → personalization.customPrompt`（sys） | ✘ 桌面端 | — | ✘ 仅用户上下文 | ✘ |
| `zcode` | 配置或 hook | ✔ | ✘ | `AGENTS.md`（sys） | ✘ 桌面优先 | — | ✔ | ✘ |
| `antigravity` | 二进制补丁 | **✘** | ✘ | 无 | ✘ | ✘ | — | ✘ |

## 2. 探测规则

| harness | 可执行名 | 环境变量 / 主目录 | 标记文件 | 桌面 exe |
|---|---|---|---|---|
| `pi` | `pi` | `PI_HOME` → `~/.pi/agent` | `extensions` | — |
| `pi-desktop` | **无 CLI** | `PI_DESKTOP_DATA_DIR` → `~/.pi-desktop` | `plugins`、`pi.sqlite`、`secrets` | `PI-Desktop.exe`、`PI-Desktop-Portable*.exe` |
| `omp` | `omp` | `OMP_HOME` → `~/.omp` | `agent/config.yml` | — |
| `opencode` | `opencode` | `XDG_CONFIG_HOME` **是配置根** → `$XDG_CONFIG_HOME/opencode`（默认 `~/.config/opencode`） | 6 项 | — |
| `opencode2` | `opencode2` | 同 `opencode`（`XDG_CONFIG_HOME` 也是配置根，要拼一级） | 同 `opencode` | — |
| `dsh` | `dsh` | `DSH_HOME` → `~/.dsh` | `profiles` | `DeepSeek Harness.exe` |
| `codex` | `codex` | `CODEX_HOME` → `~/.codex` | `config.toml` | — |
| `claude` | `claude` | `CLAUDE_CONFIG_DIR` → `~/.claude` | `settings.json` | — |
| `grok` | `grok` | `GROK_HOME` → `~/.grok` | `config.toml` | — |
| `codebuddy` | `codebuddy` | `CODEBUDDY_HOME` → `~/.codebuddy` | `history.jsonl` | — |
| `hermes` | `hermes` | `HERMES_HOME` → Windows `%LOCALAPPDATA%\hermes`，否则 `~/.hermes` | `hermes-agent` | — |
| `kilo` | `kilo`、`kilocode` | `KILO_CONFIG_DIR` → `~/.config/kilo` | `agent` | — |
| `workbuddy` | `WorkBuddy`、`workbuddy` | `WORKBUDDY_CONFIG_DIR` → `~/.workbuddy` | `WorkBuddy.exe`、`resources/app.asar` | `WorkBuddy.exe` |
| `zcode` | `zcode` | `ZCODE_HOME` → `~/.zcode` | `v2/provider_config.json` | `ZCode.exe` |
| `antigravity` | `antigravity` | `Programs/antigravity` | `resources/bin/language_server.exe` | `Antigravity.exe` |

Windows 上还要看 `%LOCALAPPDATA%\Programs\…` 之类的常见安装路径。`metis-paths` 把这张表实现成数据，
不写成 `if` 链。

## 3. 逐家说明与 Metis 决策

### `pi`

- **install**：`<pi>/AGENTS.md` 托管块（sys，每次启动重读；正文是**快照**，换人设要重装）。
- **启动**：只透传 `pi <args>`，命令行**不加**人设参数 —— 因为 `pi.cmd` 走 cmd.exe，10K 人设会撞 Windows
  8191 的命令行上限（基线实测"命令行太长"）。注入靠扩展。
- **slash**：`<pi>/extensions/agent-purge.ts`，`before_agent_start` 改写 systemPrompt，`/purge on|off|status|reload|mode|always`。
- **oneshot**：`pi -p <prompt>`。
- **Metis 决策**：全通道对齐。扩展源码由 Metis 自己实现（独立产物），语义等价。

### `pi-desktop`

- **无 CLI 入口**：`metis pi-desktop` 明确报错并说明"桌面版没有命令行入口"。
- **install**：`<pi>/SYSTEM.md` 托管块（与 `pi` CLI 共用 `~/.pi/agent`；装了两边都变，界面要提示）。
- **slash/热切**：`<pi-desktop>/plugins/imported/agent-purge/` + `registry.json` 登记（首次安装需重启一次，
  之后 `/purge` 与改设置都即时生效）。
- **project**：`AGENTS.md`；注意项目级 `.pi/SYSTEM.md` 优先级高于全局。
- **抓包**：无法隔离 → 状态 `blocked`。
- **Metis 决策**：install + slash + project 对齐；不提供 CLI 启动（与基线一致，并给出同样的解释）。

### `omp`

- **启动**：`--system-prompt <text>`（replace/strict）或 `--append-system-prompt <text>`（append）。
- **install**：`<omp>/agent/AGENTS.md`（就是它的 rules 文件，`--no-rules` 关掉的就是这个）。
- **slash**：`<omp>/agent/extensions/agent-purge.ts`，`before_agent_start` 改写 systemPrompt 数组。
- **特例**：`--profile` 或 `OMP_PROFILE` 存在、或 `--no-extensions` 时，不启用临时扩展。
- **已知限制**：`user` 层实测会段错误（exit 0xC0000005），该层不可测；`sys` 层基线探针 50%，建议加种子。
- **Metis 决策**：对齐。`--auto on|off` 写 `state.ompAuto`。

### `opencode`

- **启动**：临时插件 `<cwd>/.opencode/plugin/agent-purge.ts`，退出时撤销。
- **install**：`<opencode>/plugins/agent-purge.ts`（4 个历史落点都要能被卸载扫描到）。
- **热切**：插件钩子每轮读 state，`kind: "state"`。
- **限制**：`--attach` 被显式拒绝（一次性启用使用独立宿主）。
- **slash**：文件存在但**不对外提供**（user 层实测无效）。
- **Metis 决策**：对齐，包括"不提供无效的 slash 开关"这条。

### `opencode2`

- 与 `opencode` **物理分开**：v2 插件 API 不同，不能混用。
- **启动**：需要私有 server（`opencode2 serve --stdio --hostname 127.0.0.1 --port 0`），解析 readiness URL 后
  客户端加 `--server <url>`。
- **oneshot**：`opencode2 run <prompt>`（headless 用 `--standalone --model …`）。
- **Metis 决策**：对齐；私有 server 是 Metis 唯一会监听端口的地方之一（只绑 `127.0.0.1`、随机端口、
  随进程退出）。

### `dsh`（DeepSeek Harness）

- **profile 语义**：`desktop` / `web` / `headless` / `sdk`，**只装进已经存在的 profile**
  （`profiles/*/package.json` 存在才算）；一个都没有 → 明确报错"先启动一次 dsh"。
- **install**：cordis 插件包（每轮读 state）。
- **slash**：`/purge` 注册插件；它**只是开关**，注入仍然靠常驻插件 —— 所以开 slash 时要级联安装 install
  （基线就是这么做的，Metis 保持）。
- **shim 不可用**：dsh 命令行只能起 headless，接管裸命令会让双击/网页入口变成终端会话。
- **抓包**：需要 `--profile headless` 预处理。
- **Metis 决策**：对齐，包括级联安装与"slash 只是开关"的解释。

### `codex`

- **启动**：`-c model_instructions_file="<path>"`（正斜杠）；append 用 `-c developer_instructions="<JSON>"`。
  TOML 值必须是合法 basic string，否则空格/引号/换行会解析失败。
- **install**：`config.toml` 的**顶层键**，必须写在第一个 `[table]` 之前。
- **实际请求角色是 `developer` 不是 `system`** —— 界面与文档必须这么写，否则用户会以为没生效。
- **热切**：不支持（config 启动时读，会话内没有重拼 system 的钩子）。
- **Metis 决策**：对齐。

### `claude`

- **启动**：`--system-prompt-file <path>`；append 用 `--append-system-prompt-file <path>`。
- **install**：`CLAUDE.md` 是 `<system-reminder>` 上下文块 → **user 层，压不住身份**。
  Metis 的做法与基线一致：**不把这条通道当可用能力**，界面不摆开关，只说明原因。
- **残留**：不补丁时残留 62 字身份句（`You are a Claude agent, built on Anthropic's Claude Agent SDK.`）。
- **Metis 决策**：启动参数通道对齐；**二进制补丁不做**（见 `PARITY.md` 的 decision 条目）。

### `grok`

- **启动**：`--system-prompt-override <text>`；append 用 `--rules <text>`。
- **install**：`<grok>/AGENTS.md`，内容被 append 进系统提示，**单文件上限 10000 字符** → 超出要截断并附说明行。
- **Metis 决策**：对齐（含截断行为与提示）。抓包需要设置 `XAI_API_KEY` 等辅助变量（只用假值）。

### `codebuddy`

- **启动**：`--system-prompt-file <path>`；append 用 `--append-system-prompt <text>`。
- **install 不可用**：只有 `settings.json`（插件开关/模型），没有能把文本塞进提示的词条。
- **oneshot**：`codebuddy -p <prompt> --max-turns 5 […]`。
- **Metis 决策**：对齐。

### `hermes`

- **启动**：非 off 时追加 `--ignore-rules`，并把 `SOUL.md` 换成正文，退出时还原。
- **install**：`SOUL.md` 托管块（身份槽位 #1，**只在会话首次/重建时读**；续会话沿用 session DB）。
- **热切**：不支持 —— 改文件不当场生效。
- **抓包**：无隔离适配器 → `blocked`。
- **Metis 决策**：对齐。

### `kilo`

- **install**：replace/strict 覆盖 `<kilo>/agent/code.md`（带 `description`/`mode: primary` front-matter）；
  append 写 `<kilo>/AGENTS.md` 托管块。
- **启动**：`kilo run <args>` + 临时安装，退出时撤销。
- **oneshot**：`kilo run <prompt> --format json`。
- **残留**：宿主仍会前置 `You are Kilo`。
- **Metis 决策**：对齐。

### `workbuddy`

- **install**：`settings.json → personalization.customPrompt`（结构化字段），并同步 `user-*/IDENTITY.md`、
  `SOUL.md`、`USER.md`。基线另有 10 个 `.tpl` 模板补丁的历史残留清理 —— **Metis 只清理，不写入**。
- **shim 不可用**：桌面客户端，双击启动绕开 PATH。
- **project 不可用**：`CODEBUDDY.md` / `AGENTS.md` 只作为首轮**用户上下文**（`<project_context>`，
  8000 字符上限），压不住身份。
- **无 CLI 启动**。
- **Metis 决策**：install 对齐；历史 `.tpl` 残留只做检测与报告，不新增写入。

### `zcode`

- **install**：`<zcode>/AGENTS.md` 托管块（官方文档写的默认 instructions 文件；桌面版闭源，无法抓包复核）。
- **CLI 启动**：基线只给 `manual` 三行（CLI 拿不到 `customSystemPrompt`）。
- **oneshot**：参数存在，但 persona 对 CLI **不生效**。
- **Metis 决策**：install 与"启动但不注入"对齐，并把"persona 对 CLI 不生效"写在界面上；
  **二进制补丁不做**。

### `antigravity`

- **完全不可注入**：`inject: false`。身份句在 `resources/bin/language_server.exe`（Go `embed.FS`）里，
  改动会让签名失效、更新被覆盖。
- **Metis 决策**：**不做**。界面里显示为"不支持"并说明原因（与基线一致：不摆点了没反应的开关）。
  基线给出的 `manual` 文案在 Metis 里改成一句"这条路线 Metis 不提供"，并说明理由。

## 4. `replace` 的降级：这五家的 replace 实际按 strict 执行

`codex`、`claude`、`grok`、`codebuddy`、`hermes` 的**启动入口没有读取宿主提示词的通道**，
所以"换身份"做不到精细替换：`replace` 实际上就是 `strict`（只留人设，宿主原来的说明也被拿掉）。

规则：

- 这五家在 `replace` 下**必须在终端/界面明确提示**："这家没有宿主提示词读取口，`replace` 按 `strict` 执行；
  想保留宿主说明请用 `append`。"
- 这条提示是产品行为的一部分，有测试（`metis launch <h> --mode replace` 的输出里必须出现）。
- `append` 在这五家是有效的，且**可能出现两份互相矛盾的身份说明** —— 界面上要如实说。

## 5. 通道能力的判定输入

一条通道能不能用，由三件事共同决定（判定只在 `metis-harness` 一处做）：

1. **这家可注入吗**（`antigravity` 不可注入 → 全通道不可用）。
2. **本机有这条路吗**（PATH 里有没有该命令；只有桌面版 → shim 不可用）。
3. **落点的层对不对 / 结构允不允许**（`claude` 的 `CLAUDE.md` 是 user 层 → install 不可用；
   `codebuddy` 没有词条 → install 不可用；`dsh` 没有 profile → install 与 slash 都不可用）。

每个不可用都必须有**一句人话的原因**，且这句话有测试钉住（`AGENTS.md` 单测契约里的一条）。

## 6. 与基线的差异

| 差异 | 说明 |
|---|---|
| 二进制补丁类路线不实现 | `claude` 的 62 字身份句、`zcode` 的 `zcode.cjs`、`antigravity` 的 `language_server.exe` |
| `workbuddy` 的 `.tpl` 模板补丁不再写入 | 只保留检测与报告 |
| 托管块标记改成 `metis` | 因此不能直接撤销基线写下的块；导入时明确报告 |
| 能力表的文案重新撰写 | 语义相同、措辞独立（不复制上游文案） |
