# 第三方归属

Metis 本身以 **MIT OR Apache-2.0** 双许可发布（见 `LICENSE-MIT` / `LICENSE-APACHE`）。

## 运行时依赖

全部为宽松许可（MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode / OFL 等），`deny.toml` 会拒绝其它类别。
完整清单由 `cargo deny check licenses` 生成。主要依赖：

| 依赖 | 许可 | 用途 |
|---|---|---|
| `eframe` / `egui` / `egui_extras` | MIT OR Apache-2.0 | 桌面界面 |
| `wgpu` / `winit` | MIT OR Apache-2.0 | 渲染与窗口 |
| `serde` / `serde_json` | MIT OR Apache-2.0 | 一切持久化 |
| `clap` | MIT OR Apache-2.0 | CLI 解析 |
| `thiserror` | MIT OR Apache-2.0 | 错误类型 |
| `sha2` / `hex` | MIT OR Apache-2.0 | 内容哈希（账本与备份） |
| `uuid` | Apache-2.0 OR MIT | 交换 operation id |
| `time` | MIT OR Apache-2.0 | RFC 3339 时间戳 |
| `dirs` | MIT OR Apache-2.0 | 用户主目录 |
| `schemars` | MIT | 命令参数 schema |
| `toml` | MIT OR Apache-2.0 | 宿主 `config.toml` 的读写校验 |
| `process-wrap` | MIT | 进程组 / Job Object |

## 字体

M1 阶段**没有**内置任何字体文件：界面使用系统字体（`Microsoft YaHei UI` / `PingFang SC` /
`Noto Sans CJK SC` 等）。将来若内置字体，必须同时提交其许可证文本（OFL-1.1）并把链接加进本文件。

## 图标

M1 阶段**没有**内置第三方图标：界面只用文字标签与 egui 自绘图形。
将来若引入图标集（例如 Lucide，ISC 许可），必须提交其许可证并把链接加进本文件。

## 品牌

Metis 不内置任何 coding agent 的官方图标或商标素材。`docs/PARITY.md` 的 decision 条目记录了
这条决定（倾向：不内置，用名称 + 通用图标）。
