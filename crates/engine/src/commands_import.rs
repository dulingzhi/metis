//! `import` 命令：从本地目录或 GitHub `owner/repo` 拉人设。

use std::io::Read;

use serde_json::Value;

use metis_core::{ErrorKind, MetisError, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects, optional_str, required_str};

/// 从目录或 GitHub 导入人设。
#[derive(Debug)]
pub struct ImportPersonas;

impl Command for ImportPersonas {
    fn id(&self) -> &'static str {
        "metis.import-personas"
    }
    fn title(&self) -> &'static str {
        "metis.import-personas.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let source = required_str(params, "source")?;
        let options = crate::persona_import::ImportOptions {
            overwrite: params.get("overwrite").and_then(|v| v.as_bool()).unwrap_or(false),
            dry_run: params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(false),
        };

        // 判定来源：显式 `kind`，否则"看起来像目录就用目录，否则当 owner/repo"。
        let kind = optional_str(params, "kind").unwrap_or_else(|| if std::path::Path::new(&source).is_dir() { "dir".to_owned() } else { "github".to_owned() });

        let report = match kind.as_str() {
            "dir" => crate::persona_import::import_dir(engine.paths(), std::path::Path::new(&source), &options)?,
            "github" => {
                // 生产用的 fetch：`ureq` + 短超时。核心逻辑接受注入，测试永不联网。
                //
                // 返回**字节**而不是字符串：tarball 是二进制的。
                crate::persona_import::import_github(engine.paths(), &source, &options, "main", |url| {
                    let response = ureq::get(url).header("user-agent", "Metis").call();
                    match response {
                        Ok(response) => {
                            let status = response.status().as_u16();
                            let mut body = Vec::new();
                            response.into_body().as_reader().take(64 * 1024 * 1024).read_to_end(&mut body).map_err(|error| error.to_string())?;
                            Ok((status, body))
                        }
                        Err(ureq::Error::StatusCode(code)) => Ok((code, Vec::new())),
                        Err(error) => Err(error.to_string()),
                    }
                })?
            }
            other => {
                return Err(MetisError::new(ErrorKind::InvalidArgument, format!("未知来源类型：{other}"))
                    .with_remediation("用 `dir`（本地目录）或 `github`（owner/repo）"));
            }
        };

        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Commands;
    use metis_paths::Paths;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Engine, Commands) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        let engine = Engine::new(paths);
        (dir, engine, Commands::default_set())
    }

    #[test]
    fn import_from_a_directory() {
        let (dir, engine, commands) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("a.md"), "---\nid: a\n---\n\n甲\n").expect("write");

        let value = commands.run("metis.import-personas", &engine, &json!({"source": src.display().to_string()})).expect("import").to_json();
        assert_eq!(value["dryRun"], false);
        let items = value["items"].as_array().expect("items");
        assert!(items.iter().any(|i| i["outcome"] == "imported"), "{items:?}");
    }

    #[test]
    fn import_dry_run_marks_the_flag() {
        let (dir, engine, commands) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("a.md"), "---\nid: a\n---\n\n甲\n").expect("write");

        let value = commands.run("metis.import-personas", &engine, &json!({"source": src.display().to_string(), "dryRun": true})).expect("import").to_json();
        assert_eq!(value["dryRun"], true);
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("metis.import-personas", &engine, &json!({"source": "x", "kind": "ftp"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().unwrap_or_default().contains("github"));
    }

    #[test]
    fn missing_source_parameter_is_rejected() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("metis.import-personas", &engine, &json!({})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    }

    #[test]
    fn a_directory_source_never_touches_the_network() {
        // `kind` 自动判定：给一个真实存在的目录，必须走本地导入器。
        // （`kind: github` 的联网路径由 `persona_import` 的注入式测试覆盖，
        //  这里刻意不触发真网络 —— 测试不得访问外网。）
        let (dir, engine, commands) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("a.md"), "---\nid: a\n---\n\n甲\n").expect("write");

        let value = commands.run("metis.import-personas", &engine, &json!({ "source": src.display().to_string() })).expect("import").to_json();
        assert_eq!(value["items"].as_array().map(|i| i.len()), Some(1));
    }

    #[test]
    fn a_malformed_repo_name_is_rejected_before_any_network_call() {
        // 关键：坏输入在**发请求之前**就被拒，不会真的联网。
        let (_dir, engine, commands) = setup();
        for bad in ["", "no-slash", "a/b/c", "../x"] {
            let result = commands.run("metis.import-personas", &engine, &json!({ "source": bad, "kind": "github" }));
            assert!(result.is_err(), "{bad:?} 应被拒");
            assert_eq!(result.expect_err("err").kind(), ErrorKind::InvalidArgument, "{bad:?} 应在本地被拒");
        }
    }
}
