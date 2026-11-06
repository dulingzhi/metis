//! 重建 `slash` 的 golden 产物。
//!
//! 由 `cargo xtask golden` 调用。**这是开发工具，不是产品代码**：
//! 它唯一的工作是把当前模板渲染成文件，所以允许 `println!` 报告写了什么
//! （通过 clippy 的显式豁免，理由就是这一行）。
#![allow(clippy::print_stdout)]
fn main() {
    let home = std::path::Path::new("C:/Users/example/.metis");
    let harness = metis_core::HarnessId::parse("codex").expect("id");
    for (name, template) in [
        ("extension.mjs", metis_channel::slash::Template::Extension),
        ("opencode-v1.mjs", metis_channel::slash::Template::OpencodeV1),
        ("opencode-v2.mjs", metis_channel::slash::Template::OpencodeV2),
        ("dsh-plugin.mjs", metis_channel::slash::Template::Dsh),
    ] {
        let rendered = metis_channel::slash::render(template, home, &harness);
        let path = std::path::Path::new("crates/channel/assets/golden").join(name);
        std::fs::write(&path, rendered.as_bytes()).expect("write");
        println!("wrote {} ({} bytes)", path.display(), rendered.len());
    }
    // 运行时文件是原样写出的（不走占位符替换），也要有 golden。
    let runtime = std::path::Path::new("crates/channel/assets/golden").join("runtime.mjs");
    std::fs::write(&runtime, metis_channel::slash::RUNTIME.as_bytes()).expect("write");
    println!("wrote {} ({} bytes)", runtime.display(), metis_channel::slash::RUNTIME.len());
}
