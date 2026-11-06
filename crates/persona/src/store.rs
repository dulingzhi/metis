//! 人设库的读写。
//!
//! 所有路径来自 [`metis_paths::Paths`]；所有写盘走 [`metis_fs::write_atomic`]。

use std::path::PathBuf;

use metis_core::{ErrorKind, MetisError, PersonaId, Result};
use metis_paths::Paths;

use crate::meta::Meta;
use crate::parse::{parse_persona, render_persona};

/// 一份人设。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    /// 有效 id（元数据优先，否则文件名）。
    pub id: PersonaId,
    /// 元数据。
    pub meta: Meta,
    /// 正文（**只有它进入提示词**）。
    pub body: String,
    /// 文件路径。
    pub path: PathBuf,
    /// 解析告警。
    pub warnings: Vec<String>,
}

impl Persona {
    /// 显示名。
    pub fn display_name(&self) -> String {
        self.meta.display_name(&self.id)
    }

    /// 一句话描述（管理界面用）。
    pub fn description(&self) -> Option<&str> {
        self.meta.description.as_deref()
    }

    /// 正文字符数（界面上的字计数）。
    pub fn char_count(&self) -> usize {
        self.body.chars().count()
    }

    /// 规范化后的正文：首尾空白去掉、行尾统一。写进宿主的就是它。
    pub fn trimmed_body(&self) -> &str {
        self.body.trim()
    }
}

/// 库里出问题的条目（解析失败的、id 冲突的）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    /// 哪个文件。
    pub path: PathBuf,
    /// 怎么了。
    pub message: String,
}

/// 人设库。
#[derive(Debug, Clone)]
pub struct PersonaStore {
    paths: Paths,
}

impl PersonaStore {
    /// 用给定路径建库。
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// 人设目录。
    pub fn dir(&self) -> PathBuf {
        self.paths.personas_dir()
    }

    /// 列出全部人设，按 `order` 再按 id 排序。
    ///
    /// 解析失败的文件**不会让整次列表失败**：它们进 `errors`，用户能看见并修。
    pub fn list(&self) -> Result<(Vec<Persona>, Vec<StoreError>)> {
        let dir = self.dir();
        let mut personas = Vec::new();
        let mut errors = Vec::new();
        if !dir.is_dir() {
            return Ok((personas, errors));
        }
        let entries = std::fs::read_dir(&dir).map_err(|error| MetisError::from(error).with_detail(dir.display().to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
            if stem == "active" {
                continue; // 快照文件，不是人设
            }
            match self.read_file(&path, &stem) {
                Ok(persona) => personas.push(persona),
                Err(error) => errors.push(StoreError { path, message: error.to_string() }),
            }
        }
        personas.sort_by(|a, b| {
            let order_a = a.meta.order.unwrap_or(i64::MAX);
            let order_b = b.meta.order.unwrap_or(i64::MAX);
            order_a.cmp(&order_b).then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        // id 冲突也要报告，不能悄悄只留一个。
        let mut seen: Vec<String> = Vec::new();
        for persona in &personas {
            let id = persona.id.as_str().to_owned();
            if seen.contains(&id) {
                errors.push(StoreError { path: persona.path.clone(), message: format!("人设 id 重复：{id}") });
            } else {
                seen.push(id);
            }
        }
        Ok((personas, errors))
    }

    fn read_file(&self, path: &std::path::Path, stem: &str) -> Result<Persona> {
        let bytes = std::fs::read(path).map_err(|error| MetisError::from(error).with_detail(path.display().to_string()))?;
        if bytes.len() > PersonaId::MAX_BODY_BYTES {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, format!("人设文件超过 4 MiB：{}", path.display()))
                .with_remediation("提示词不该这么大；拆成多份或删减正文"));
        }
        let text = String::from_utf8(bytes).map_err(|error| {
            MetisError::new(ErrorKind::PersonaInvalid, format!("人设文件不是 UTF-8：{}", path.display()))
                .with_detail(error.to_string())
                .with_remediation("另存为 UTF-8 再试")
        })?;
        let parsed = parse_persona(&text);
        let (id, used_fallback) = parsed.meta.effective_id(stem);
        let mut warnings = parsed.warnings;
        if used_fallback && parsed.meta.id.is_some() {
            warnings.push(format!("元数据里的 id 非法，改用文件名：{stem}"));
        }
        if let Some(declared) = parsed.meta.id.as_deref()
            && declared != stem
        {
            warnings.push(format!("文件名叫 {stem}.md，但元数据写的是 {declared}"));
        }
        Ok(Persona { id, meta: parsed.meta, body: parsed.body, path: path.to_path_buf(), warnings })
    }

    /// 读一份人设。
    pub fn read(&self, id: &PersonaId) -> Result<Option<Persona>> {
        let path = self.paths.persona_file(id);
        if !path.is_file() {
            return Ok(None);
        }
        self.read_file(&path, id.as_str()).map(Some)
    }

    /// 写一份人设（新建或更新）。
    pub fn save(&self, id: &PersonaId, meta: &Meta, body: &str) -> Result<PathBuf> {
        if body.len() > PersonaId::MAX_BODY_BYTES {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, "人设正文超过 4 MiB").with_remediation("删减正文，或拆成多份人设"));
        }
        let path = self.paths.persona_file(id);
        let rendered = render_persona(id, meta, body);
        metis_fs::write_atomic(&path, rendered.as_bytes())?;
        Ok(path)
    }

    /// 删除一份人设。不存在不算失败。
    pub fn delete(&self, id: &PersonaId) -> Result<bool> {
        let path = self.paths.persona_file(id);
        if !path.is_file() {
            return Ok(false);
        }
        metis_fs::remove_if_exists(&path)?;
        Ok(true)
    }

    /// 写 `active.md` 快照（当前生效人设），供宿主插件在不读 state 的情况下取正文。
    pub fn set_active(&self, persona: &Persona) -> Result<PathBuf> {
        let path = self.paths.active_persona_file();
        let content = format!("{}\n", persona.trimmed_body());
        metis_fs::write_atomic(&path, content.as_bytes())?;
        Ok(path)
    }

    /// 读 `active.md` 快照。
    pub fn active_body(&self) -> Result<Option<String>> {
        metis_fs::read_to_string_optional(&self.paths.active_persona_file())
    }

    /// 读 `active.md` 快照里的 id（用于判断快照是否过期）。
    pub fn mark_active_stale(&self) -> Result<()> {
        metis_fs::remove_if_exists(&self.paths.active_persona_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, PersonaStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::with_home(dir.path());
        (dir, PersonaStore::new(paths))
    }

    fn id(value: &str) -> PersonaId {
        PersonaId::parse(value).expect("id")
    }

    #[test]
    fn empty_store_lists_nothing() {
        let (_dir, store) = store();
        let (personas, errors) = store.list().expect("list");
        assert!(personas.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn save_then_read_round_trips() {
        let (_dir, store) = store();
        let meta = Meta { name: Some("简洁协作".to_owned()), description: Some("先结论".to_owned()), ..Meta::default() };
        let path = store.save(&id("concise"), &meta, "用简体中文。\n").expect("save");
        assert!(path.is_file());

        let read = store.read(&id("concise")).expect("read").expect("present");
        assert_eq!(read.id.as_str(), "concise");
        assert_eq!(read.display_name(), "简洁协作");
        assert_eq!(read.trimmed_body(), "用简体中文。");
        assert_eq!(read.description(), Some("先结论"));
    }

    #[test]
    fn save_preserves_unknown_metadata_keys() {
        let (_dir, store) = store();
        let mut meta = Meta::default();
        meta.extra.insert("custom-tool".to_owned(), "yes".to_owned());
        store.save(&id("x"), &meta, "body").expect("save");
        let read = store.read(&id("x")).expect("read").expect("present");
        assert_eq!(read.meta.extra.get("custom-tool").map(String::as_str), Some("yes"));
    }

    #[test]
    fn list_sorts_by_order_then_id() {
        let (_dir, store) = store();
        let late = Meta { order: Some(20), ..Meta::default() };
        store.save(&id("bbb"), &late, "b").expect("save b");
        let early = Meta { order: Some(10), ..Meta::default() };
        store.save(&id("zzz"), &early, "z").expect("save z");
        store.save(&id("aaa"), &Meta::default(), "a").expect("save a");

        let (personas, _) = store.list().expect("list");
        let order: Vec<&str> = personas.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, vec!["zzz", "bbb", "aaa"], "先按 order，再按 id；没有 order 的排最后");
    }

    #[test]
    fn active_snapshot_is_not_listed_as_a_persona() {
        let (_dir, store) = store();
        store.save(&id("real"), &Meta::default(), "body").expect("save");
        let persona = store.read(&id("real")).expect("read").expect("present");
        store.set_active(&persona).expect("set active");

        let (personas, _) = store.list().expect("list");
        assert_eq!(personas.len(), 1);
        assert_eq!(personas[0].id.as_str(), "real");
    }

    #[test]
    fn active_snapshot_round_trips_trimmed_body() {
        let (_dir, store) = store();
        store.save(&id("real"), &Meta::default(), "  正文  \n").expect("save");
        let persona = store.read(&id("real")).expect("read").expect("present");
        store.set_active(&persona).expect("set active");
        assert_eq!(store.active_body().expect("body").as_deref(), Some("正文\n"));
    }

    #[test]
    fn deleting_a_persona_is_idempotent() {
        let (_dir, store) = store();
        store.save(&id("x"), &Meta::default(), "b").expect("save");
        assert!(store.delete(&id("x")).expect("delete"));
        assert!(!store.delete(&id("x")).expect("delete again"));
        assert!(store.read(&id("x")).expect("read").is_none());
    }

    #[test]
    fn oversized_body_is_rejected() {
        let (_dir, store) = store();
        let body = "x".repeat(PersonaId::MAX_BODY_BYTES + 1);
        let error = store.save(&id("big"), &Meta::default(), &body).expect_err("should reject");
        assert_eq!(error.kind(), ErrorKind::PersonaInvalid);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn non_utf8_file_is_reported_not_fatal() {
        let (dir, store) = store();
        let personas = dir.path().join("personas");
        std::fs::create_dir_all(&personas).expect("mkdir");
        std::fs::write(personas.join("bad.md"), [0xff, 0xfe]).expect("seed");
        store.save(&id("good"), &Meta::default(), "ok").expect("save");

        let (list, errors) = store.list().expect("list must not fail wholesale");
        assert_eq!(list.len(), 1, "好的那份仍应列出");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("UTF-8"));
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let (dir, store) = store();
        let personas = dir.path().join("personas");
        std::fs::create_dir_all(&personas).expect("mkdir");
        // 两个文件都声明同一个 id。
        std::fs::write(personas.join("a.md"), "---\nid: shared\n---\nA\n").expect("seed a");
        std::fs::write(personas.join("b.md"), "---\nid: shared\n---\nB\n").expect("seed b");
        let (list, errors) = store.list().expect("list");
        assert_eq!(list.len(), 2);
        assert!(errors.iter().any(|e| e.message.contains("重复")), "id 冲突必须报告：{errors:?}");
    }

    #[test]
    fn mismatched_id_warns() {
        let (dir, store) = store();
        let personas = dir.path().join("personas");
        std::fs::create_dir_all(&personas).expect("mkdir");
        std::fs::write(personas.join("file.md"), "---\nid: declared\n---\nbody\n").expect("seed");
        let (list, _) = store.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id.as_str(), "declared");
        assert!(list[0].warnings.iter().any(|w| w.contains("文件名叫 file.md")), "应提示文件名与 id 不一致");
    }

    #[test]
    fn char_count_counts_characters_not_bytes() {
        let (_dir, store) = store();
        store.save(&id("zh"), &Meta::default(), "中文四字").expect("save");
        let persona = store.read(&id("zh")).expect("read").expect("present");
        assert_eq!(persona.char_count(), 4, "字数应数字符，不是字节");
    }

    #[test]
    fn mark_active_stale_removes_the_snapshot() {
        let (_dir, store) = store();
        store.save(&id("x"), &Meta::default(), "b").expect("save");
        let persona = store.read(&id("x")).expect("read").expect("present");
        store.set_active(&persona).expect("set active");
        assert!(store.active_body().expect("body").is_some());
        store.mark_active_stale().expect("clear");
        assert!(store.active_body().expect("body").is_none());
    }
}
