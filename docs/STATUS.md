# 当前状态

> **唯一**回答"现在真的能做什么"的地方。规则（`AGENTS.md` 自我迭代协议第 2 条）：
> 每条"能做什么"必须能指出**代码位置**与**测试名**；指不出来就写"未实现"。
> 不允许把计划写成已完成。

最后更新：M0–M5 的主要闭环已落地（见下）。

## 一句话

**Metis 现在能用了**：`metis` CLI 可以管人设、改模式、武装通道、应用、暂停/恢复，并且**写入可逆**
（逐字节回滚、账本、外部修改保护）。桌面 GUI 已能开窗并显示四个屏幕。`cargo test --workspace`
**335 个测试全绿**，`clippy -D warnings` 与全部 xtask 门禁通过。

## 已经能用（有代码、有测试）

| 能力 | 代码位置 | 证据 |
|---|---|---|
| 15 家执行器的静态事实（探测规则、落点、层级、热切） | `crates/harness/src/registry.rs`（`LazyLock<Vec<Spec>>` 表） | `registry::tests::*`（15 个用例，含"每个 why 都有原因"） |
| 能力表（`ok` + `why`，user 层不算能力） | `crates/harness/src/registry.rs::caps_of` | `claude_install_is_unavailable_because_it_is_a_user_layer` 等 |
| 激活解析：**九步短路**、全局暂停压过例外、临时覆盖不授予许可 | `crates/core/src/activation.rs` | `activation::tests::*`（14 个用例） |
| 原子写、控制锁（可重入 + 死锁回收 + token 释放） | `crates/fs/src/lib.rs`、`crates/fs/src/lock.rs` | `write_atomic_*`、`lock::tests::*`（7 个） |
| 所有权账本（幂等、所有者匹配、未知 undo 类型硬报错） | `crates/fs/src/footprint.rs` | `footprint::tests::*`（13 个） |
| 文件交换 journal + 崩溃回收（外部修改拒绝覆盖） | `crates/fs/src/swap.rs` | `swap::tests::*`（12 个） |
| 托管块（Markdown/TOML 标记、块外一字节不改、CRLF 保持） | `crates/channel/src/blocks.rs` | `blocks::tests::*`（14 个） |
| `install` 通道（托管块 + JSON 字段 + 整文件备份回滚） | `crates/channel/src/install.rs` | `apply_then_revert_restores_bytes_exactly`、`json_field_round_trips` |
| `shim` 通道（包装脚本，不内嵌人设） | `crates/channel/src/shim.rs` | `script_never_embeds_the_persona_body` 等 |
| `project` 通道（绝对路径登记、逐字节还原） | `crates/channel/src/project.rs` | `apply_then_revert_restores_bytes` 等 |
| **`slash` 通道**（6 家的宿主 JS 产物 + 运行时 + 幂等 + 可撤销） | `crates/channel/src/slash.rs` + `assets/host/*.mjs` | **`runtime_behaviour_tests_pass`**（34 条 JS 行为断言，跑真实产物）、`rendered_output_is_syntactically_valid_javascript`（node --check）、`install_is_idempotent`、`install_then_revert_cleans_up` |
| **DSH 的 `cordis.patch.yml` 挂载**（否则宿主不加载插件） | `crates/channel/src/dsh_patch.rs` | `mounted_patch_is_valid_yaml_with_an_insert_entry`（node 校验结构）、`mount_then_unmount_round_trips_on_disk`（逐字节还原）、engine 层 `dsh_install_mounts_the_cordis_patch_block` |
| 人设库（front-matter、未知键保留、`active` 保留字、4 MiB 上限） | `crates/persona/src/*.rs` | `persona::tests::*`（34 个） |
| `state.json`（原子写、损坏保留原字节、未知字段保留、epoch 语义） | `crates/engine/src/state.rs` | `state::tests::*`（16 个） |
| apply 编排（先发布 off、逐格报告、`runtimeConfirmed` 恒 false） | `crates/engine/src/apply.rs` | `apply::tests::*`（9 个） |
| 命令注册表（一切皆命令，CLI 与 GUI 同一实现） | `crates/engine/src/command.rs` | `command::tests::*`（16 个） |
| **`clean` / `uninstall` / `self-uninstall`**（按账本撤回，先过安全闸门） | `crates/engine/src/clean.rs` | `clean::tests::*`（10 个，含"危险数据根拒绝"、"有残留就保留目录"）、`self_uninstall_refuses_without_confirmation` |
| **`import-agent-purge`**（只读导入，不导入 armed） | `crates/engine/src/import.rs` | `import::tests::*`（11 个，含"源文件一个字节都不变"、"active.md 不当人设"） |
| **UI 自动化测试** | `scripts/ui-test.ps1` | 12 项：面板开关/Esc/Ctrl+K、侧栏四项 role=button、三次切屏、主题切换、面板 Enter 执行。**实测能抓 bug**：注释掉 Esc 修复 → 立刻报红 |
| **快照基线（P5）** | `crates/ui-egui/tests/snapshot.rs`、`tests/snapshots/*.png` | 5 张基线；3 条无 GPU 测试（用例覆盖全屏、深浅两主题、fixture 确定性）+ 2 条 `#[ignore]` 渲染测试。**实测**：改 2px 内边距 → 5 张全红；恢复 → 全绿 |
| **命令面板（P4）** | `crates/ui-egui/src/palette.rs` | 16 条：条目完整性与图标、子序列匹配、过滤、`Enter` 执行、`Esc` 关闭、`↑↓` 循环、两主题渲染、空结果。覆盖率 98.8% |
| **四个屏幕（P3）** | `crates/ui-egui/src/screens/mod.rs` | 8 条：四屏渲染、深色渲染、三模式瓦片、应用按钮启用逻辑、空状态、搜索过滤、busy 态、关于屏 |
| **侧栏（P2）** | `crates/ui-egui/src/sidebar.rs` | 6 条：图标映射、窄窗自动折叠、手动折叠优先、两态渲染、四屏选中态。**真机验证** 1300px 展开 / 820px 自动折叠 |
| **备份体积管理**（先算再删，被引用的一律保留） | `crates/engine/src/backups.rs`、`commands_backups.rs` | `backups::tests::*`（10 条）、`commands_backups::tests::*`（5 条）。关键：`prune_removes_only_orphans`、`everything_referenced_means_nothing_is_prunable`、CLI 默认预览 |
| **发布（v0.1.0）** | `.github/workflows/release.yml`、`scripts/package.ps1`、`scripts/gen-icon.py` | tag 触发；校验 tag ≡ Cargo.toml ≡ 二进制版本；三平台产物 + SHA256。**实测**：下载 → 校验和匹配 → 解压 → `metis 0.1.0` 可运行 |
| **执行器卡片（自适应网格）** | `crates/ui-egui/src/screens/mod.rs` | 6 条：`grid_columns` 三档宽度、卡片固定高度、首屏预算、浮层选中才画/未知 id 不画/不改变页图元数。**真机验证**：4 列流式换行、下拉不超框、详情浮层对齐 |
| **GUI GitHub 导入** | `crates/ui-egui/src/{actions,app,screens}.rs` | 5 条：默认源、空地址拦截、命令映射、后台任务、标签。**真机验证**：点「导入」→ 5 份人设出现在列表，显示「导入完成」 |
| **GitHub 人设源** | `crates/engine/src/tarball.rs`、`persona_import.rs` | 20 条：tarball 解包（前缀剥离/文档跳过/非 UTF-8/坏包/目录）、单请求验证、无索引兼容、404/网络/坏 JSON。**真机验证**：从 `dulingzhi/metis-personas` 导入 5 份，1.3s |
| **CLI 补齐（4 条）** | `apps/metis/src/main.rs` | 10 条 e2e：`use` 无 id 打印/列例外、`sync` 不改人设、`install` 只开一条通道、`install nope` 退出 2、`slash status/on/off` 往返 |
| **`project` 通道 CLI**（登记 / 摘除 / 列出，摘除先撤盘） | `crates/engine/src/commands_project.rs` | `commands_project::tests::*`（8 条，含 `remove_takes_the_block_off_disk`、`list_flags_a_directory_that_was_deleted`）。**真机端到端验证**：add → apply 写入 `<!-- metis:begin -->` 块 → remove 后标记消失 |
| **slash golden 测试** | `crates/channel/src/slash.rs`（`golden_tests`） | 5 份 golden（extension / opencode v1 / v2 / dsh-plugin / runtime）；**实测改模板会让它变红**；`cargo xtask golden` 重建 |
| **无障碍基础** | `crates/ui-egui/src/{theme,app}.rs` | `focus_is_visibly_wider_than_a_normal_border`、`applying_reduced_motion_sets_animation_to_zero`、`ctrl_number_switches_screens`（Alt）、`shortcuts_are_ignored_while_a_widget_has_focus` |
| **`armor`**（实测 → 策略 → 矩阵，不猜） | `crates/engine/src/armor.rs`、`crates/engine/src/commands_armor.rs` | `armor::tests::*`（18 条）、`store_tests`（5 条）、`commands_armor::tests::*`（10 条）。关键：`without_measurements_the_plan_says_measure_first`、`the_user_layer_is_never_recommended_even_at_100_percent`、`outcome_to_bench_only_counts_ok_as_a_hit` |
| **GUI 三层测试脚本** | `scripts/gui-test.ps1` | 逻辑层（无头渲染 + I9）→ 装配层（启动守卫 / 窗口参数 / `--version`）→ 真机层（三种尺寸截图 + 深色与浅色 MD5 必须不同）。实测三层全过 |
| **i18n**（TSV 双表 + 缺键门禁 + `--lang`） | `crates/core/src/i18n.rs`、`crates/core/assets/i18n/*.tsv` | `i18n::tests::*`（10 条）、`every_command_title_key_is_translated`、`cargo xtask i18n`。**实测确认两张表都编进了二进制** |
| **键盘快捷键** | `crates/ui-egui/src/app.rs`（`handle_shortcuts`） | `ctrl_number_switches_screens`、`ctrl_l_toggles_the_theme`、`a_bare_key_does_not_trigger_a_shortcut`、`shortcuts_are_ignored_while_a_widget_has_focus` |
| **CLI/GUI 一致性（I9）** | `crates/ui-egui/tests/consistency.rs` | 9 条：id 唯一性、CLI/GUI 命令映射、两条入口输出逐字段相同、只读命令不改状态 |
| **深色主题** | `crates/ui-egui/src/theme.rs`（`Tokens::DARK`） | `both_themes_meet_wcag_aa_for_body_text`（≥4.5:1）、`dark_is_actually_dark_and_light_is_actually_light`、`install_with_dark_switches_egui_visuals`。**真机截图确认** |
| **探测并行化** | `crates/harness/src/detect.rs` | `parallel_detection_keeps_the_registry_order`、`parallel_detection_is_faster_than_serial` |
| **覆盖率门禁真正生效** | `xtask/src/main.rs`（`coverage` 任务） | 实测行 **90.03%**、函数 **87.76%**；`cargo xtask coverage` 退出 0 |
| **补救建议全覆盖**（`cargo xtask remediation` 强制） | `xtask/src/remediation.rs` | `remediation::tests::*`（7 个，含 `the_real_workspace_is_clean`）；补齐了 19 处缺失 |
| **人设导入**（本地目录 + GitHub，按内容去重） | `crates/engine/src/persona_import.rs` | `persona_import::tests::*`（14 个，含"导入幂等报 identical"、"源目录不变"、"拒绝路径穿越"） |
| **`journal`**（追加日志、按月、保留期裁剪、删除也留痕） | `crates/engine/src/journal.rs` | `journal::tests::*`（11 个，含"追加不改历史行"、"坏行跳过"、"裁剪后计数对得上"） |
| **`update`**（真版本比较 + 可注入 fetch） | `crates/engine/src/update.rs` | `update::tests::*`（17 个，含"0.10.0 大于 0.9.0"、"预发布不推给稳定版"、"下载不完整不动旧文件"） |
| **假 provider + `verify`**（抓宿主的真实请求，判人设落在哪层） | `crates/process/src/provider.rs` + `crates/engine/src/verify.rs` + `cmd_verify` | `provider::tests::*`（10 个）、**`verify_detects_a_developer_layer_hit_end_to_end`**（真起 node 假宿主 → 抓请求 → 判 developer）、`verify_reports_user_only_when_the_persona_lands_in_a_user_message`、`verify_never_writes_persona_text_into_the_report` |
| **探测 `doctor`**（装没装、版本、主目录、标记文件、可用通道） | `crates/harness/src/detect.rs` + `crates/engine/src/commands_probe.rs` | `detect::tests::*`（13 个）、`doctor_reports_every_harness`、CLI 端到端 2 个 |
| **启动 `launch`**（15 家的参数形状、默认 dry-run、人设脱敏） | `crates/harness/src/launch_spec.rs` | `launch_spec::tests::*`（22 个）、`launch_preview_redacts_the_persona_body`、CLI 端到端 4 个 |
| **一次性问答 `ask`**（12 家有参数形状，其余明确拒绝） | `crates/harness/src/launch_spec.rs::oneshot` | `oneshot_covers_the_known_harnesses_and_refuses_the_rest`、`ask_refuses_a_harness_without_a_oneshot_mode` |
| CLI：`status`/`ls`/`use`/`mode`/`pause`/`resume`/`arm`/`disarm`/`apply`/`persona`/`rm`/`harnesses`/`doctor`/`launch`/`ask` | `apps/metis/src/main.rs` | `apps/metis/tests/cli.rs`（**22 个端到端用例**） |
| 退出码约定（0 成功 / 1 运行失败 / 2 调用方传错） | `apps/metis/src/main.rs::exit_code_for` | `using_a_missing_persona_fails_with_code_2` 等 |
| 桌面 GUI（4 个屏幕 + 主题 token + **系统 CJK 字体** + 后台任务表） | `crates/ui-egui/src/*.rs`、`apps/metis-gui/src/main.rs` | `screens::tests::*`（5 个无头渲染）、`app::tests::*`（7 个）、`cjk_font_is_installed_on_this_machine`。**已在真机开窗并截图确认渲染** |
| 工程门禁：`layers` / `docs` / `secrets` / `pending` / `ci` / `version` / `coverage` | `xtask/src/main.rs`、`xtask/src/layers.rs` | `xtask` 的 12 个自检用例（含"未注册 crate 必须失败"） |
| 测试夹具（临时 home、假执行器、golden 比对） | `crates/testkit/src/lib.rs` | `testkit::tests::*`（7 个） |

**实测过的真实写入**（本机验证，非单元测试）：
- `metis arm codex --channel install` + `apply` → `~/.codex/config.toml` 里出现 `# >>> metis` 块；
- `metis pause` → 块被切掉，文件其余部分不变；
- `metis resume` → 块写回；
- `metis disarm codex` → 块撤掉，账本条目归零；
- `metis arm pi --channel slash` + `apply` → `~/.pi/agent/extensions/metis.mjs` 与 `runtime.mjs` 落盘，
  `node --check` 通过；用户自己已有的 `orca-*` 扩展**没有被碰**；
- `metis disarm pi` → 两个产物被删除，用户的扩展完好；
- `metis doctor --no-version` → 本机 15 家里识别出 12 家"配置目录存在"，其中 `codex`/`claude` 找到了可执行文件、
  `workbuddy`/`zcode` 正确标记为"只有桌面版"；其余 5 家报"配置目录在，但 PATH 里找不到命令"（已核实确实不在 PATH）。

## 未实现

| 能力 | 路线图 | 现状 |
|---|---|---|

| 人设导入（本地目录 / GitHub `owner/repo`） | M2.2 | 未做（`import-agent-purge` 已能导入，但没有通用的"从目录/GitHub 拉人设"） |

| GUI 的诊断屏真实内容、导入/更新对话框、i18n | M6.3–M6.8 | 部分：4 个屏幕能渲染，但诊断屏只显示路径与一个按钮 |
| GUI 的更新对话框 | M6.7 | 未做（`clean` / `import` / `verify` 的按钮已接上） |
| 打包、安装器、发布 | M7 | 未做 |
| 深色主题、快照测试、键盘快捷键 | M8、M6.10 | 未做 |

## 未验证（诚实清单）

| 项 | 需要什么 |
|---|---|
| 15 家的**真实**探测结果（哪家装了、版本、路径） | 本机装齐这些 harness。已验证：静态能力表、`~/.codex` 的 install 写入、`~/.pi` 的 slash 产物、`~/.dsh` 两个 profile 的插件安装与撤销、`doctor` 的 12/15 识别结果 |
| `verify` 在**真实宿主**上的命中判定 | **仍未取得过一次真实的 `ok`**。本机 6 家可验证宿主（codex/claude/grok/opencode/omp/dsh）**全部返回 `no-request`** —— 它们不接受 Metis 传的 base-url 覆盖。命中判定本身的正确性由进程内端到端测试证明（`a_developer_message_is_judged_ok_end_to_end`），但那不是真机。**要真正验证需要一台宿主认 base-url 的机器。** |
| macOS / Linux 的 shim 与路径行为 | 对应平台机器（当前只在 Windows 上跑过） |

| 跨进程并发 apply | 有单元测试，但没有真跑两个进程抢锁 |

## 当前可运行的命令

```sh
cargo build --workspace          # 构建
cargo test --workspace           # 335 个测试
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask layers               # 分层与外部进程入口检查
cargo xtask docs                 # 文档一致性
cargo xtask secrets              # 密钥与本机路径检查
cargo xtask pending              # 路线图未完成项（当前 71 条）
cargo xtask ci                   # 本地干跑 CI

# CLI（用 METIS_HOME 隔离，不碰真实数据目录）
$env:METIS_HOME = "$env:TEMP\metis-try"
cargo run -p metis -- harnesses
cargo run -p metis -- persona concise --body "用简体中文。" --name "简洁协作"
cargo run -p metis -- use concise
cargo run -p metis -- arm codex --channel install
cargo run -p metis -- apply
cargo run -p metis -- pause
```

## 与基线的对齐进度

`docs/PARITY.md` 是唯一进度表。当前：状态字段与激活语义**已对齐**；四条通道里三条落地、
CLI 已覆盖选/用/装/看/维护五组命令；仍缺 `armor` / `update` / `pull`。

## 下一步

按 `docs/ROADMAP.md`：M3.6（slash 宿主产物 + golden 测试）→ M4.2（真实探测）→ M5.2（verify 抓包）。
这三项做完，功能面才算是"跟 agent-purge 对齐"。
