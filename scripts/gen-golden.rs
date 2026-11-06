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
}
