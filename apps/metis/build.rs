//! 把应用图标与版本信息嵌进 Windows 可执行文件。
//!
//! 为什么需要它：Windows 的 exe 没有"内嵌图标"这回事 —— 图标是**资源**，
//! 必须用一个 `.rc` 编译进 exe。不做这件事的表现是：资源管理器里显示的是
//! 默认的空白程序图标，属性页里没有版本/公司信息，用户会以为这是个来路不明的程序。
//!
//! **只在 Windows 上跑**：其他平台没有资源段，图标由打包流程处理
//! （macOS 用 `.app` 的 `Info.plist` + `.icns`，Linux 用 `.desktop`）。

fn main() {
    // 改图标要能触发重新编译，否则换了图看不到变化。
    println!("cargo:rerun-if-changed=../../assets/app-icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    // **整个 Windows 分支必须 `#[cfg]` 掉**。
    //
    // `winresource` 在 Cargo.toml 里是 `[target.'cfg(windows)'.build-dependencies]`，
    // 所以在 Linux/macOS 上它**根本不存在**。直接 `use` 会让 build script 编译失败
    // （`cannot find module or crate winresource`）—— CI 的三个平台因此只有
    // Windows 能过。这是第一次跑 CI 才发现的。
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    // 版本从 Cargo.toml 来，**不要在别处再写一遍** ——
    // 两处写版本必然有一天不一致（`M7.3` 就是为此存在的）。
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let name = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "metis".to_owned());
    let description = std::env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();

    // 产品名：合并后只有一个 exe，CLI 与 GUI 共用同一个名字。
    let product = "Metis";

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../assets/app-icon.ico");
    resource.set("FileDescription", if description.is_empty() { product } else { &description });
    resource.set("ProductName", product);
    resource.set("FileVersion", &version);
    resource.set("ProductVersion", &version);
    resource.set("CompanyName", "Metis contributors");
    resource.set("LegalCopyright", "MIT OR Apache-2.0");
    resource.set("OriginalFilename", &format!("{name}.exe"));

    // 失败要给**可操作的**提示，而不是让 cargo 抛一句 "failed to compile resources"。
    if let Err(error) = resource.compile() {
        println!(
            "cargo:warning=嵌入 Windows 图标/版本信息失败：{error}\n\
             这只影响图标与属性页，不影响程序功能。\n\
             常见原因：没装 Windows SDK（rc.exe）；或 icon 文件损坏。"
        );
    }
}
