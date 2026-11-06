//! 测试夹具。
//!
//! 存在的唯一目的：让每个测试都**不碰真实 `$HOME`、不调真实 harness、不依赖网络**
//! （`docs/TESTING.md` §3）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
// 这个 crate 是**测试夹具**：它的职责就是"测试环境搭不起来就立刻炸"，
// 以及在 golden 不一致时用 panic 给出可读差异。这里的 panic 是产品行为，不是缺陷。
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use metis_core::PersonaId;
use metis_engine::Engine;
use metis_paths::Paths;

/// 临时数据根：`Drop` 时清理，测试绝不触碰真实用户目录。
#[derive(Debug)]
pub struct TempHome {
    dir: tempfile::TempDir,
    paths: Paths,
}

impl TempHome {
    /// 新建。
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("临时目录必须能创建");
        let paths = Paths::with_home(dir.path().join("metis-home"));
        Self { dir, paths }
    }

    /// 数据目录。
    pub fn root(&self) -> &Path {
        self.paths.home()
    }

    /// 路径集合。
    pub fn paths(&self) -> Paths {
        self.paths.clone()
    }

    /// 建一个引擎。
    pub fn engine(&self) -> Engine {
        Engine::new(self.paths.clone())
    }

    /// 环境变量（给子进程用）。
    pub fn env(&self) -> Vec<(String, String)> {
        vec![(metis_paths::ENV_HOME.to_owned(), self.paths.home().display().to_string())]
    }

    /// 写 `state.json` 的原始文本（用于测试损坏与未知字段）。
    pub fn write_state_raw(&self, text: &str) -> PathBuf {
        let file = self.paths.state_file();
        metis_fs::ensure_dir(self.paths.home()).expect("mkdir");
        std::fs::write(&file, text).expect("write state");
        file
    }

    /// 放一份人设。
    pub fn seed_persona(&self, id: &str, body: &str) {
        let store = metis_persona::PersonaStore::new(self.paths.clone());
        let persona_id = PersonaId::parse(id).expect("persona id");
        store.save(&persona_id, &metis_persona::Meta::default(), body).expect("save persona");
    }

    /// 临时目录本身（用来造假的可执行文件之类）。
    pub fn scratch(&self) -> &Path {
        self.dir.path()
    }
}

impl Default for TempHome {
    fn default() -> Self {
        Self::new()
    }
}

/// 比对 golden 文件；不一致时给出可读的差异与重建提示。
pub fn assert_golden(golden_path: &Path, actual: &[u8]) {
    let Ok(expected) = std::fs::read(golden_path) else {
        panic!("golden 文件不存在：{}\n提示：设置 UPDATE_GOLDEN=1 重建", golden_path.display());
    };
    if expected == actual {
        return;
    }
    let expected_text = String::from_utf8_lossy(&expected);
    let actual_text = String::from_utf8_lossy(actual);
    let mut diff = String::new();
    for (index, (a, b)) in expected_text.lines().zip(actual_text.lines()).enumerate() {
        if a != b {
            diff.push_str(&format!("行 {}:\n  期望: {a}\n  实际: {b}\n", index + 1));
        }
        if diff.len() > 2000 {
            diff.push_str("…（更多差异已省略）\n");
            break;
        }
    }
    if diff.is_empty() {
        diff.push_str(&format!("长度不同：期望 {} 字节，实际 {} 字节\n", expected.len(), actual.len()));
    }
    panic!("与 golden 不一致：{}\n{diff}", golden_path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_home_is_isolated_and_cleanable() {
        let home = TempHome::new();
        assert!(home.root().to_string_lossy().contains("metis-home"));
        assert!(!home.root().exists(), "还没写东西时不该存在");
        metis_fs::ensure_dir(home.root()).expect("mkdir");
        assert!(home.root().is_dir());
    }

    #[test]
    fn env_points_at_the_temp_home() {
        let home = TempHome::new();
        let env = home.env();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "METIS_HOME");
        assert!(env[0].1.contains("metis-home"));
    }

    #[test]
    fn seeded_persona_is_readable_through_the_engine() {
        let home = TempHome::new();
        home.seed_persona("concise", "正文");
        let store = metis_persona::PersonaStore::new(home.paths());
        let persona = store.read(&PersonaId::parse("concise").expect("id")).expect("read").expect("present");
        assert_eq!(persona.trimmed_body(), "正文");
    }

    #[test]
    fn write_state_raw_can_seed_a_corrupt_file() {
        let home = TempHome::new();
        let file = home.write_state_raw("{ broken");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "{ broken");
        let error = home.engine().state().expect_err("corrupt");
        assert_eq!(error.kind(), metis_core::ErrorKind::StateCorrupt);
    }

    #[test]
    fn golden_helper_accepts_identical_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let golden = dir.path().join("g.txt");
        std::fs::write(&golden, b"same").expect("seed");
        assert_golden(&golden, b"same");
    }

    #[test]
    #[should_panic(expected = "行 1")]
    fn golden_helper_reports_the_first_differing_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let golden = dir.path().join("g.txt");
        std::fs::write(&golden, b"expected\n").expect("seed");
        assert_golden(&golden, b"actual\n");
    }

    #[test]
    #[should_panic(expected = "golden 文件不存在")]
    fn golden_helper_says_how_to_rebuild_when_missing() {
        assert_golden(Path::new("/definitely/missing/golden.txt"), b"x");
    }
}
