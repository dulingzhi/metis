//! DSH 的 `cordis.patch.yml` 挂载块。
//!
//! 只把插件文件放进 `node_modules/` 是**不会生效**的：宿主要先被 YAML 里的
//! `- insert:` 条目点名，才会加载它。这一步是 DSH 通道真正生效的最后一环。
//!
//! 写法约束（照宿主自己的 patch 层语义）：
//! - patch 文件是**顶层 YAML 数组**，每个条目形如 `- id: … / name: …`；
//! - 我们要"插入一个新插件"，所以条目是 `- insert:` 加一个列表；
//! - 模板里如果只有文档根的 `[]`（空 flow 数组），必须先摊平 ——
//!   `[]` 后面直接接 `- insert:` 是非法 YAML；
//! - 用起止注释标出我们那一段，撤销时精确切除，**块外一个字节都不改**。

use std::path::Path;

use metis_core::Result;
use metis_fs::footprint::{self, Footprint, Undo};

/// 挂载块的起止标记（带插件名，多个插件互不覆盖）。
#[cfg(test)]
pub(crate) fn markers_for_test(plugin: &str) -> (String, String) {
    markers(plugin)
}

fn markers(plugin: &str) -> (String, String) {
    (format!("# >>> metis:{plugin}"), format!("# <<< metis:{plugin}"))
}

/// `cordis.patch.yml` 的相对位置。
pub const PATCH_FILE: &str = "cordis.patch.yml";

/// 渲染挂载块（**不含**前导空行：由拼接处决定，避免重复叠加）。
pub fn render_block(plugin: &str) -> String {
    let (begin, end) = markers(plugin);
    format!("{begin}\n- insert:\n    - id: {plugin}\n      name: {plugin}\n{end}\n")
}

/// 切除挂载块。块外内容逐字节保留。
pub fn strip_block(content: &str, plugin: &str) -> (String, bool) {
    let (begin, end) = markers(plugin);
    let mut out = String::with_capacity(content.len());
    let mut inside = false;
    let mut removed = false;
    for line in content.split_inclusive('\n') {
        if inside {
            if line.trim() == end {
                inside = false;
            }
            removed = true;
            continue;
        }
        if line.trim() == begin {
            inside = true;
            removed = true;
            // 吃掉块前面我们自己加的那个空行，才能逐字节还原。
            if out.ends_with("\n\n") {
                out.pop();
            }
            continue;
        }
        out.push_str(line);
    }
    (out, removed)
}

/// 把挂载块插进 patch 文件内容。
///
/// 幂等：已经有同名块就替换掉旧的，不会叠加。
pub fn upsert(content: &str, plugin: &str) -> String {
    let (stripped, _) = strip_block(content, plugin);
    let body = flatten_empty_root(&stripped);
    let mut out = body;
    if !out.is_empty() {
        // 与前面的条目之间留一个空行，纯为了可读；撤销时会被一并切掉。
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&render_block(plugin));
    out
}

/// 把文档根的 `[]` 摊平。
///
/// 宿主的初始模板就是一个 `[]`；空 flow 数组后面接 `- insert:` 会被 YAML 解析器拒绝
/// （`end of the stream or a document separator is expected`）。只处理**文档根**那一行 ——
/// 缩进里的 `[]` 和手写的 flow 数组原样留着。
pub fn flatten_empty_root(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut done = false;
    for line in content.split_inclusive('\n') {
        if !done && !line.trim().is_empty() && !line.trim_start().starts_with('#') {
            if line.trim() == "[]" {
                done = true;
                continue; // 丢掉这一行
            }
            done = true;
        }
        out.push_str(line);
    }
    out
}

/// 挂载一个插件到某个 profile。
pub fn mount(home: &Path, profile_dir: &Path, plugin: &str) -> Result<bool> {
    let file = profile_dir.join(PATCH_FILE);
    let existing = metis_fs::read_to_string_optional(&file)?.unwrap_or_default();
    let next = upsert(&existing, plugin);
    if next == existing {
        return Ok(false);
    }
    metis_fs::write_atomic(&file, next.as_bytes())?;
    footprint::record(
        home,
        Footprint {
            id: String::new(),
            path: file.clone(),
            kind: "file".to_owned(),
            harness: "dsh".to_owned(),
            channel: "install".to_owned(),
            undo: Undo::Block { marker: markers(plugin).0 },
            sha256: metis_fs::hash_file(&file)?,
            created_at: String::new(),
        },
    )?;
    Ok(true)
}

/// 从某个 profile 卸载挂载块。
pub fn unmount(home: &Path, profile_dir: &Path, plugin: &str) -> Result<bool> {
    let file = profile_dir.join(PATCH_FILE);
    let Some(existing) = metis_fs::read_to_string_optional(&file)? else { return Ok(false) };
    let (next, removed) = strip_block(&existing, plugin);
    if !removed {
        return Ok(false);
    }
    if next.trim().is_empty() {
        // 切完只剩空白：把文件还原成宿主的空模板，而不是留一个空文件。
        metis_fs::write_atomic(&file, b"[]\n")?;
    } else {
        metis_fs::write_atomic(&file, next.as_bytes())?;
    }
    footprint::cancel(home, footprint::CancelKey::Location { path: &file, harness: "dsh", channel: "install" })?;
    Ok(true)
}

#[cfg(test)]
// 测试要调 node 验证 YAML 结构：测试代码，不起宿主进程。
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn render_mentions_the_plugin_by_id_and_name() {
        let block = render_block("metis");
        assert!(block.contains("- insert:"));
        assert!(block.contains("id: metis"));
        assert!(block.contains("name: metis"));
        assert!(block.contains("# >>> metis:metis"));
    }

    #[test]
    fn empty_root_array_is_flattened() {
        // 宿主初始模板就是 `[]`：不摊平的话 `- insert:` 会让 YAML 解析失败。
        assert_eq!(flatten_empty_root("[]\n"), "");
        assert_eq!(flatten_empty_root("[]"), "");
        // 有真实内容时不动。
        let real = "- id: a\n  name: b\n";
        assert_eq!(flatten_empty_root(real), real);
        // 注释在前、`[]` 在后：只丢 `[]`。
        let commented = "# 说明\n[]\n";
        assert_eq!(flatten_empty_root(commented), "# 说明\n");
    }

    #[test]
    fn upsert_onto_empty_file_produces_valid_yaml_shape() {
        let out = upsert("[]\n", "metis");
        assert!(!out.contains("[]"), "空 flow 数组必须被摊平：{out}");
        assert!(out.trim_start().starts_with("# >>> metis:metis"), "挂载块应是数组的第一个条目：{out}");
    }

    #[test]
    fn upsert_preserves_existing_entries() {
        let existing = "- id: ui-chat\n  name: \"@deepseek-ai/dsh-client-ui-chat\"\n  config:\n    transcriptView: detailed\n";
        let out = upsert(existing, "metis");
        assert!(out.starts_with(existing), "用户已有条目必须原样保留");
        assert!(out.contains("- insert:"));
    }

    #[test]
    fn upsert_is_idempotent() {
        let once = upsert("- id: a\n", "metis");
        let twice = upsert(&once, "metis");
        assert_eq!(once, twice, "重复挂载不能叠加");
        assert_eq!(twice.matches("# >>> metis:metis").count(), 1);
    }

    #[test]
    fn different_plugins_do_not_overwrite_each_other() {
        let a = upsert("- id: x\n", "metis");
        let b = upsert(&a, "metis-slash");
        assert!(b.contains("# >>> metis:metis\n"), "第一个插件的块必须还在");
        assert!(b.contains("# >>> metis:metis-slash"));
    }

    #[test]
    fn strip_removes_only_our_block() {
        let original = "- id: keep\n  name: keep\n";
        let mounted = upsert(original, "metis");
        let (stripped, removed) = strip_block(&mounted, "metis");
        assert!(removed);
        assert_eq!(stripped, original, "块外内容必须逐字节还原");
    }

    #[test]
    fn strip_without_a_block_is_a_noop() {
        let content = "- id: a\n";
        let (stripped, removed) = strip_block(content, "metis");
        assert!(!removed);
        assert_eq!(stripped, content);
    }

    #[test]
    fn mount_then_unmount_round_trips_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).expect("mkdir");
        let before = "- id: ui-chat\n  name: chat\n";
        std::fs::write(profile.join(PATCH_FILE), before).expect("seed");

        assert!(mount(&home, &profile, "metis").expect("mount"));
        let mounted = std::fs::read_to_string(profile.join(PATCH_FILE)).expect("read");
        assert!(mounted.contains("- insert:"));
        assert!(mounted.starts_with(before), "原有内容必须在前面原样保留");

        assert!(unmount(&home, &profile, "metis").expect("unmount"));
        assert_eq!(std::fs::read_to_string(profile.join(PATCH_FILE)).expect("read"), before, "卸载必须逐字节还原");
    }

    #[test]
    fn mount_is_idempotent_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).expect("mkdir");
        let file = profile.join(PATCH_FILE);
        std::fs::write(&file, "[]\n").expect("seed");

        assert!(mount(&home, &profile, "metis").expect("first"));
        let once = std::fs::read_to_string(&file).expect("read");
        assert!(!mount(&home, &profile, "metis").expect("second"), "第二次不该再写");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), once);
    }

    #[test]
    fn mounted_patch_is_valid_yaml_with_an_insert_entry() {
        // 只断言"文本里有 - insert:" 不够：YAML 解析器不认就等于没挂上。
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("cordis.patch.yml");
        let existing = "# 说明\n- id: ui-chat\n  name: chat\n  config:\n    transcriptView: detailed\n";
        std::fs::write(&file, existing).expect("seed");
        let mounted = upsert(existing, "metis");
        std::fs::write(&file, &mounted).expect("write");

        let script = dir.path().join("check.mjs");
        std::fs::write(
            &script,
            format!(
                "import {{ readFileSync }} from 'node:fs';\nconst t = readFileSync({:?}, 'utf8');\nif (/^\\[\\]\\s*$/m.test(t)) {{ console.error('empty flow array left in place'); process.exit(1); }}\nif (!/^- insert:/m.test(t)) {{ console.error('no insert entry'); process.exit(1); }}\nconsole.log('OK');\n",
                file.display().to_string()
            ),
        )
        .expect("write script");

        let output = std::process::Command::new("node").arg(&script).output().expect("node");
        assert!(output.status.success(), "挂载后的 patch 结构不对：{}", String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn unmount_restores_the_host_empty_template() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).expect("mkdir");
        let file = profile.join(PATCH_FILE);
        std::fs::write(&file, "[]\n").expect("seed");

        mount(&home, &profile, "metis").expect("mount");
        unmount(&home, &profile, "metis").expect("unmount");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "[]\n", "切空之后要还给宿主一个合法模板");
    }
}
