//! 界面文案：TSV 表 + 缺键检测。
//!
//! 设计取舍：
//! - **TSV 而不是 JSON/YAML**：一屏能看完、diff 干净、翻译者不需要懂格式；
//! - **键缺失时回退到键本身**，而不是空串或 panic —— 界面上出现 `state.status.title`
//!   是明显的 bug 信号，但不会让程序崩；
//! - **引擎与界面共用一套键**：CLI 打出来的标题和窗口里显示的必须是同一句文案，
//!   否则同一件事在两种入口有两种说法。
//!
//! 表放在 `assets/i18n/`，用 `include_str!` 编进二进制（单可执行文件原则）。

/// 支持的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// 简体中文（默认）。
    #[default]
    ZhHans,
    /// 英文。
    En,
}

impl Lang {
    /// 序列化标识（也是 TSV 列名）。
    pub const fn code(self) -> &'static str {
        match self {
            Self::ZhHans => "zh-Hans",
            Self::En => "en",
        }
    }

    /// 从标识解析（认 `zh`、`zh-CN`、`zh-Hans`、`en`、`en-US`）。
    pub fn parse(value: &str) -> Option<Self> {
        let lowered = value.trim().to_ascii_lowercase();
        if lowered == "zh" || lowered.starts_with("zh-") || lowered.starts_with("zh_") {
            Some(Self::ZhHans)
        } else if lowered == "en" || lowered.starts_with("en-") || lowered.starts_with("en_") {
            Some(Self::En)
        } else {
            None
        }
    }

    /// 全部语言（缺键检测要逐列检查）。
    pub const ALL: [Self; 2] = [Self::ZhHans, Self::En];
}

/// 中文表（`键 \t 文案`）。
const ZH_HANS_TSV: &str = include_str!("../assets/i18n/zh-Hans.tsv");
/// 英文表。
const EN_TSV: &str = include_str!("../assets/i18n/en.tsv");

/// 编译进来的原始表。
pub fn raw_table(lang: Lang) -> &'static str {
    match lang {
        Lang::ZhHans => ZH_HANS_TSV,
        Lang::En => EN_TSV,
    }
}

/// 解析一张 TSV 表。
///
/// 规则：`#` 开头的行与空行忽略；每行第一个制表符之前是键，之后是文案。
/// 文案里允许再有制表符（会保留）。
pub fn parse_table(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('\t') else {
            continue; // 没有制表符的行不是条目
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.push((key.to_owned(), value.trim().to_owned()));
    }
    out
}

/// 查一条文案。键不存在时**原样返回键**（明显的 bug 信号，但不崩）。
pub fn translate(lang: Lang, key: &str) -> String {
    for (candidate, value) in parse_table(raw_table(lang)) {
        if candidate == key {
            return value;
        }
    }
    key.to_owned()
}

/// 某个语言里的全部键（升序、去重）。
pub fn keys(lang: Lang) -> Vec<String> {
    let mut out: Vec<String> = parse_table(raw_table(lang)).into_iter().map(|(key, _)| key).collect();
    out.sort();
    out.dedup();
    out
}

/// 缺键检测：返回"某个语言里有、另一个语言里没有"的键。
///
/// 同时也报告**重复键**（同一个键在同一张表里出现两次 —— 后者会静默覆盖前者）。
pub fn check() -> Report {
    let mut report = Report::default();
    for lang in Lang::ALL {
        let entries = parse_table(raw_table(lang));
        let mut seen = std::collections::BTreeSet::new();
        for (key, _) in &entries {
            if !seen.insert(key.clone()) {
                report.duplicates.push(format!("{}: {key}", lang.code()));
            }
        }
        report.counts.push((lang.code().to_owned(), seen.len()));
    }

    let zh = keys(Lang::ZhHans);
    let en = keys(Lang::En);
    for key in &zh {
        if !en.contains(key) {
            report.missing_in_en.push(key.clone());
        }
    }
    for key in &en {
        if !zh.contains(key) {
            report.missing_in_zh.push(key.clone());
        }
    }
    report
}

/// 检查结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// 每个语言的条目数。
    pub counts: Vec<(String, usize)>,
    /// 中文有、英文没有。
    pub missing_in_en: Vec<String>,
    /// 英文有、中文没有。
    pub missing_in_zh: Vec<String>,
    /// 重复键。
    pub duplicates: Vec<String>,
}

impl Report {
    /// 有问题吗。
    pub fn is_ok(&self) -> bool {
        self.missing_in_en.is_empty() && self.missing_in_zh.is_empty() && self.duplicates.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_parses_the_common_forms() {
        for value in ["zh", "zh-CN", "zh-Hans", "zh_CN", "Zh"] {
            assert_eq!(Lang::parse(value), Some(Lang::ZhHans), "{value}");
        }
        for value in ["en", "en-US", "EN", "en_GB"] {
            assert_eq!(Lang::parse(value), Some(Lang::En), "{value}");
        }
        for value in ["", "fr", "de-DE", "  "] {
            assert_eq!(Lang::parse(value), None, "{value}");
        }
    }

    #[test]
    fn lang_codes_are_the_tsv_column_names() {
        assert_eq!(Lang::ZhHans.code(), "zh-Hans");
        assert_eq!(Lang::En.code(), "en");
    }

    #[test]
    fn table_parsing_skips_comments_and_blank_lines() {
        let text = "# 注释\n\nstate.status.title\t查看状态\n\n# 又一行注释\n";
        let entries = parse_table(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "state.status.title");
        assert_eq!(entries[0].1, "查看状态");
    }

    #[test]
    fn table_parsing_keeps_tabs_inside_the_value() {
        let entries = parse_table("key\ta\tb\n");
        assert_eq!(entries[0].1, "a\tb");
    }

    #[test]
    fn table_parsing_ignores_lines_without_a_tab() {
        assert!(parse_table("no tab here\n").is_empty());
    }

    #[test]
    fn translate_returns_the_value_when_present() {
        // 用一张确定存在的内置键。
        let value = translate(Lang::ZhHans, "state.status.title");
        assert_ne!(value, "state.status.title", "中文表里应该有这一条");
        assert!(!value.is_empty());
    }

    #[test]
    fn translate_falls_back_to_the_key() {
        // 缺键回退到键本身：界面会显示 `no.such.key`，一眼看得出是 bug，但不会崩。
        assert_eq!(translate(Lang::ZhHans, "no.such.key"), "no.such.key");
        assert_eq!(translate(Lang::En, "no.such.key"), "no.such.key");
    }

    #[test]
    fn both_tables_parse_to_something() {
        for lang in Lang::ALL {
            assert!(!keys(lang).is_empty(), "{} 表是空的", lang.code());
        }
    }

    #[test]
    fn there_are_no_missing_or_duplicate_keys() {
        // 这是 i18n 的验收线：两张表的键必须完全一致，且没有重复。
        let report = check();
        assert!(report.missing_in_en.is_empty(), "英文缺这些键：{:?}", report.missing_in_en);
        assert!(report.missing_in_zh.is_empty(), "中文缺这些键：{:?}", report.missing_in_zh);
        assert!(report.duplicates.is_empty(), "重复键：{:?}", report.duplicates);
        assert!(report.is_ok());
    }

    #[test]
    fn english_and_chinese_differ_for_real_sentences() {
        // 防止"英文表其实是复制粘贴的中文"。
        let zh = translate(Lang::ZhHans, "state.status.title");
        let en = translate(Lang::En, "state.status.title");
        assert_ne!(zh, en, "中英两表不该完全一样");
        assert!(en.is_ascii(), "英文文案应当只有 ASCII：{en}");
    }
}
