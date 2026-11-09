# 磁盘格式与数据布局

> 相关：[架构](ARCHITECTURE.md) · [功能对齐](PARITY.md) · [测试](TESTING.md)

> 状态：设计已定，代码未落地（M0）。本文件定义 `~/.metis/` 的**全部**落盘格式。
> 任何落盘结构都带版本字段 `v`；改动格式必须同时给出迁移路径与迁移测试。

## 1. 数据根目录

| 平台 | 默认路径 |
|---|---|
| Windows | `%USERPROFILE%\.metis\` |
| Linux / macOS | `~/.metis/` |

覆盖优先级：`METIS_HOME` 环境变量（绝对路径或相对当前目录解析）> 默认路径。

**与基线的差异**：基线用 `~/.agent-purge/`（`AGENT_PURGE_HOME`）。Metis 不读也不写那个目录；
一次性导入见 §9。

```
~/.metis/
  state.json                  唯一配置（0600，原子写）
  personas/
    <id>.md                   人设正文 + front-matter
    active.md                 当前人设的物化快照（供宿主插件读取）
  footprints.json             写入所有权账本
  backups/
    <sha256>.bak              整文件回滚用的原件（只增，不自动删）
  verification/
    latest.json               最近一次真机抓包验证报告
  armor/
    <timestamp>.json          测量矩阵快照
  hub-cache/
    <sha256>.json             远端 Hub 的 ETag 缓存（可随时删除）
  skills/
    <slug>/
      SKILL.md                远端 Hub 技能的入口文件
      .metis-hub.json         Metis 的安装清单（只删它列出的文件）
  journal/
    <yyyy-mm>.jsonl           追加式变更日志（Metis 新增，见 §8）
  shim/                       裸命令接管的包装脚本目录
  logs/
    metis.log                 轮转日志（上限见 §10）
```

**恢复 journal 不在数据目录**：它在系统临时目录（见 §6），因为它的生命周期绑定在"某次进程正在做一次交换"
上，而不是用户的长期数据。`METIS_JOURNAL_DIR` 可覆盖，主要给测试用。

## 2. `state.json`（v1）

```json
{
  "v": 1,
  "persona": "concise",
  "default": { "harness": "codex", "mode": "replace" },
  "armed": { "codex": ["install", "shim"] },
  "overrides": { "claude": { "mode": "append" } },
  "projectDirs": { "pi": ["D:\\work\\demo"] },
  "disabledHarnesses": [],
  "pausedInstalls": [],
  "prevMode": "replace",
  "activationEpoch": "0f8c…",
  "harnessEpochs": { "codex": "1a2b…" },
  "lastApply": { "…": "见 §2.2" },
  "dshProfiles": ["desktop", "headless"],
  "dshRuntime": { "desktop": { "enabled": false } },
  "piAuto": false,
  "ompAuto": false,
  "piUserLayer": false
}
```

写盘规则：

- 原子：写 `<file>.tmp-<uuid>` → `fsync` → `rename` 覆盖；失败时删临时文件，原文件保持不动。
- 权限 `0600`，内容为 `serde_json` 的两空格缩进 + 末尾换行（与基线一致，便于人工 diff）。
- **损坏即停止**：解析失败（不是对象、JSON 语法错误）→ 原字节**一个都不改**，返回 `StateCorrupt`，
  消息里带路径。不做"尽力修复"，不重命名，不备份覆盖。
- 未知字段**保留**：`load → modify → save` 必须把读到的未知键原样带回去，否则会破坏新版本写的配置。

### 2.1 迁移链

| 来源 | 触发条件 | 迁移动作 |
|---|---|---|
| `personas/.active`（旧） | `state.persona` 为空且该文件存在 | 读到 id 写入 `persona`，把原文件改名 `<name>.migrated` |
| `.defaults.json`（旧） | `state.default` 为空且该文件存在 | 读 `{harness, mode}` 生成 `default`，原文件改名 `.migrated` |
| `agent-purge` 数据目录 | 仅由 `metis import agent-purge` 显式触发 | 见 §9；**不自动**迁移 |
| `v0 → v1`（未来） | `v` 缺失或为 0 | 按字段默认值补齐并把 `v` 写成 1，迁移前的原文件复制到 `backups/` |

迁移必须是**可重复执行且幂等**的：第二次运行不得再改任何字节。迁移记录写进 `journal/`。

### 2.2 `lastApply`（`ApplyReport`）

```json
{
  "id": "uuid",
  "at": "2026-11-06T01:23:45.678Z",
  "revision": "9f2c1ab34d5e6f70",
  "persona": "concise",
  "mode": "replace",
  "ok": false,
  "partial": true,
  "runtimeConfirmed": false,
  "results": [
    { "harness": "codex", "channel": "install", "status": "written",
      "action": "installed", "when": "next-session",
      "message": "配置已写入；新会话读取", "runtimeConfirmed": false,
      "historicalCapabilityVerified": true, "persona": "concise", "mode": "replace",
      "file": "C:\\Users\\example\\.codex\\config.toml" },
    { "harness": "grok", "channel": "install", "status": "failed",
      "error": "文件在交换后被修改，保留 journal 等待处理：…" }
  ]
}
```

- `revision` = `sha256(json([persona_id, trimmed_body, mode]))[..16]`。它让"这次报告对应哪版人设"可核对。
- `status` ∈ `written` | `failed`。
- `when` ∈ `next-turn` | `next-launch` | `next-session`（由通道决定，不由 harness 决定）。
- `runtimeConfirmed` **在 apply 里恒为 `false`**；只有 verification 的持久化结果才能让它变真（见 `ARCHITECTURE.md` §5）。

## 3. 人设文件 `personas/<id>.md`

```markdown
---
id: concise
name: 简洁协作
description: 中文、先结论、修改前先读代码
harness: ["*"]
model: ["*"]
order: 10
---

用简体中文协作。先给结论，再给依据。
```

| 键 | 必填 | 说明 |
|---|---|---|
| `id` | 否（默认取文件名） | 与文件名不一致时以 front-matter 为准并给出警告 |
| `name` | 否（默认取 id） | 仅用于管理界面 |
| `description` / `desc` | 否 | `desc` 是旧键，读时兼容，写时统一成 `description` |
| `harness` / `model` | 否 | 标签，供筛选；`["*"]` 表示不限 |
| `order` | 否 | 列表排序 |

规则：

- **只有正文进入提示词**；元数据只用于管理。
- 未在编辑表单中出现的键**必须原样保留**（包括我们不认识的键）。
- `active` 是保留 id，不能作为人设名（保存时明确报错）。
- 编码：UTF-8。读入时容忍 BOM 与 CRLF，写出时统一 LF、无 BOM。
- 文件名安全：id 只允许 `[A-Za-z0-9._-]`，不得含路径分隔符、不得是 `.` / `..`、不得以 `.` 开头。
- 正文长度上限（防御性）：读写 >4 MiB 的单个文件直接报错，不做部分加载。
- `active.md`：当前生效人设的**快照**，供宿主插件在不知道 state 的情况下读取。它永远由 engine 生成，
  用户不应手工编辑；每次 apply 后重写。

## 4. 所有权账本 `footprints.json`

```json
{
  "version": 1,
  "entries": [
    {
      "id": "3f9a1c…（24 位十六进制）",
      "path": "C:\\Users\\example\\.codex\\config.toml",
      "kind": "file",
      "harness": "codex",
      "channel": "install",
      "undo": { "type": "block", "marker": "agent-purge:codex" },
      "sha256": "写入后的文件哈希",
      "createdAt": "2026-11-06T01:23:45.678Z"
    }
  ]
}
```

- `id` = `sha256(kind \0 canonical_path \0 harness \0 channel)[..24]`；
  Windows 上 `canonical_path` 转小写（路径大小写不敏感），Unix 上保持原样。
- 同一个 `(kind, path, harness, channel)` 是**同一个所有者**：重复写入是覆盖而不是追加。
- `undo.type` ∈ `block` | `whole-file` | `field` | `generated` | `path`（`path` 只登记位置，不做内容回滚）。
- 账本损坏 → 原文件保留，**停止销账**并报错。销账失败绝不能顺手删账本。

`backups/<sha256>.bak` 保存"整文件替换"前的原件。**这些文件只增不减**，只有用户在界面上显式执行
"清理备份"（带二次确认与体积提示）才会被删。

## 5. 托管块标记

写进用户既有文件的托管块必须能被**精确识别与切除**，且不改动块外一个字节。

| 文件类型 | 起始 | 结束 |
|---|---|---|
| Markdown（`AGENTS.md` / `CLAUDE.md` / `SOUL.md`） | `<!-- metis:begin -->` | `<!-- metis:end -->` |
| TOML | `# >>> metis` | `# <<< metis` |
| JSON（无法用注释） | 用结构化字段，不用块 | — |
| 脚本派生文件（shim 包装脚本） | 整文件归我们所有，用独立文件 | — |

识别规则（照抄基线的判定方式，因为这是"能不能安全撤销"的关键）：

- 只在**独立成行**（去掉行尾换行并 trim 后与标记完全相等）时认作标记；行中间出现的字符串不算。
- 切除时从 START 行切到**下一个** END 行（含）；找不到 END 就把剩下的都当作块内容。
- 切除后如果只剩空行与空白，把文件还原为"空文件"或"删除文件"取决于它原本是否存在（账本里 `beforeHash == null`）。
- **只撤销标记明确的块**：不认识的内容一律留在原地。

> 有意差异：基线用 `agent-purge` 字样的标记。Metis 用自己的标记，因此**不能**直接撤销基线写下的块。
> `metis import agent-purge` 会检测到这类块并**明确报告**，由用户决定是先跑基线的 `clean` 还是手工处理。

## 6. 恢复 journal（系统临时目录）

路径：`$TMPDIR/metis-restore/<sha256(canonical_path)>.json`（`METIS_JOURNAL_DIR` 可覆盖）。

```json
{
  "version": 2,
  "file": "C:\\Users\\example\\.codex\\config.toml",
  "pid": 12345,
  "operation": "uuid",
  "at": "2026-11-06T01:23:45.678Z",
  "backup": "base64 的原文件字节，或 null（文件原本不存在）",
  "beforeHash": "sha256 十六进制，或 null",
  "afterHash": "sha256 十六进制",
  "mode": 420
}
```

交换与恢复的算法（必须逐条实现，它是"不覆盖用户修改"的保证）：

1. 目标内容与磁盘现状**已一致** → 不写 journal，直接返回"无事可做"的 undo（幂等且不加锁）。
   *这一条是必须的*：否则常驻插件每次启动都独占一次交换，第二个会话会被挡在门外。
2. 以 `O_CREAT|O_EXCL` 创建 journal 文件。已存在时：
   - 若持有者 `version == 2` 且有 `operation`，且 pid **已不存在** → 先恢复它的交换，再重试一次；
   - 否则返回"文件存在未结束的交换（持有者 PID …）"错误，指出 journal 路径。
3. 读原文件字节（不存在记 `null`），算 `beforeHash`；写 journal 并 `fsync`。
4. 原子写新内容。
5. 恢复（undo 或崩溃回收）时：
   - 重读 journal；`operation` 不匹配 → 报"恢复所有权已改变"；
   - 当前哈希 == `afterHash` → 写回 `backup`（或删除文件）；**不做任何写入**如果当前哈希已是 `beforeHash`；
   - 当前哈希**既不是 before 也不是 after** → 报"文件在交换后被修改，保留 journal 等待处理"，**保留一切**；
   - 只有成功后才删 journal。

启动时扫描 journal 目录，回收**已死亡进程**的交换；旧版/不完整的 journal 只警告，绝不覆盖文件。

## 7. 验证与测量产物

`verification/latest.json`：

```json
{
  "v": 1,
  "at": "2026-11-06T01:23:45.678Z",
  "results": [
    { "harness": "codex", "status": "ok", "where": "developer", "chars": 1234,
      "mode": "replace",
      "requests": [ { "role": "developer", "chars": 1234, "markerCount": 1, "head": "AP_CAPTURE_…" } ] }
  ]
}
```

- `status` ∈ `ok` | `missing`（本机没装）| `failed` | `blocked`（有 CLI 但隔离通道未验证）。
- `where` ∈ `system` | `developer` | `user` | `none`。
- `requests[].head` 只保留**判定用的短标记**，绝不保存完整人设正文（见 `ARCHITECTURE.md` §11 的隐私约束）。

`armor/<timestamp>.json`：一次测量的原始矩阵（每层命中率、样本数、推荐层、被禁层与原因）。
只追加，不覆盖历史。

## 8. 变更日志 `journal/<yyyy-mm>.jsonl`

Metis 新增（基线没有独立的追加式变更日志）。每行一个 JSON 对象：

```json
{"at":"2026-11-06T01:23:45.678Z","pid":12345,"actor":"cli|gui","op":"apply.install",
 "harness":"codex","channel":"install","path":"…","action":"installed",
 "beforeHash":"…","afterHash":"…","revision":"9f2c…","ok":true}
```

用途：出事之后能回答"到底是哪一次操作改了这个文件"。按月分文件，默认保留 6 个月（超过自动删除最旧的月份，
删除动作本身也记一条日志）。**日志里不得出现人设正文、不得出现密钥。**

## 9. 从 agent-purge 导入

命令：`metis import agent-purge [--from <dir>] [--dry-run] [--yes]`。

规则：

1. 只读源目录（默认 `~/.agent-purge/`，或 `AGENT_PURGE_HOME`），**不修改、不删除**。
2. 导入内容：`personas/*.md`、`state.json` 中与 Metis 同名的字段。
3. 冲突处理：目标已存在同名 id 时**默认不覆盖**，逐条报告；`--overwrite` 才覆盖。
4. 导入后**不自动接管**：`armed` 与 `projectDirs` 默认**不导入**（因为落点标记不同，需要重新落盘）。
   只导入人设、`persona`、`default`、`overrides`。`lastApply` 不导入（那是基线的历史）。
5. 若检测到基线写下的托管块仍留在宿主文件里 → 报告文件清单与建议
   （"先运行 `agent-purge clean` 再让 Metis 接管"），并**不改动**这些文件。
6. `--dry-run` 打印将要写入的每一项，不落盘。

## 10. 日志与体积

| 目录 | 上限策略 |
|---|---|
| `journal/*.jsonl` | 保留 6 个月 |
| `logs/metis.log` | 单文件 2 MiB，保留 3 个轮转 |
| `armor/*.json` | 保留最近 20 次 |
| `backups/` | 不自动删；界面上显示总体积并提供手动清理 |

任何自动清理都必须：先写日志、只删自己创建的目录下的文件、失败不阻断主流程。

## 11. 测试要求（对应 `TESTING.md`）

- 每个格式都有 golden 文件：写入结果与仓库里的 golden 逐字节一致。
- 每个迁移都有"跑两次"的幂等测试。
- 损坏输入（截断 JSON、错误类型、非法 id、超长正文、BOM/CRLF）各有一条测试，断言原文件字节不变。
- 路径大小写：Windows 下 `C:\A\b.toml` 与 `c:\a\B.TOML` 必须判定为同一位置。

## 12. 远端 Hub 缓存 `hub-cache/<sha256>.json`

远端人设库（worldd / KK Hub）的**只读**缓存。文件名是完整请求 URL 的 sha256（十六进制），
这样列表的每一页、详情、不同 base URL 天然分开，不需要额外的索引文件。

```json
{
  "v": 1,
  "url": "https://n.kkpt.tech:8443/v1/hub/items?kind=expert&limit=20",
  "etag": "\"1dd7e2ac…\"",
  "fetchedAt": 1762000000,
  "body": "{\"items\":[…],\"nextCursor\":\"20\"}"
}
```

规则：

- **可随时删除**：缓存只是省请求与离线回退，不是真相来源；删掉后下次重新拉。
- 写入原子（`metis_fs::write_atomic`）；读不出来/版本不对/URL 对不上 → 当作没有缓存。
- `body` 原样保存响应字节（UTF-8 JSON），不做重新序列化。
- 新鲜期：列表 60 秒、详情 10 分钟；期内直接读缓存，不发请求。
- 过了新鲜期仍带 `If-None-Match` 条件请求：`304` 沿用缓存；`5xx`/网络失败回退缓存并标
  “stale”；`4xx` **不回退**（避免把“下架/参数错”伪装成旧数据）。
- 缓存里不得出现凭据：公开读不需要 token，Metis 也不保存 Hub 密码。

## 13. 远端 Hub 导入：`skills/` 与 `hub-*`

`metis.hub-import` 把 Hub 的 `expert` 写成本地人设，并按需把关联 `skill` 写到 `skills/<slug>/`。
**只写人设文件与技能目录，不自动 `apply`/`arm`。**

### 13.1 技能目录

```text
~/.metis/skills/<slug>/
  SKILL.md              ← skill.content.body
  references/…          ← skill.content.files[].path（只允许安全的相对路径）
  .metis-hub.json       ← 安装清单（最后写，作为提交点）
```

清单：

```json
{
  "v": 1,
  "slug": "wb-apiz",
  "name": "Apiz AI 网关 CLI",
  "version": "1.0.0",
  "hash": "sha256:…",
  "installedAt": 1762000000,
  "files": ["SKILL.md", "references/mcp-server.md"]
}
```

规则：

- 清单是**唯一归属凭据**：更新/移除只处理它列出的文件；目录里用户自己放的文件一律保留。
- 没有清单的目录**不覆盖、不删除**（无法证明是 Metis 写的）；CLI 需要 `--overwrite` 才覆盖安装。
- 附件路径逐段校验：拒绝 `..`、绝对路径、盘符/前缀、反斜杠、`:`、NUL、过长，
  以及会覆盖 `SKILL.md` 或 `.metis-hub.json` 的路径。
- 版本变化时，先删除旧清单里、且不在新文件集里的文件，再写新文件；清单最后写。
- `metis.hub-skill-remove` 只删清单里的文件；目录里还有用户文件时保留目录并列进 `leftovers`。

### 13.2 人设 front-matter 的 `hub-*` 键

这些键通过 `Meta.extra` 原样保留，不参与提示词：

| 键 | 说明 |
|---|---|
| `hub-url` | Hub 的 API origin（不带尾部 `/`） |
| `hub-kind` | `expert` |
| `hub-slug` / `hub-version` / `hub-hash` | 远端身份；`hash` 用于判断“是否同一份内容” |
| `hub-verified` / `hub-official` | 服务端哈希校验 / 是否官方 |
| `hub-author` | 作者账号 id（官方条目没有） |
| `hub-tags` / `hub-providers` | flow 列表；provider 是模型厂商，不是 harness |
| `hub-skills` | 已安装技能的 `slug@version` 列表 |
| `hub-model` / `hub-tools` / `hub-notes` | 远端的可选提示，仅记录，不改变 Metis 行为 |
| `hub-imported-at` | RFC3339 导入时间 |

正文结构：

```markdown
<prompt>

## 参考资料：<knowledge.name>

<knowledge.text>

## 本地技能

以下技能已随人设安装到本机。需要时先读取对应文件，再按其说明操作：

- <name>（v<version>）：`C:\Users\example\.metis\skills\<slug>\SKILL.md` — <description>
```

`includeKnowledge=false` 时省略“参考资料”小节；`skills=none` 时省略“本地技能”小节。
