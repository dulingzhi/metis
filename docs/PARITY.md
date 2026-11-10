# 功能对齐矩阵

> 相关：[执行器矩阵](HARNESS-MATRIX.md) · [架构](ARCHITECTURE.md) · [磁盘格式](DATA-FORMAT.md) · [路线图](ROADMAP.md) · [坑](PITFALLS.md)

> **唯一**的对齐基线（`AGENTS.md` 功能对齐契约）。左边是 agent-purge v0.2.19 的可观察行为，右边是 Metis 的归属。
> 状态取值：
> - `未做` —— 还没实现（默认状态，M0 全是这个）
> - `已对齐` —— 行为一致，且有测试钉住
> - `有意不同` —— 行为不同，理由写在本文件里
> - `不需要` —— Metis 的产品边界之外
> - `decision` —— 等维护者拍板，拍板前不许动手
>
> 改状态必须同时给出**证据**（测试名或实测记录）。只改这一列不算完成。
> 原始调研：`.research/agent-purge-spec.md`。

## 1. CLI 命令面

**核实方式**：每条都跑过真实 CLI（`metis <cmd> --help` 与一次实际调用），不是看代码猜的。
最后核实：2026-11-06。

| agent-purge | Metis | 状态 | 说明与证据 |
|---|---|---|---|
| `agent-purge`（无参） | `metis` | **已对齐** | 打印引导并以 0 退出。实测输出 `Metis —— 一份人设，统一管理多个 coding agent` + 命令清单 |
| `agent-purge <harness> [args]` | `metis launch <harness> [--exec]` | **已对齐** | 默认只打印将执行的命令，`--exec` 才真启动（比基线更安全）。证据：`launch_dry_run_shows_the_command_without_executing` |
| `--help` / `-h` | `metis --help` | **已对齐** | clap 生成，早退不读写状态 |
| `use <id>` | `metis use <id>` | **已对齐** | 换全局人设 |
| `use <id> --for <h>` | `metis use <id> --for <h>` | **已对齐** | 分家例外。实测输出 `codex 改用人设 alpha` |
| `use`（无 id） | `metis use` | **已对齐** | 打印当前人设 + 全部例外。证据：`use_without_an_id_prints_the_current_persona`、`use_without_an_id_lists_overrides` |
| `sync [--failed]` | `metis sync [--failed]` | **已对齐** | 等价于 `apply`（不换人设）。证据：`sync_applies_without_changing_the_persona`、`sync_failed_flag_is_accepted` |
| `mode <m>` / `mode <m> --for <h>` | `metis mode …` | **已对齐** | `strict` / `replace` / `append` / `off`；`--for` 实测可用 |
| `pause` | `metis pause` | **已对齐** | 全局 off + **立即撤下静态内容**；保留通道选择。证据：`pausing_works_without_any_persona`、`mode_and_power_round_trip_through_the_cli` |
| `resume` | `metis resume` | **已对齐** | 回到 `prevMode` 与原范围 |
| `install [h…] [--force]` | `metis install [h…]` | **已对齐** | 只开 install 通道；**校验"已注册"而非只校验格式**。证据：`install_enables_only_the_install_channel`、`install_rejects_an_unregistered_harness_with_code_2` |
| `uninstall [h…] [--force]` | `metis uninstall <h>` | **已对齐** | 撤该家全部通道并删掉已写入内容。证据：`uninstall_clears_the_armed_list` |
| `arm [h…] [--all] [--channel…]` | `metis arm <h> --channel <c>…` | **部分** | 通道选择已对齐（无 `--channel` 时默认 shim+slash+install）；`--all` 未实现（实测 `unexpected argument '--all'`） |
| `register` / `注册` | — | **有意不同** | 中文别名不迁移；`arm` 是唯一入口，减少同义词 |
| `disarm [h…]` | `metis disarm <h>` | **已对齐** | 撤全部通道**并把已写入的内容撤下来**。证据：`arm_then_disarm_updates_state` |
| `slash [status\|on\|off\|h…]` | `metis slash status\|on\|off [h…]` | **已对齐** | 四条通道现在都有 CLI 入口。证据：`slash_status_lists_harnesses`、`slash_on_off_round_trips`、`slash_rejects_an_unknown_action_with_code_2` |
| `project [on\|off\|status] [h…] [--dir]` | `metis project list\|add\|remove` | **已对齐** | 目录以绝对路径登记、摘除时先撤盘。证据：`remove_takes_the_block_off_disk` + 真机端到端 |
| `status` | `metis status` | **已对齐** | state 全量 + 路径；`--json` 机器可读 |
| `ls` | `metis ls` | **已对齐** | 人设列表，`*` 标当前 |
| — | `metis show <id>` | **有意不同（新增）** | 读一份人设的完整正文 + 元数据（`persona.show`）；GUI 的详情浮层走同一条命令。证据：`show_prints_the_full_persona` |
| `pull [src]` | — | **不需要**（原 decision 1） | 已拍板：默认不带源；通用导入由 `metis import <dir\|owner/repo>` 覆盖 |
| `persona import <dir> [--no-overwrite]` | `metis import <dir\|owner/repo>` | **已对齐** | 本地目录 + GitHub 两种来源；按内容去重（`identical`）；`--overwrite` 控制覆盖。GitHub 走 **codeload tarball（1 个请求）**，不打 API |
| `import agent-purge` | `metis import-agent-purge` | **有意不同（新增）** | 只读导入；**不导入 armed**（标记不同）；报告上游残留 |
| `doctor` / `detect` | `metis doctor` | **已对齐（本机）** | 15 家真实探测：2 家有可执行文件 / 10 家只有配置目录 / 3 家未安装 |
| `matrix` | `metis harnesses` / `--json` | **已对齐** | 15 家 × 四通道的可用性与原因 |
| `armor matrix\|plan\|preflight\|check\|run\|judge` | `metis armor plan\|preflight\|record\|judge\|adopt` | **已对齐（语义等价）** | `run` 由用户跑真实评测后 `record` 记入；判层由 `verify` 承担 |
| `update` / `upgrade [--download] [--yes]` | `metis update` | **部分** | Release 检查已对齐（真版本比较、可注入 fetch）；**就地替换 exe 未做**（Windows 运行中的 exe 不能覆盖） |
| `web [--port] …` | — | **不需要** | 没有本地 Web 服务；等价能力是 `metis`（无参数）打开的窗口 |
| `--serve`（隐藏） | — | **不需要** | 同上 |
| `clean [--dry-run]` | `metis clean [--dry-run]` | **已对齐** | 按账本撤回，保留人设与 state。证据：`clean::tests::*`（10 条）+ 真机端到端 |
| `self-uninstall [--yes] [--dry-run]` | `metis self-uninstall [--yes] [--dry-run]` | **已对齐** | 无 `--yes`/`--dry-run` 时以退出码 2 拒绝。证据：`self_uninstall_refuses_without_confirmation` |
| `verify [h…]` | `metis verify [h…]` | **已对齐（代码）/ 真机未验证** | 抓包判定已实现并按 `blocked`/`no-request` 如实报告；**真机一次 `ok` 都没拿到**（本机 6 家宿主都不接受 base-url 覆盖） |
| — | `metis report` | **有意不同（新增）** | 单独读验证报告，不必重跑 |
| — | `metis backups usage\|prune` | **有意不同（新增）** | 备份体积管理；默认只预览 |
| — | `metis journal [--month]` / `journal-prune` | **有意不同（新增）** | 追加式变更日志 + 保留期裁剪 |
| — | `metis i18n [--all]` | **有意不同（新增）** | 文案表自检 |
| — | `metis mode`（含 `off`） | **已对齐** | `off` 既是单家模式，也参与总电源语义 |
| — | `metis`（无参数） | **有意不同（新增）** | egui 桌面控制台，替代 Web 控制台；与 CLI 同一个 exe |
| — | `metis --json` | **有意不同（新增）** | 所有读命令支持机器可读输出 |

### 1.0 小结（CLI 面）

- **已对齐**：20 项
- **有意不同 / 不需要 / 新增**：14 项
- **部分**：2 项（`arm --all`、`update` 的就地替换）
- **真缺**：3 项 —— `use`（无 id）、`sync`、`install`；外加 `slash` 的 CLI 入口
### 1.1 全局旗标

| agent-purge | Metis | 状态 | 备注 |
|---|---|---|---|
| `--` 之后全透传 | **部分** | `metis launch` / `ask` 支持；未做通配透传到任意子命令 |
| `--mode <m>` | `metis launch --mode <m>` | **已对齐** | 只作用于本次启动，不改持久状态 |
| `--persona <id>` | — | **不需要** | 用 `metis use <id>` 先选人设 |
| `--ask "<prompt>"` | `metis ask <harness> <prompt>` | **已对齐** | 位置参数而非旗标；默认只打印，`--exec` 才真跑 |
| `--dry-run` | `metis clean --dry-run` 等 | **部分** | 破坏性命令有；`apply` / `arm` 没有 |
| `--cwd <dir>` | — | **未做** | 启动宿主时指定工作目录 |
| `--auto on\|off`（仅 pi/omp） | — | **部分** | 状态字段 `piAuto`/`ompAuto` 已对齐，缺 CLI 开关 |
| `--layer sys\|user`（仅 pi） | — | **不需要**（decision 2） | 不提供：user 层压不住身份 |
| `--profile <p>`（dsh/omp） | — | **部分** | 多 profile 展开已实现，缺 CLI 选择器 |
| `--no-extensions`（omp） | — | **未做** | |
| `--force` | — | **未做** | 覆盖外部修改时的强制开关 |
| `--no-color` | — | **不需要** | 输出无 ANSI 颜色码；`--json` 才是机器可读入口 |
| `--home <DIR>` | `metis --home <DIR> <cmd>` | **已对齐** | **必须写在子命令之前**（clap global 参数行为） |
| `--lang <LANG>` | `metis --lang <LANG> <cmd>` | **已对齐** | 同上；认 `zh`/`zh-Hans`/`en`/`en-US`，非法值退出码 2 |

### 1.2 退出码

| 场景 | agent-purge | Metis |
|---|---|---|
| 成功 / `--help` / `--ask` 完成 | 0 | 0（对齐） |
| 逐项失败、`clean` 有残留、`verify` 有 mismatch | 1 | 1（对齐） |
| 语义错误（未知 harness、非法 `--mode`、缺 `--ask` 值、未确认删除） | 2 | 2（对齐） |
| 启动宿主 | 子进程退出码 | 子进程退出码（对齐） |

## 2. 状态字段

`~/.metis/state.json` 的字段与基线同名同义（细节见 `DATA-FORMAT.md` §2）。
**核实方式**：跑一遍完整操作链后直接读 `state.json`。最后核实：2026-11-06。

| 字段 | 状态 | 备注 |
|---|---|---|
| `v` | 有意不同 | 基线是 `1` 且无迁移链；Metis 保留 `1` 并新增幂等迁移链 |
| `persona` | **已对齐** | 实测写入 `"persona": "a"` |
| `default.{harness,mode}` | **已对齐** | 实测 `"default": {"mode": "append"}`；`mode` 同时是总电源 |
| `armed` | **已对齐** | 实测 `"armed": {"codex": ["install"]}` |
| `overrides` | **已对齐** | 分家例外；`follow` = 删除该键。证据：`state::tests::*` |
| `projectDirs` | **已对齐** | 绝对路径登记。证据：`project::tests::*` + `metis project` CLI |
| `disabledHarnesses` | **已对齐** | 整家停用。证据：`activation::tests::harness_off_and_disabled_are_distinct_reasons` |
| `pausedInstalls` | **已对齐** | 暂停期间选择但未落盘 |
| `prevMode` | **已对齐** | 恢复用。证据：`state::tests::power_off_records_prev_mode_and_restores_it` |
| `activationEpoch` / `harnessEpochs` | **已对齐** | 实测 `"activationEpoch": "initial"`；off→on 重新生成。证据：`harness_off_then_on_regenerates_its_epoch` |
| `lastApply` | **已对齐** | 实测含 `at`/`id`/`mode`/`ok`/`partial`/`persona`/`results`/`revision`/`runtimeConfirmed` |
| `dshProfiles` | **已对齐** | DSH profile 白名单。证据：`slash::tests::profile_dirs_*` |
| `dshRuntime` | **未做** | `/purge` 运行时开关（宿主侧跳变），优先级低 |
| `piAuto` / `ompAuto` | **已对齐** | 扩展自动注入开关，字段与序列化均有测试 |
| `piUserLayer` | **不需要**（原 decision 2 已拍板） | 维护者决定不提供 `--layer user`：user 层压不住身份，不作为能力提供 |
| `runtime/<token>.json`（启动租约） | **有意不同** | Metis 用 `fs::lock` 的控制锁（`O_EXCL` + pid + token + reentrant + RAII）实现同等保证，不落单独的租约文件 |
| 未知字段保留 | **有意不同（新增）** | Metis 明确要求 `load → save` 保留未知键（基线是整体覆盖）。证据：`unknown_fields_survive_a_load_modify_save_cycle` |
## 3. 激活语义

| 判定 | 状态 |
|---|---|
| 九步短路顺序（missing-state → global-pause → harness-off → harness-disabled → session-off → profile-off → invalid-mode → not-enabled → on） | **已对齐**（双端） | Rust：`activation::tests::*`；JS：`runtime.behaviour.mjs` 的 12 条断言 |
| 许可来源：显式启动 > install 已武装 > slash 已武装且（会话命令 或 `Auto`） | **已对齐**（双端） | Rust 侧 `core::activation`；宿主侧 `runtime.mjs` 的同名函数。两边各有测试 |
| `blockedBy` 的取值集合 | **已对齐** | 19 个值，闭合枚举 + `ALL_CODES`；证据：`core::error::tests::all_codes_match_the_enum_variants` |
| 环境变量/token 不能直接授予许可 | **已对齐** | 只有 `launch: true` / 已武装通道 / 会话内命令 / Auto 四种来源；没有环境变量入口 |
| `off` 与"配置已写"分开报告 | **已对齐** | `ApplyReport.runtimeConfirmed` 恒为 `false`；CLI 固定打印"配置写入不等于当前会话已生效" |

## 4. 通道语义

| 行为 | 状态 | 备注 |
|---|---|---|
| `install` 的落点、层、托管块标记 | 有意不同 | 标记从 `agent-purge` 改成 `metis`；落点路径一致 |
| `shim` 的包装脚本与 PATH 前置 | **已对齐** | Windows `.cmd` / Unix shell；内容不内嵌人设，转发到 `metis launch <h> --exec`（**必须带 `--exec`**，否则只打印计划）；`launch --exec` 继承终端并回传退出码；CLI 模式先 `AttachConsole(ATTACH_PARENT_PROCESS)`，宿主在当前控制台运行而不是另开窗口；解析宿主时跳过 shim 目录避免递归；shim 目录不在 PATH 时 `apply`/GUI 给出提示。证据：`shim::tests::script_forwards_to_metis_and_passes_args`、`install_then_revert_cleans_up`、`which::tests::same_directory_is_recognised`；端到端假 harness 验证 stdin / 退出码 / 同一控制台 |
| `slash` 的扩展产物 | **已对齐**（6 家） | 语义等价，产物由 Metis 自己实现；数据目录指向 `~/.metis`。证据：`slash::tests::runtime_behaviour_tests_pass`（34 条 JS 行为断言） |
| `project` 的托管块 | **已对齐** | 支持 `AGENTS.md` / `CLAUDE.md` 等（按 `project_file`）；绝对路径登记、摘除先撤盘。证据：`project::tests::apply_then_revert_restores_bytes` + 真机端到端 |
| 先发布 `off` 再清理静态内容 | **已对齐** | 证据：`apply::tests::apply_publishes_off_before_touching_channels` |
| `onlyFailed` 重试 | **部分** | 已实现，但匹配键是 `harness + channel`，不是基线那样按 file 精确匹配 |
| `mode == off` 时清理旧版本遗留 | **已对齐** | 只在本账本登记过的范围内清理 |
| dsh：开 slash 时级联安装 install | **已对齐** | `dsh` 的 `slash` 依赖 `install`（`/purge` 只是开关，注入靠常驻插件）；`HARNESS-MATRIX.md` 已写明 |
| dsh：撤 install 时一并撤 slash | **已对齐** | `disarm` 的 `PerProfileDir` 分支先摘 `cordis.patch.yml` 挂载块，再删产物；证据 `dsh_install_mounts_the_cordis_patch_block` |
| `arm --all` / `disarm --all` 同时操作总电源 | **未做** | 实测报 unexpected argument |

## 5. Web API → GUI / CLI 归属

基线有一个本地 Web 服务（`agent-purge web`）和一套 REST API。Metis **没有 Web 服务**，
所有能力走 CLI 或 egui 窗口。下表的"Metis 实现"一列是**实际存在**的入口。

| 基线 API | 能力 | Metis 实现 | 状态 |
|---|---|---|---|
| `GET /api/state` | 全量状态 + 人设 + 能力 | `metis status --json` / `metis harnesses --json` | **已对齐（换形态）** |
| `POST /api/apply` | 表单整体提交并应用 | `metis use` + `metis apply`（GUI：控制台「应用变更」） | **已对齐（换形态）** |
| `POST /api/power` | 总电源 | `metis pause` / `resume` | **已对齐（换形态）** |
| `POST /api/sync` | 重新同步（可只失败项） | `metis apply --failed` | **已对齐（换形态）** |
| `POST /api/defaults` | 保存默认执行器 | `metis use <id> --for <h>` / `state.default` | **已对齐（换形态）** |
| `POST /api/pi-auto` / `omp-auto` | 扩展自动注入开关 | 状态字段 `piAuto` / `ompAuto`（无独立子命令） | **部分**（字段已对齐，缺 CLI 开关） |
| `POST /api/dsh-profile` | DSH profile 白名单 | 状态字段 `dshProfiles`（无独立子命令） | **部分**（同上） |
| `POST /api/persona` | 新建/保存人设 | `metis persona`（GUI：人设库） | **已对齐（换形态）** |
| `DELETE /api/persona?id=` | 删除人设 | `metis rm <id>` | **已对齐（换形态）** |
| `POST /api/persona/import` | 批量导入 | `metis import <dir\|owner/repo>` | **已对齐（换形态）** |
| `POST /api/harness` | 分家配置 | `metis use/mode … --for <h>` | **已对齐（换形态）** |
| `POST /api/use` | 换人设 | `metis use <id>` | **已对齐（换形态）** |
| `POST /api/install` | install 通道开关 | `metis arm <h> --channel install` | **已对齐（换形态）**（专门的 `metis install` 待补） |
| `POST /api/arm` / `arm-all` | 通道开关 / 全开 | `metis arm` / `disarm` | **部分**（`--all` 缺） |
| `POST /api/clean` | 撤回所有写入 | `metis clean` | **已对齐（换形态）** |
| `GET /api/update` | 更新检查 | `metis update` | **已对齐（换形态）** |
| `POST /api/update-upgrade` | 就地升级 | — | **未做**（Windows 运行中的 exe 不能覆盖） |
| `POST /api/verify` | 真机验证 | `metis verify` | **已对齐（换形态）** |
| `GET /api/verification` | 验证报告 | `metis report` | **已对齐（换形态）** |
| `GET /api/armor` | 测量矩阵 | `metis armor judge` | **已对齐（换形态）** |

**结论**：20 项里 17 项"已对齐（换形态）"，2 项部分（`piAuto`/`dshProfiles` 缺独立开关、`arm --all`），
1 项未做（就地升级）。

## 6. Web 控制台的界面能力 → egui

| 基线 Web 界面 | Metis GUI | 状态 |
|---|---|---|
| 控制台：执行器列表 + 通道开关 + 详情展开 | 控制台屏：卡片 + 状态胶囊 + 四通道开关 + 展开详情 | **已对齐** |
| 控制台：总电源 + 撤销全部接管 | 顶部状态条（点击切换）+ 诊断屏「撤回全部接管」 | **已对齐** |
| 控制台：默认命令启动设置 | 人设下拉 + 三个模式选择瓦片 | **已对齐** |
| 人设库：列表/搜索/编辑/字数/新建 | 人设库屏：列表 + 搜索框（`Ctrl+K` 直达）+ 当前标记 | **部分**（编辑/新建只在 CLI，屏内无编辑器） |
| 人设库：删除确认 | — | **未做**（GUI 无删除入口） |
| 导入对话框（本地目录 / GitHub，覆盖开关） | **人设库屏「从 GitHub 导入」卡片**（填 `owner/repo`、预览/导入）+ 关于屏「从 agent-purge 导入」 | **已对齐** |
| 更新对话框（版本、变更日志、升级） | — | **未做** |
| 诊断：最近一次应用结果 + 重试失败项 | 诊断屏：诊断信息 + 实测验证 + 撤回接管 | **部分**（无"重试失败项"按钮） |
| 诊断：真实请求验证结果 | 诊断屏「实测验证」按钮（结果进提示条） | **部分**（无历史结果列表） |
| 顶栏：连接状态、版本徽标、刷新 | 顶栏：就绪状态 + 版本 + 刷新 + 命令面板入口 | **已对齐** |
| 「配置写入 ≠ 当前会话生效」提示 | 控制台 `WRITE_NOT_EFFECT` 文案（常驻） | **已对齐** |
| 减少动效支持 | 顶栏「动效：开/关」+ 环境变量优先 | **已对齐** |

**另加的（基线没有）**：`Ctrl+K` 命令面板、折叠侧栏、深浅双主题、快照基线、
无障碍语义（侧栏四项 `role=button`）。
## 7. 明确不做 / 不需要

| 项 | 理由 |
|---|---|
| 二进制补丁路线（`claude` 的 62 字身份句、`zcode` 的 `cjs`、`antigravity` 的 exe） | 破坏第三方签名与自动更新，风险高于收益；在界面中显示为"不支持"并说明原因 |
| `workbuddy` 的 10 个 `.tpl` 模板补丁 | 同上；Metis 只做检测与报告 |
| `magpie-plugin` | 第三方网关注解，与产品边界无关 |
| `bench/`（22 个评测脚本的原始形态） | 评测能力以 `metis armor` 重新实现；原始脚本不迁移 |
| Playwright 浏览器测试 | 没有浏览器端 |
| npm 打包 / `npx` / 薄包 | 单可执行文件；不需要运行时 |
| 页面心跳、退出倒计时、CSRF、同源校验、端口冲突处理 | 不监听端口 |
| `--detach` 后台常驻 | 没有常驻服务 |

## 8. 已拍板的决策（原 decision，2026-11-06 维护者拍板）

原 7 条开放决策已全部拍板。**结论与当时的倾向一致**，逐条记录在此以免后人重新讨论。

| # | 决策 | 拍板结果 | 理由 |
|---|---|---|---|
| 1 | `pull` 的默认人设源 | **默认不带源**，`pull` 必须显式给出 `owner/repo` | 不让 Metis 悄悄指向别人的仓库 |
| 2 | `pi` 的 `--layer user` | **不提供 `--layer`**，在 `HARNESS-MATRIX.md` 说明原因 | user 层压不住身份，不作为能力提供 |
| 3 | 二进制补丁路线 | **只在 `doctor` 里报告**"这家有补丁路线，Metis 不提供"，不给操作 | 报告是诚实的，操作会引入不可逆风险 |
| 4 | `state.json` 的 `v` | **保持 `v: 1`**，只在真正改结构时升版 | 字段兼容，没必要为了"看起来新"升版 |
| 5 | 品牌图标 | **不内置官方图标**，用名称 + 通用图标 | 版权与维护成本 |
| 6 | `en` 文案 | **与 `zh-Hans` 一起做**（已完成，见 `crates/core/assets/i18n/`） | 引擎文案必须走同一套键，事后补代价更高 |
| 7 | **托管块标记与账本哈希是否与基线字节兼容** | **不兼容**（维护者 2026-11-06 明确拍板） | 见下 |

### 决策 7 的落地细节（最重要的一条）

**结论：不与 agent-purge 字节兼容。** 走"一次性导入 + 明确报告"路线。

具体含义：

- **托管块标记**：用 Metis 自己的（`# >>> metis` / `<!-- metis:begin -->` / `# >>> metis:<plugin>`）。
  Metis **不认、也不动**基线留下的 `# >>> agent-purge` 系列块。
- **stash 文件名与账本条目 id**：用 Metis 自己的算法，不与上游 `sha256(...)[..24]` 兼容。
- **交换 journal**：Metis 自己的格式（版本号独立演进）。
- **`state.json`**：字段名保持与基线**语义对应**，但位置在 `~/.metis/`，不与 `~/.agent-purge/` 共用。
- **检测到基线残留时**：`metis import agent-purge` 与 `metis doctor` **报告文件清单**，
  并建议"先运行基线的 `clean`，再让 Metis 接管"。Metis 不替用户猜那些块的归属。

**收益**：彻底贯彻 `AGENTS.md` 的独立实现原则；不把上游的历史命名永久固化进 Metis。
**代价**：不能直接撤销基线的写入，用户必须先在基线里 `clean` 一次。
**为什么接受这个代价**：数据目录本来就独立（`~/.metis/`），半兼容反而会让用户误以为"能直接接管"。
"看起来能接管但实际撤不干净"比"明确要求先 clean"危险得多。

### 这条决策如何被测试钉住

- `channel` 的标记常量集中在 `crates/channel/src/blocks.rs` 的 `MARKER_*` 里，
  有测试断言它**不含** `agent-purge` 字样：`markers_never_mention_the_upstream_brand`。
- `import agent-purge` 检测到上游块时**只报告不修改**：`detects_upstream_blocks_without_touching_them`。
## 9. 每一条"已对齐"需要什么证据

- 有测试：给出测试名（例如 `channel::install::tests::apply_is_idempotent`）。
- 只测了磁盘、没测真机：明确写"仅磁盘"，并在 `STATUS.md` 的"未验证"里列一行。
- 真机验证过：写清时间、harness 版本、看到的证据（抓到的角色与命中）。
- **不允许**把"代码看起来对"当证据。

## 10. Metis 独有扩展（不属于 agent-purge 基线）

这些能力没有对应的上游行为，不参与"已对齐 / 未做"的统计：

| 扩展 | 行为 | 证据 |
|---|---|---|
| 远端 Hub 浏览 / 导入（worldd / KK Hub） | `metis hub list\|show\|import\|skill`；GUI 人设库屏的远端库卡片；导入只写本地人设与 `~/.metis/skills/<slug>/`，**不自动 apply**；未校验条目必须二次确认 | `hub::tests::*`、`hub_import::tests::*`、`hub_skills::tests::*`、`commands_hub::tests::*`、`apps/metis/tests/cli.rs`、UI 无头测试 + 2026-11-09 真机（见 `STATUS.md`、`TESTING.md`） |
| 远端来源与技能清单 | front-matter `hub-*`；`.metis-hub.json` 是技能目录的唯一归属凭据 | `DATA-FORMAT.md` §12–13；`PITFALLS.md` G80–G86 |
