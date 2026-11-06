//! 组件画廊：把组件库里的每一个组件画一遍。
//!
//! 它的用途有两个，都很实际：
//! 1. **视觉验收**：改主题或组件后，看一眼就知道有没有变丑；
//! 2. **快照基线**：`cargo run -p metis-ui-egui --example gallery` 出图，
//!    和 `target/gallery/` 里上一次的图对比。
//!
//! 它**不是产品界面的一部分**——是一个开发工具。
//!
//! 作为工具，它的职责之一就是把"写到哪了"打出来，所以显式允许打印。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use metis_ui_egui::tokens::{Tokens, font, fonts, space};
use metis_ui_egui::{icons, widgets};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = arg(&args, "--out-dir").unwrap_or_else(|| "target/gallery".to_owned());
    let dark = args.iter().any(|a| a == "--dark");
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(2.0);

    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        eprintln!("建不了输出目录 {out_dir}：{error}");
        return;
    }

    let name = if dark { "gallery-dark" } else { "gallery-light" };
    let path = std::path::Path::new(&out_dir).join(format!("{name}.png"));

    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(1180.0, 1500.0)).with_pixels_per_point(scale).with_max_steps(64).wgpu().build_eframe(move |cc| {
            metis_ui_egui::setup(&cc.egui_ctx, dark, false);
            GalleryApp
        });

    for _ in 0..24 {
        harness.step();
    }

    match harness.render() {
        Ok(image) => match image.save(&path) {
            Ok(()) => println!("wrote {} ({}×{})", path.display(), image.width(), image.height()),
            Err(error) => eprintln!("存不了 {}：{error}", path.display()),
        },
        Err(error) => eprintln!("渲染失败：{error}"),
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

struct GalleryApp;

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = Tokens::get(ui.ctx());
        egui::Frame::new().fill(t.canvas).inner_margin(egui::Margin::same(28)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                widgets::page_header(ui, "组件画廊", "Metis 设计系统的全部组件。改主题或组件后跑一次，看图对比。");

                // ── 按钮 ──
                section(ui, &t, "按钮");
                ui.horizontal(|ui| {
                    widgets::primary(ui, "应用变更", true);
                    widgets::primary(ui, "禁用状态", false);
                    widgets::secondary(ui, "撤销选择");
                    widgets::link(ui, "了解更多");
                    widgets::danger(ui, "删除");
                });

                ui.add_space(space::L);
                section(ui, &t, "分段控件");
                let _ = widgets::segmented(ui, &["全部", "已安装", "可注入"], 0);

                ui.add_space(space::L);
                section(ui, &t, "选择瓦片（选中 / 未选）");
                widgets::choice_tile(ui, Some("shield-check"), "换身份", "优先替换身份行", true);
                ui.add_space(space::S);
                widgets::choice_tile(ui, Some("message-square"), "只留人设", "替换可控入口中的宿主文本", false);

                ui.add_space(space::L);
                section(ui, &t, "卡片");
                widgets::card(&t).show(ui, |ui| {
                    widgets::section_title(ui, "人设与模式");
                    widgets::caption(ui, "未修改");
                    ui.add_space(space::S);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("concise").size(font::BODY).color(t.text));
                        widgets::pill(ui, "已接管", t.success, t.accent_soft);
                        widgets::pill(ui, "未安装", t.text_faint, t.raised);
                        widgets::pill(ui, "警告", t.warning, t.raised);
                    });
                    widgets::separator(ui);
                    widgets::key_value(ui, "数据目录", "C:/Users/example/.metis");
                    widgets::key_value(ui, "模式", "replace");
                });

                ui.add_space(space::L);
                section(ui, &t, "状态色（必须同时有文字，不能只靠颜色）");
                ui.horizontal(|ui| {
                    widgets::pill(ui, "成功", t.success, t.raised);
                    widgets::pill(ui, "警告", t.warning, t.raised);
                    widgets::pill(ui, "错误", t.danger, t.raised);
                    widgets::pill(ui, "信息", t.accent_ink, t.accent_soft);
                });

                ui.add_space(space::L);
                section(ui, &t, "图标（Lucide，按需着色）");
                ui.horizontal_wrapped(|ui| {
                    for name in [
                        "house",
                        "bot",
                        "activity",
                        "info",
                        "settings",
                        "search",
                        "plus",
                        "check",
                        "x",
                        "trash-2",
                        "refresh-cw",
                        "shield-check",
                        "circle-help",
                        "chevron-right",
                        "chevron-down",
                        "layers",
                        "play",
                        "moon",
                        "sun",
                        "gauge",
                        "history",
                        "database",
                        "download",
                        "upload",
                        "lock",
                        "puzzle",
                        "radar",
                        "terminal",
                    ] {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                        icons::paint(ui, rect, name, 18.0, t.icon);
                    }
                });

                ui.add_space(space::L);
                section(ui, &t, "工作指示器");
                widgets::working(ui, "正在检查 15 家执行器…", Some("已扫描 7 家"));

                ui.add_space(space::L);
                section(ui, &t, "空状态");
                widgets::card(&t).show(ui, |ui| {
                    widgets::empty_state(ui, "folder", "还没有人设", "用 metis persona new 建一份，或从 agent-purge 导入。");
                });

                ui.add_space(space::L);
                section(ui, &t, "字体层级");
                ui.label(egui::RichText::new("一级标题 H1").size(font::H1).family(fonts::heading()).color(t.text));
                ui.label(egui::RichText::new("二级标题 H2").size(font::H2).family(fonts::medium()).color(t.text));
                ui.label(egui::RichText::new("区块标题").size(font::SECTION).family(fonts::medium()).color(t.text));
                ui.label(egui::RichText::new("正文 body —— 中文与 Latin mixed 都要好看").size(font::BODY).color(t.text));
                ui.label(egui::RichText::new("小字 small").size(font::SMALL).color(t.text_dim));
                ui.label(egui::RichText::new("说明 caption").size(font::CAPTION).color(t.text_faint));
            });
        });
    }
}

fn section(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.label(egui::RichText::new(title).size(font::SMALL).color(t.text_faint));
    ui.add_space(space::S);
}
