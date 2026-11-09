//! 界面快照基线（P5）：把每个屏幕渲染成 PNG，与仓库里的基线逐像素比较。
//!
//! # 为什么需要
//!
//! 组件的单测只能证明"画了东西"，不能证明"画得对"。一次不小心的内边距改动
//! 会让所有卡片错位 —— 测试全绿，界面变丑。快照把"长什么样"变成可 diff 的事实。
//!
//! # 怎么跑
//!
//! ```sh
//! cargo xtask snapshot           # 与基线比较（CI 用）
//! cargo xtask snapshot --update  # 更新基线（改完界面后跑，把 diff 一起提交）
//! ```
//!
//! # 为什么默认 `#[ignore]`
//!
//! `egui_kittest` 需要 wgpu 后端。**在没有可用显示设备的机器上它不是失败，是永久挂起**
//! （`docs/PITFALLS.md` G3）。所以：
//!
//! - 本文件里的测试全部 `#[ignore]`，`cargo test` 不会碰它们；
//! - `cargo xtask snapshot` 用 `--ignored` 显式调用；
//! - 无 GPU 环境用 `METIS_SKIP_SNAPSHOT=1` 让门禁跳过而不是挂住。
//!
//! # 基线怎么存
//!
//! `crates/ui-egui/tests/snapshots/<screen>-<theme>.png`。
//!
//! 判等直接比 PNG 字节 —— 前提是**渲染本身完全确定**。这一点花了力气才做到：
//! fixture 里所有会变的东西（临时目录名、提示条、busy 状态）都必须固定，
//! 否则两次跑出来的图会在几个像素上抖动（`docs/PITFALLS.md` G65）。

// 这是测试工具，往 stdout 报告"写了哪个基线"是它的职责。
#![allow(clippy::print_stdout)]

use metis_paths::Paths;
use metis_ui_egui::tokens::Tokens;
use metis_ui_egui::{MetisApp, Screen};

/// 基线目录。
fn snapshot_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots")
}

/// 造一个内容确定的 app（不碰真实 HOME，也不依赖本机装了哪些执行器）。
fn fixture_app() -> (tempfile::TempDir, MetisApp) {
    let dir = tempfile::tempdir().expect("tempdir");
    // 用**固定**路径而不是 `dir.path()`：临时目录名每次都不同，而状态栏会把
    // `state.json` 的完整路径画出来 —— 那 9 个字符的差异会让截图底部抖动
    // （实测差异区域 x[480..613] y[878..889]，正好是路径文字）。
    let paths = Paths::with_home(std::path::PathBuf::from("C:/Users/example/.metis")).with_harness_root(std::path::PathBuf::from("C:/Users/example/harnesses"));
    let mut app = MetisApp::with_paths(paths);
    app.refresh_blocking();

    // `refresh_blocking` 会探测本机 PATH（`harness.list` 的 `detect: true`），
    // 于是 console 的快照会随"那台机器装了什么"漂移 —— 这正是 G67 的根因。
    // 用注册表的**静态事实**重建一份确定的行；安装状态也写死。
    app.snapshot.harnesses = metis_harness::registry::all()
        .iter()
        .map(|spec| {
            let mut row = metis_ui_egui::model::row_from_spec(spec, &[], None);
            row.on = false;
            row.explain = "未启用".to_owned();
            row.installed = Some(true);
            row.has_binary = true;
            row
        })
        .collect();

    // 固定内容：人设、草稿、展开项都写死，否则基线会随本机状态漂移。
    app.snapshot.personas = vec![
        metis_ui_egui::model::PersonaRow {
            id: "concise".to_owned(),
            name: "简洁协作".to_owned(),
            description: Some("用简体中文协作".to_owned()),
            chars: 42,
            active: true,
        },
        metis_ui_egui::model::PersonaRow { id: "strict".to_owned(), name: "严格模式".to_owned(), description: None, chars: 18, active: false },
    ];
    app.draft.persona = Some("concise".to_owned());
    app.draft.mode = metis_core::Mode::Replace;
    app.snapshot.saved_persona = Some("concise".to_owned());
    app.snapshot.saved_mode = metis_core::Mode::Replace;
    // 展开一家，让通道开关那一块也进画面。
    app.ui.expanded = Some("codex".to_owned());

    // **清掉提示条**：它每次 `refresh_blocking` 给出的文案可能不同
    // （"就绪" / 某个瞬时状态），会让截图在底部 12 行像素上抖动。
    // 这是快照不确定的真实来源 —— 第一次做基线时两次跑出不同的哈希才发现。
    app.notice = None;
    app.busy = None;
    (dir, app)
}

/// 渲染一屏，返回 PNG 字节。
fn render(screen: Screen, dark: bool, width: f32, height: f32) -> Vec<u8> {
    use egui_kittest::Harness;

    let mut harness = Harness::builder().with_size(egui::vec2(width, height)).with_pixels_per_point(1.0).with_max_steps(64).wgpu().build_eframe(move |cc| {
        metis_ui_egui::setup(&cc.egui_ctx, dark, true);
        let (dir, mut app) = fixture_app();
        app.set_screen(screen);
        // 临时目录必须在 app 存活期间一直有效。
        //
        // 与其给产品代码加一个只为测试存在的字段，不如在这里**故意泄漏**它：
        // 这是一次性测试进程，泄漏一个目录不会积累。产品结构保持干净。
        std::mem::forget(dir);
        app
    });

    for _ in 0..12 {
        harness.step();
    }

    let image = harness.render().expect("render");
    // `egui_kittest` 的 ImageBuffer 只有 `save`：存到临时文件再读回来。
    // 比依赖 `image` crate 的编码 API 稳（它的版本与 feature 组合很挑）。
    let temp = std::env::temp_dir().join(format!("metis-snapshot-{}.png", uuid::Uuid::new_v4()));
    image.save(&temp).expect("save png");
    let bytes = std::fs::read(&temp).expect("read png");
    let _ = std::fs::remove_file(&temp);
    bytes
}

/// 一张快照的标识。
struct Case {
    name: &'static str,
    screen: Screen,
    dark: bool,
}

const CASES: &[Case] = &[
    Case { name: "console-light", screen: Screen::Console, dark: false },
    Case { name: "console-dark", screen: Screen::Console, dark: true },
    Case { name: "library-light", screen: Screen::Library, dark: false },
    Case { name: "diagnostics-light", screen: Screen::Diagnostics, dark: false },
    Case { name: "about-light", screen: Screen::About, dark: false },
];

/// 是否被要求跳过（无 GPU 环境用）。
fn skipped() -> bool {
    matches!(std::env::var("METIS_SKIP_SNAPSHOT").as_deref(), Ok("1") | Ok("true"))
}

#[test]
#[ignore = "需要 wgpu；用 cargo xtask snapshot 跑"]
fn snapshots_match_the_baseline() {
    if skipped() {
        println!("跳过快照（METIS_SKIP_SNAPSHOT 已设置）");
        return;
    }
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).expect("mkdir");
    let update = matches!(std::env::var("METIS_UPDATE_SNAPSHOT").as_deref(), Ok("1") | Ok("true"));

    let mut problems = Vec::new();
    for case in CASES {
        let actual = render(case.screen, case.dark, 1180.0, 900.0);
        let path = dir.join(format!("{}.png", case.name));
        if update || !path.exists() {
            std::fs::write(&path, &actual).expect("write baseline");
            println!("写入基线 {}", path.display());
            continue;
        }
        let baseline = std::fs::read(&path).expect("read baseline");
        if baseline != actual {
            problems.push(case.name);
        }
    }

    assert!(problems.is_empty(), "这些屏幕与基线不一致：{problems:?}\n跑 `cargo xtask snapshot --update` 更新基线，并把 diff 一起提交。");
}

#[test]
#[ignore = "需要 wgpu；用 cargo xtask snapshot 跑"]
fn every_case_renders_a_non_blank_image() {
    if skipped() {
        return;
    }
    // 空白画面是快照测试最容易漏掉的失败形态：基线本身就拍了一张白图。
    for case in CASES {
        let png = render(case.screen, case.dark, 900.0, 620.0);
        assert!(png.len() > 12_000, "{} 的截图只有 {} 字节，很可能是空白", case.name, png.len());
    }
}

#[test]
fn the_snapshot_cases_cover_every_screen() {
    // 这条**不需要 GPU**，所以它能在任何机器上跑 —— 保证"新增屏幕必须加进基线"。
    for screen in Screen::ALL {
        assert!(CASES.iter().any(|case| case.screen == screen), "{screen:?} 没有快照用例");
    }
}

#[test]
fn both_themes_are_covered() {
    assert!(CASES.iter().any(|case| case.dark), "缺少深色用例");
    assert!(CASES.iter().any(|case| !case.dark), "缺少浅色用例");
}

#[test]
fn the_fixture_is_deterministic() {
    // 基线的前提是 fixture 每次都一样。这条守住它。
    let (_d1, a) = fixture_app();
    let (_d2, b) = fixture_app();
    assert_eq!(a.snapshot.personas.len(), b.snapshot.personas.len());
    assert_eq!(a.draft.persona, b.draft.persona);
    assert_eq!(a.draft.mode, b.draft.mode);
    assert_eq!(a.ui.expanded, b.ui.expanded);
    assert_eq!(Tokens::for_dark(false).kind, metis_ui_egui::ThemeKind::Light);
    assert_eq!(Tokens::for_dark(true).kind, metis_ui_egui::ThemeKind::Dark);
}
