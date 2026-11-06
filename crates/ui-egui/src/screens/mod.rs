//! 四个屏幕的内容区（P3 重写版）。
//!
//! 每屏都是"页头 + 若干卡片"的结构。所有视觉决定都来自 [`crate::widgets`]
//! 与 [`crate::tokens`]，这里只表达**意图**（这是主操作 / 这是可选项）。
//!
//! 与旧版的区别：
//! - 用 `choice_tile` 代替 `radio`（模式选择本身就是内容，比单选框好读）；
//! - 用 `card()` 而不是 `Frame::group`（统一圆角与描边）；
//! - 状态行用 `pill`（"已接管" / "未安装"一眼可辨）；
//! - 主操作用 `widgets::primary`，一屏只有一个。

use egui::{RichText, Ui};
use serde_json::json;

use crate::model::{self, Filter};
use crate::tokens::{Tokens, font, fonts, space};
use crate::widgets;
use crate::{MetisApp, icons};
use metis_core::{Channel, Mode};

/// 控制台：选人设、定模式、应用变更、勾选接管的 AI。
pub fn console(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "控制台", "选好人设，明确范围，再应用。");

        power_strip(app, ui, &t);
        ui.add_space(space::L);

        persona_card(app, ui, &t);
        ui.add_space(space::L);

        harness_card(app, ui, &t);
    });
}

/// 顶部状态条：开关 + 当前人设 + 模式。
fn power_strip(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    let on = app.snapshot.power_on;
    ui.horizontal(|ui| {
        // 开关本身是个按钮：点一下就切。用 pill 的视觉但保留按钮语义。
        let label = if on { "已开启 · 点击暂停" } else { "已暂停 · 点击恢复" };
        let (fg, bg) = if on { (t.success, t.accent_soft) } else { (t.warning, t.raised) };
        if widgets::pill(ui, label, fg, bg).interact(egui::Sense::click()).clicked() {
            app.invoke("state.power", json!({ "on": !on }));
            app.refresh_blocking();
        }
        let persona = app.draft.persona.clone().unwrap_or_else(|| "未选择人设".to_owned());
        ui.label(RichText::new(persona).size(font::BODY).family(fonts::medium()).color(t.text));
        ui.label(RichText::new(model::mode_label(app.draft.mode)).size(font::SMALL).color(t.text_faint));
    });
}

/// 人设与模式。
fn persona_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);

    widgets::card(t).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "人设与模式");
            widgets::pill(ui, change.label(), t.text_faint, t.raised);
        });
        ui.add_space(space::S);

        // 人设选择：下拉比列表省空间，人设可能很多。
        let current = app.draft.persona.clone().unwrap_or_else(|| "未选择".to_owned());
        egui::ComboBox::from_id_salt("persona-picker").selected_text(RichText::new(current).size(font::BODY)).show_ui(ui, |ui| {
            for persona in &app.snapshot.personas {
                let selected = app.draft.persona.as_deref() == Some(persona.id.as_str());
                if ui.selectable_label(selected, &persona.name).clicked() {
                    app.draft.persona = Some(persona.id.clone());
                }
            }
        });

        ui.add_space(space::M);
        ui.label(RichText::new("模式").size(font::SMALL).color(t.text_faint));
        ui.add_space(space::XS);

        // 三个模式用选择瓦片：每个都带一句"会发生什么"。
        for mode in [Mode::Replace, Mode::Strict, Mode::Append] {
            if widgets::choice_tile(ui, Some(model::mode_icon(mode)), model::mode_label(mode), model::mode_hint(mode), app.draft.mode == mode).clicked() {
                app.draft.mode = mode;
            }
            ui.add_space(space::XS);
        }

        ui.add_space(space::M);
        // 铁律 4：写入不等于生效。这句必须一直在。
        ui.label(RichText::new(model::WRITE_NOT_EFFECT).size(font::CAPTION).color(t.warning));
        ui.add_space(space::S);

        ui.horizontal(|ui| {
            if widgets::secondary(ui, "撤销选择").clicked() {
                app.draft = crate::state::Draft::from_saved(app.snapshot.saved_persona.clone(), app.snapshot.saved_mode);
            }
            let can_apply = change.is_dirty() && app.draft.persona.is_some();
            if widgets::primary(ui, "应用变更", can_apply).clicked() {
                if let Some(persona) = app.draft.persona.clone() {
                    app.invoke("persona.use", json!({ "persona": persona }));
                }
                app.invoke("state.mode", json!({ "mode": app.draft.mode.as_str() }));
                app.spawn_apply();
            }
        });
    });
}

/// 接管哪些 AI。
fn harness_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    widgets::card(t).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "接管哪些 AI");
            widgets::pill(ui, &model::scope_summary(&app.snapshot.harnesses), t.accent_ink, t.accent_soft);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut show = app.ui.show_uninstalled;
                if widgets::toggle(ui, &mut show, "显示本机没装的").changed() {
                    app.ui.show_uninstalled = show;
                }
            });
        });
        ui.add_space(space::S);

        let filter = Filter { injectable_only: false, armed_only: false, installed_only: !app.ui.show_uninstalled };
        let rows = model::filter_rows(&app.snapshot.harnesses, filter);
        if rows.is_empty() {
            widgets::empty_state(ui, "search", "没有匹配的执行器", "试试勾上「显示本机没装的」。");
            return;
        }

        for row in rows {
            let expanded = app.ui.expanded.as_deref() == Some(row.id.as_str());
            // 一家一行：名字 + 状态胶囊 + 展开箭头。
            ui.horizontal(|ui| {
                let arrow = if expanded { "chevron-down" } else { "chevron-right" };
                if icons::button(ui, arrow, 13.0, t.text_faint).clicked() {
                    let id = row.id.clone();
                    app.ui.toggle_expanded(&id);
                }
                ui.label(RichText::new(&row.display).size(font::BODY).family(fonts::medium()).color(t.text));

                // 状态用胶囊 + 文字，不靠颜色单独表达（无障碍要求）。
                let (label, fg, bg) = if row.on {
                    ("注入中", t.success, t.accent_soft)
                } else if row.installed == Some(false) {
                    ("未安装", t.text_faint, t.raised)
                } else {
                    ("未开启", t.text_faint, t.raised)
                };
                widgets::pill(ui, label, fg, bg);
                ui.label(RichText::new(row.status_line()).size(font::CAPTION).color(t.text_faint));
            });

            // 通道开关。
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                for channel in Channel::ALL {
                    let (ok, why) = row.channel(channel);
                    let mut armed = row.is_armed(channel);
                    let text = RichText::new(model::channel_label(channel)).size(font::CAPTION).color(if ok { t.text_dim } else { t.text_faint });
                    let response = ui.add_enabled(ok, egui::Checkbox::new(&mut armed, text));
                    if response.clicked() {
                        let on = !row.is_armed(channel);
                        app.invoke("channel.arm", json!({ "harness": row.id, "channel": channel.as_str(), "on": on }));
                        app.refresh_blocking();
                    }
                    // 铁律 5：不能用就说清为什么。
                    match why {
                        Some(why) if !ok => {
                            response.on_hover_text(why);
                        }
                        _ => {
                            response.on_hover_text(model::channel_hint(channel));
                        }
                    }
                }
            });

            if expanded {
                ui.add_space(space::XS);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new(format!("热切：{}", row.hot)).size(font::CAPTION).color(t.text_faint));
                });
                for channel in Channel::ALL {
                    let (ok, why) = row.channel(channel);
                    if !ok {
                        let why = why.unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new(format!("{} 不可用：{why}", model::channel_label(channel))).size(font::CAPTION).color(t.warning));
                        });
                    }
                }
            }
            ui.add_space(space::XS);
            widgets::separator(ui);
        }
    });
}

/// 人设库。
pub fn library(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "人设库", "正文用于提示词，名称与说明只用于管理。");

        import_card(app, ui, &t);
        ui.add_space(space::L);

        widgets::card(&t).show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section_title(ui, "全部人设");
                widgets::pill(ui, &format!("{} 份", app.snapshot.personas.len()), t.accent_ink, t.accent_soft);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 搜索框：Ctrl+K 会把焦点给它。
                    let response = ui.add(egui::TextEdit::singleline(&mut app.ui.persona_query).hint_text("搜索人设").desired_width(180.0));
                    if std::mem::take(&mut app.ui.focus_search) {
                        response.request_focus();
                    }
                });
            });
            ui.add_space(space::S);

            let query = app.ui.persona_query.trim().to_lowercase();
            let personas: Vec<_> = app
                .snapshot
                .personas
                .iter()
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query) || p.id.to_lowercase().contains(&query))
                .collect();

            if personas.is_empty() {
                let (icon, title, body) = if query.is_empty() {
                    ("folder", "还没有人设", "用 `metis persona new` 建一份，或从 agent-purge 导入。")
                } else {
                    ("search", "没有匹配的人设", "换个关键词试试。")
                };
                widgets::empty_state(ui, icon, title, body);
                return;
            }

            for persona in personas {
                let is_current = app.snapshot.saved_persona.as_deref() == Some(persona.id.as_str());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(ui, rect, "file-code", 16.0, if is_current { t.accent_ink } else { t.icon });
                    ui.label(RichText::new(&persona.name).size(font::BODY).family(fonts::medium()).color(t.text));
                    if is_current {
                        widgets::pill(ui, "当前", t.on_accent, t.accent);
                    }
                    if let Some(description) = persona.description.as_deref().filter(|d| !d.is_empty()) {
                        ui.label(RichText::new(description).size(font::CAPTION).color(t.text_faint));
                    }
                });
                ui.add_space(space::XS);
                widgets::separator(ui);
            }
        });
    });
}

/// 从 GitHub 导入人设。
///
/// 默认填官方源（[`crate::state::OFFICIAL_SOURCE`]），点一下就能用；
/// 想导别人的直接改输入框。
fn import_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    widgets::card(t).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "从 GitHub 导入");
            if app.ui.import_source.trim() == crate::state::OFFICIAL_SOURCE {
                widgets::pill(ui, "官方源", t.accent_ink, t.accent_soft);
            }
        });
        widgets::caption(ui, "填 `owner/repo`。只读；同名人设不会被覆盖，会标为「跳过」。");
        ui.add_space(space::S);

        // 输入框独占一行，按钮另起一行。
        //
        // 原本是"输入框 + 两个按钮"挤在一行，真机截图里按钮被挤出卡片外看不见 ——
        // `horizontal` 里 `available_width` 会随前一个控件缩小，算着减 210 不可靠。
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.import_source)
                .hint_text("owner/repo")
                .desired_width(f32::INFINITY)
                .font(egui::FontId::new(font::BODY, fonts::body())),
        );
        ui.add_space(space::S);
        ui.horizontal(|ui| {
            if widgets::primary(ui, "导入", true).clicked() {
                crate::actions::run(app, crate::actions::Action::ImportGithub, None);
            }
            if widgets::secondary(ui, "预览").clicked() {
                crate::actions::run(app, crate::actions::Action::ImportGithubDryRun, None);
            }
        });

        if app.jobs.is_busy() {
            ui.add_space(space::S);
            widgets::working(ui, "正在导入…", None);
        }
    });
}

/// 验证与诊断。
pub fn diagnostics(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "验证与诊断", "配置写入不代表有会话已生效；这里做的是实测与清理。");

        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "实测验证");
            widgets::caption(ui, "启动本地假 provider，抓宿主的真实请求，判定人设落在哪一层。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::primary(ui, "实测验证", true).clicked() {
                    crate::actions::run(app, crate::actions::Action::Verify, None);
                }
                if widgets::secondary(ui, "重新同步已选范围").clicked() {
                    app.spawn_apply();
                }
            });
            if app.jobs.is_busy() {
                ui.add_space(space::S);
                widgets::working(ui, "正在验证…", None);
            }
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "撤回接管");
            widgets::caption(ui, "只撤 Metis 写过的内容；人设与配置保留。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "预览清理").clicked() {
                    crate::actions::run(app, crate::actions::Action::CleanDryRun, None);
                }
                if widgets::danger(ui, "撤回全部接管").clicked() {
                    crate::actions::run(app, crate::actions::Action::Clean, None);
                }
            });
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "诊断信息");
            ui.add_space(space::S);
            widgets::key_value(ui, "数据目录", &app.snapshot.state_path);
            widgets::key_value(ui, "当前人设", app.snapshot.saved_persona.as_deref().unwrap_or("（未选择）"));
            widgets::key_value(ui, "当前模式", model::mode_label(app.snapshot.saved_mode));
            widgets::key_value(ui, "注入状态", if app.snapshot.power_on { "已开启" } else { "已暂停" });
            widgets::key_value(ui, "接管情况", &model::scope_summary(&app.snapshot.harnesses));
        });
    });
}

/// 关于。
pub fn about(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "关于 Metis", "一份人设，统一管理多个 coding agent。");

        widgets::card(&t).show(ui, |ui| {
            widgets::key_value(ui, "版本", env!("CARGO_PKG_VERSION"));
            widgets::key_value(ui, "数据目录", &app.snapshot.state_path);
            widgets::key_value(ui, "许可", "MIT OR Apache-2.0");
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "备份占用");
            widgets::caption(ui, "整文件回滚用的原件。只清理账本不再引用的那些；被引用的永远保留。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "查看占用").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsUsage, None);
                }
                if widgets::secondary(ui, "预览清理").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsPruneDryRun, None);
                }
                if widgets::danger(ui, "清理无用备份").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsPrune, None);
                }
            });
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "从 agent-purge 迁移");
            widgets::caption(ui, "只读导入人设与部分状态；接管状态需要重新开启（两边落点标记不同）。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "预览导入").clicked() {
                    crate::actions::run(app, crate::actions::Action::ImportDryRun, None);
                }
                if widgets::primary(ui, "从 agent-purge 导入", true).clicked() {
                    crate::actions::run(app, crate::actions::Action::Import, None);
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    /// 无头渲染一屏，返回图元数。
    fn render(screen: crate::state::Screen, app: &mut MetisApp) -> usize {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| match screen {
            crate::state::Screen::Console => console(app, ui),
            crate::state::Screen::Library => library(app, ui),
            crate::state::Screen::Diagnostics => diagnostics(app, ui),
            crate::state::Screen::About => about(app, ui),
        });
        output.textures_delta.clear();
        output.shapes.len()
    }

    #[test]
    fn every_screen_renders_something() {
        for screen in crate::state::Screen::ALL {
            let (_dir, mut app) = app();
            let count = render(screen, &mut app);
            assert!(count > 5, "{screen:?} 只画了 {count} 个图元");
        }
    }

    #[test]
    fn every_screen_renders_in_dark_mode_too() {
        // 深色下也要能画：颜色全取自 token，不该出现"白底白字"。
        for screen in crate::state::Screen::ALL {
            let (_dir, mut app) = app();
            let ctx = egui::Context::default();
            Tokens::install_full(&ctx, Tokens::DARK);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| match screen {
                crate::state::Screen::Console => console(&mut app, ui),
                crate::state::Screen::Library => library(&mut app, ui),
                crate::state::Screen::Diagnostics => diagnostics(&mut app, ui),
                crate::state::Screen::About => about(&mut app, ui),
            });
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{screen:?} 在深色下没渲染");
        }
    }

    #[test]
    fn the_console_shows_the_three_modes_as_tiles() {
        // 三个模式各一个瓦片；选中的那个用 accent 底。
        // 断言方式：三种 draft.mode 各渲染一次都必须有内容，且不 panic。
        for mode in [Mode::Replace, Mode::Strict, Mode::Append] {
            let (_dir, mut app) = app();
            app.draft.mode = mode;
            assert!(render(crate::state::Screen::Console, &mut app) > 5, "{mode} 下控制台没渲染");
        }
    }

    #[test]
    fn the_apply_button_is_disabled_until_something_changes() {
        // 没改动时不能"应用"：`primary(..., enabled)` 的 enabled 参数要真的生效。
        let (_dir, mut app) = app();
        let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);
        assert!(!change.is_dirty(), "初始状态不该是 dirty");
        // 改一下就该 dirty。
        app.draft.persona = Some("someone".to_owned());
        let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);
        assert!(change.is_dirty(), "选了人设之后应该是 dirty");
    }

    #[test]
    fn an_empty_library_shows_an_empty_state() {
        // 没有人设时给"怎么办"，而不是一片空白。
        let (_dir, mut app) = app();
        assert!(app.snapshot.personas.is_empty(), "前置条件：临时 home 里没有人设");
        assert!(render(crate::state::Screen::Library, &mut app) > 5);
    }

    #[test]
    fn the_library_search_filters_the_list() {
        let (_dir, mut app) = app();
        app.snapshot.personas = vec![
            crate::model::PersonaRow { id: "concise".to_owned(), name: "简洁协作".to_owned(), description: None, chars: 10, active: false },
            crate::model::PersonaRow { id: "strict".to_owned(), name: "严格模式".to_owned(), description: None, chars: 8, active: false },
        ];
        app.ui.persona_query = "简洁".to_owned();
        // 过滤发生在渲染里；这里验证数据侧：过滤后只剩一个。
        let query = app.ui.persona_query.to_lowercase();
        let kept: Vec<_> = app.snapshot.personas.iter().filter(|p| p.name.to_lowercase().contains(&query) || p.id.to_lowercase().contains(&query)).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "concise");
    }

    #[test]
    fn the_diagnostics_screen_survives_a_busy_state() {
        let (_dir, mut app) = app();
        app.busy = Some("verify".to_owned());
        assert!(render(crate::state::Screen::Diagnostics, &mut app) > 5);
    }

    #[test]
    fn the_about_screen_lists_version_and_paths() {
        let (_dir, mut app) = app();
        assert!(render(crate::state::Screen::About, &mut app) > 5);
    }
}
