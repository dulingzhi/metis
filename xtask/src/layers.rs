//! 工作区路径与分层表。
//!
//! `layers` 是整套规则能自动执行的前提（`docs/DEVELOPMENT.md` §2）。

use std::path::{Path, PathBuf};

/// 一个 crate 的分层登记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrateLayer {
    /// 目录名（`crates/<dir>` 或 `apps/<dir>`）。
    pub dir: &'static str,
    /// 包的 name。
    pub package: &'static str,
    /// 层号：越小越底层。
    pub layer: u32,
    /// 是否允许依赖 egui / eframe / winit / rfd。
    pub ui_allowed: bool,
}

/// 全部登记在册的 crate。**新增 crate 必须在这里加一行，否则 `layers` 会失败。**
pub const LAYERS: &[CrateLayer] = &[
    CrateLayer { dir: "core", package: "metis-core", layer: 0, ui_allowed: false },
    CrateLayer { dir: "paths", package: "metis-paths", layer: 0, ui_allowed: false },
    CrateLayer { dir: "fs", package: "metis-fs", layer: 0, ui_allowed: false },
    CrateLayer { dir: "process", package: "metis-process", layer: 1, ui_allowed: false },
    CrateLayer { dir: "persona", package: "metis-persona", layer: 2, ui_allowed: false },
    CrateLayer { dir: "harness", package: "metis-harness", layer: 2, ui_allowed: false },
    CrateLayer { dir: "channel", package: "metis-channel", layer: 2, ui_allowed: false },
    CrateLayer { dir: "engine", package: "metis-engine", layer: 3, ui_allowed: false },
    CrateLayer { dir: "testkit", package: "metis-testkit", layer: 9, ui_allowed: false },
    CrateLayer { dir: "ui-egui", package: "metis-ui-egui", layer: 4, ui_allowed: true },
    CrateLayer { dir: "metis", package: "metis", layer: 5, ui_allowed: false },
    CrateLayer { dir: "metis-gui", package: "metis-gui", layer: 5, ui_allowed: true },
];

/// 只有 UI 层能用的依赖。
pub const UI_ONLY: &[&str] = &["egui", "eframe", "egui_extras", "egui_kittest", "winit", "rfd"];

/// 不允许在 UI 代码里出现的副作用调用（`ui_has_no_side_effects` 检查）。
pub const UI_FORBIDDEN: &[&str] = &["std::fs::write", "File::create", "OpenOptions", "std::fs::remove_", "std::fs::rename", "std::process"];

/// 工作区根目录（xtask 在 `xtask/` 下，所以上一层就是根）。
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(Path::to_path_buf).unwrap_or(manifest)
}

/// 全部 crate 的 manifest 路径。
pub fn crate_manifests(root: &Path) -> Vec<(CrateLayer, PathBuf)> {
    let mut out = Vec::new();
    for entry in LAYERS {
        for base in ["crates", "apps"] {
            let manifest = root.join(base).join(entry.dir).join("Cargo.toml");
            if manifest.is_file() {
                out.push((*entry, manifest));
                break;
            }
        }
    }
    out
}

/// 找出工作区里存在但没登记的 crate 目录。
pub fn unregistered(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for base in ["crates", "apps"] {
        let Ok(entries) = std::fs::read_dir(root.join(base)) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !path.join("Cargo.toml").is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if !LAYERS.iter().any(|layer| layer.dir == name) {
                out.push(path);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_and_dirs_are_unique() {
        let mut packages: Vec<&str> = LAYERS.iter().map(|l| l.package).collect();
        packages.sort_unstable();
        let count = packages.len();
        packages.dedup();
        assert_eq!(count, packages.len(), "包名重复");

        let mut dirs: Vec<&str> = LAYERS.iter().map(|l| l.dir).collect();
        dirs.sort_unstable();
        let count = dirs.len();
        dirs.dedup();
        assert_eq!(count, dirs.len(), "目录重复");
    }

    #[test]
    fn only_ui_crates_may_use_egui() {
        for entry in LAYERS {
            if entry.ui_allowed {
                assert!(entry.package.contains("ui") || entry.package.contains("gui"), "{} 声明了 ui_allowed，看起来不像 UI crate", entry.package);
            }
        }
    }

    #[test]
    fn core_is_the_bottom_layer() {
        let core = LAYERS.iter().find(|l| l.package == "metis-core").expect("core");
        assert_eq!(core.layer, 0);
        assert!(!core.ui_allowed);
    }

    #[test]
    fn workspace_root_has_a_cargo_toml() {
        let root = workspace_root();
        assert!(root.join("Cargo.toml").is_file(), "找不到工作区根：{}", root.display());
    }

    #[test]
    fn every_registered_crate_exists_on_disk() {
        let root = workspace_root();
        let manifests = crate_manifests(&root);
        assert_eq!(manifests.len(), LAYERS.len(), "有登记项在磁盘上找不到 manifest");
    }

    #[test]
    fn no_unregistered_crates() {
        let root = workspace_root();
        let extra = unregistered(&root);
        assert!(extra.is_empty(), "有未登记的 crate：{extra:?}");
    }
}
