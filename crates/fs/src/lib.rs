//! 文件系统原语：原子写、哈希、路径规范化、控制锁。
//!
//! 这一层不懂任何 harness 语义，只保证**写的时候不留半截文件**、**锁住的时候不留死锁**。
//! 所有权账本见 [`footprint`]，交换与恢复见 [`swap`]。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod footprint;
pub mod lock;
pub mod swap;

use std::fs;
use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Result};
use sha2::{Digest, Sha256};

/// 文件权限：数据目录里的东西只有本人可读写。
pub const FILE_MODE: u32 = 0o600;

/// 目录权限。
pub const DIR_MODE: u32 = 0o700;

/// 计算字节的 sha256（十六进制小写）。
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// 计算文件的 sha256；文件不存在返回 `None`（**不是错误**：首次写入是常态）。
pub fn hash_file(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(hash_bytes(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(MetisError::from(error).with_detail(path.display().to_string())),
    }
}

/// 读文件；不存在返回 `None`。
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(MetisError::from(error).with_detail(path.display().to_string())),
    }
}

/// 读文件为 UTF-8 字符串；不存在返回 `None`。
///
/// 非 UTF-8 内容是**错误**，不是"尽力解码"：猜错编码会毁掉用户的配置文件。
pub fn read_to_string_optional(path: &Path) -> Result<Option<String>> {
    let Some(bytes) = read_optional(path)? else { return Ok(None) };
    String::from_utf8(bytes).map(Some).map_err(|error| {
        MetisError::new(ErrorKind::Io, format!("文件不是 UTF-8 文本：{}", path.display()))
            .with_detail(error.to_string())
            .with_remediation("用编辑器把它另存为 UTF-8")
    })
}

/// 建目录（含父级），权限尽量收紧。
pub fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|error| MetisError::from(error).with_detail(path.display().to_string()))?;
    restrict_dir(path)
}

#[cfg(unix)]
fn restrict_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // 失败不致命：某些文件系统（例如挂载的 FAT）不支持改权限。
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(DIR_MODE);
        let _ = fs::set_permissions(path, perms);
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) -> Result<()> {
    Ok(())
}

/// 原子写：先写同目录临时文件，`fsync`，再 `rename` 覆盖。
///
/// 关键性质（`ARCHITECTURE.md` 不变式 I1 的基础）：
/// - 任何一步失败，**目标文件保持原样**；
/// - 不留下临时文件；
/// - 目标已存在时权限保持不变（`rename` 会换成临时文件的权限，所以临时文件沿用原权限）。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| MetisError::new(ErrorKind::Io, format!("目标路径没有父目录：{}", path.display())).with_remediation("给一个完整的文件路径"))?;
    ensure_dir(parent)?;

    let previous_mode = fs::metadata(path).ok().map(|meta| meta.permissions());
    let temp = unique_sibling(path, "tmp");
    let write = || -> Result<()> {
        {
            let mut file = fs::File::create(&temp).map_err(|error| MetisError::from(error).with_detail(temp.display().to_string()))?;
            std::io::Write::write_all(&mut file, bytes).map_err(|error| MetisError::from(error).with_detail(temp.display().to_string()))?;
            file.sync_all().map_err(|error| MetisError::from(error).with_detail(temp.display().to_string()))?;
        }
        if let Some(perms) = previous_mode.clone() {
            let _ = fs::set_permissions(&temp, perms);
        } else {
            restrict_file(&temp);
        }
        fs::rename(&temp, path).map_err(|error| MetisError::from(error).with_detail(path.display().to_string()))?;
        Ok(())
    };
    let result = write();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(unix)]
fn restrict_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(FILE_MODE);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) {}

/// 删除文件；不存在不算失败。
pub fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MetisError::from(error).with_detail(path.display().to_string())),
    }
}

/// 在同一目录里生成一个不冲突的兄弟路径（用于临时文件）。
pub fn unique_sibling(path: &Path, kind: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".to_owned());
    let tag = uuid::Uuid::new_v4().simple().to_string();
    path.with_file_name(format!(".{name}.{kind}-{tag}"))
}

/// 把路径规范化成**所有者键**：绝对、去 `.`/`..`；Windows 上转小写。
///
/// 大小写是 Windows 上最容易被忽略的一致性来源（`PITFALLS.md` B5）。
pub fn owner_key(path: &Path) -> String {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_path_buf()) };
    let normalised = normalise_components(&absolute);
    let text = normalised.to_string_lossy().into_owned();
    if cfg!(windows) { text.to_lowercase() } else { text }
}

fn normalise_components(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_hex() {
        let a = hash_bytes(b"hello");
        let b = hash_bytes(b"hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, hash_bytes(b"hellp"));
    }

    #[test]
    fn hash_file_returns_none_for_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(hash_file(&dir.path().join("nope")).ok().flatten(), None);
    }

    #[test]
    fn write_atomic_creates_and_replaces() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("state.json");
        write_atomic(&file, b"one").expect("first write");
        assert_eq!(fs::read(&file).ok(), Some(b"one".to_vec()));
        write_atomic(&file, b"two").expect("second write");
        assert_eq!(fs::read(&file).ok(), Some(b"two".to_vec()));
    }

    #[test]
    fn write_atomic_leaves_no_temp_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        write_atomic(&file, b"x").expect("write");
        let names: Vec<String> =
            fs::read_dir(dir.path()).expect("read_dir").filter_map(|entry| entry.ok()).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.txt".to_owned()], "不该留下临时文件：{names:?}");
    }

    #[test]
    fn write_atomic_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("deep").join("nested").join("f.txt");
        write_atomic(&file, b"ok").expect("write");
        assert!(file.is_file());
    }

    #[test]
    fn write_atomic_without_parent_is_an_error() {
        let result = write_atomic(Path::new(""), b"x");
        assert!(result.is_err());
    }

    #[test]
    fn read_to_string_rejects_invalid_utf8() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("bad.bin");
        fs::write(&file, [0xff, 0xfe, 0x00]).expect("seed");
        let error = read_to_string_optional(&file).expect_err("should fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some(), "要告诉用户另存为 UTF-8");
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("gone.txt");
        assert!(remove_if_exists(&file).is_ok());
        fs::write(&file, b"x").expect("seed");
        assert!(remove_if_exists(&file).is_ok());
        assert!(!file.exists());
    }

    #[test]
    fn ensure_dir_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b");
        ensure_dir(&nested).expect("first");
        ensure_dir(&nested).expect("second");
        assert!(nested.is_dir());
    }

    #[test]
    fn owner_key_is_case_insensitive_on_windows_only() {
        let lower = owner_key(Path::new("C:\\Data\\State.json"));
        let upper = owner_key(Path::new("C:\\Data\\State.json"));
        assert_eq!(lower, upper);
        if cfg!(windows) {
            assert_eq!(owner_key(Path::new("C:\\DATA\\state.JSON")), lower, "Windows 上大小写必须归一");
        }
    }

    #[test]
    fn unique_sibling_keeps_directory_and_extension_family() {
        // **用平台无关的方式构造路径**。
        //
        // 原来写的是字面量 `Path::new("D:\\a\\b\\state.json")` —— 在 Windows 上
        // 反斜杠是分隔符，`parent()` 是 `D:\a\b`；在 Linux/macOS 上反斜杠只是普通
        // 字符，整串是**一个文件名**，`parent()` 是空路径，断言必然失败。
        // CI 的 Linux 测试第一次跑就红了。
        let dir = Path::new("a").join("b");
        let file = dir.join("state.json");
        let first = unique_sibling(&file, "tmp");
        let second = unique_sibling(&file, "tmp");
        assert_ne!(first, second, "两次生成必须不同");
        assert_eq!(first.parent(), Some(dir.as_path()), "必须留在同一个目录里");
        let name = first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        assert!(name.contains("tmp"), "应带上 kind 标记：{name}");
        // **原名要完整保留在临时名里**（含扩展名），这样清理时能看出它属于谁。
        // 实际形状是 `.state.json.tmp-<tag>`。
        assert!(name.contains("state.json"), "原名应保留：{name}");
        assert!(name.starts_with('.'), "临时文件应是隐藏名：{name}");
    }
}
