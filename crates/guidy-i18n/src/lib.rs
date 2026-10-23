//! Translations for Guidy.
//!
//! Reads i18next `translation.json` files with i18next semantics: dotted keys, `{{var}}` interpolation, `_one` /
//! `_other` plural suffixes selected by a `count` argument, fallback to
//! English, and the key itself when nothing matches.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

pub const FALLBACK_LANGUAGE: &str = "en";

/// Display metadata per locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageMeta {
    pub code: &'static str,
    pub name: &'static str,
    pub native_name: &'static str,
    pub priority: u32,
    pub rtl: bool,
}

pub const LANGUAGE_METADATA: &[LanguageMeta] = &[
    LanguageMeta {
        code: "en",
        name: "English",
        native_name: "English",
        priority: 1,
        rtl: false,
    },
    LanguageMeta {
        code: "zh",
        name: "Simplified Chinese",
        native_name: "简体中文",
        priority: 2,
        rtl: false,
    },
    LanguageMeta {
        code: "zh-TW",
        name: "Traditional Chinese",
        native_name: "繁體中文",
        priority: 3,
        rtl: false,
    },
    LanguageMeta {
        code: "es",
        name: "Spanish",
        native_name: "Español",
        priority: 4,
        rtl: false,
    },
    LanguageMeta {
        code: "fr",
        name: "French",
        native_name: "Français",
        priority: 5,
        rtl: false,
    },
    LanguageMeta {
        code: "de",
        name: "German",
        native_name: "Deutsch",
        priority: 6,
        rtl: false,
    },
    LanguageMeta {
        code: "ja",
        name: "Japanese",
        native_name: "日本語",
        priority: 7,
        rtl: false,
    },
    LanguageMeta {
        code: "ko",
        name: "Korean",
        native_name: "한국어",
        priority: 8,
        rtl: false,
    },
    LanguageMeta {
        code: "vi",
        name: "Vietnamese",
        native_name: "Tiếng Việt",
        priority: 9,
        rtl: false,
    },
    LanguageMeta {
        code: "pl",
        name: "Polish",
        native_name: "Polski",
        priority: 10,
        rtl: false,
    },
    LanguageMeta {
        code: "it",
        name: "Italian",
        native_name: "Italiano",
        priority: 11,
        rtl: false,
    },
    LanguageMeta {
        code: "ru",
        name: "Russian",
        native_name: "Русский",
        priority: 12,
        rtl: false,
    },
    LanguageMeta {
        code: "uk",
        name: "Ukrainian",
        native_name: "Українська",
        priority: 13,
        rtl: false,
    },
    LanguageMeta {
        code: "pt",
        name: "Portuguese",
        native_name: "Português",
        priority: 14,
        rtl: false,
    },
    LanguageMeta {
        code: "cs",
        name: "Czech",
        native_name: "Čeština",
        priority: 15,
        rtl: false,
    },
    LanguageMeta {
        code: "tr",
        name: "Turkish",
        native_name: "Türkçe",
        priority: 16,
        rtl: false,
    },
    LanguageMeta {
        code: "ar",
        name: "Arabic",
        native_name: "العربية",
        priority: 17,
        rtl: true,
    },
    LanguageMeta {
        code: "he",
        name: "Hebrew",
        native_name: "עברית",
        priority: 18,
        rtl: true,
    },
    LanguageMeta {
        code: "sv",
        name: "Swedish",
        native_name: "Svenska",
        priority: 19,
        rtl: false,
    },
    LanguageMeta {
        code: "bg",
        name: "Bulgarian",
        native_name: "Български",
        priority: 20,
        rtl: false,
    },
    LanguageMeta {
        code: "nl",
        name: "Dutch",
        native_name: "Nederlands",
        priority: 21,
        rtl: false,
    },
    LanguageMeta {
        code: "ne",
        name: "Nepali",
        native_name: "नेपाली",
        priority: 22,
        rtl: false,
    },
    LanguageMeta {
        code: "hi",
        name: "Hindi",
        native_name: "हिन्दी",
        priority: 23,
        rtl: false,
    },
    LanguageMeta {
        code: "da",
        name: "Danish",
        native_name: "Dansk",
        priority: 24,
        rtl: false,
    },
    LanguageMeta {
        code: "ca",
        name: "Catalan",
        native_name: "Català",
        priority: 25,
        rtl: false,
    },
    LanguageMeta {
        code: "id",
        name: "Indonesian",
        native_name: "Bahasa Indonesia",
        priority: 26,
        rtl: false,
    },
];

type Catalog = HashMap<String, String>;

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    static CATALOGS: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        LOCALE_FILES
            .iter()
            .map(|(code, json)| {
                let value: serde_json::Value =
                    serde_json::from_str(json).unwrap_or_else(|e| panic!("locale {code}: {e}"));
                let mut flat = HashMap::new();
                flatten("", &value, &mut flat);
                (*code, flat)
            })
            .collect()
    })
}

fn flatten(prefix: &str, value: &serde_json::Value, out: &mut Catalog) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

/// All flattened keys of a locale (used by the parity checker).
pub fn keys(code: &str) -> Vec<String> {
    let mut keys: Vec<String> = catalogs()
        .get(code)
        .map(|c| c.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// Locale codes present on disk, sorted by priority then English name.
pub fn supported_languages() -> Vec<LanguageMeta> {
    let mut langs: Vec<LanguageMeta> = LOCALE_FILES
        .iter()
        .map(|(code, _)| {
            LANGUAGE_METADATA
                .iter()
                .find(|m| m.code == *code)
                .copied()
                .unwrap_or(LanguageMeta {
                    code,
                    name: code,
                    native_name: code,
                    priority: u32::MAX,
                    rtl: false,
                })
        })
        .collect();
    langs.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(b.name)));
    langs
}

/// Map an arbitrary locale tag onto a supported locale: exact match, then the base language, with
/// Traditional Chinese for zh-Hant / TW / HK / MO and Cantonese.
pub fn get_supported_language(tag: &str) -> Option<&'static str> {
    if tag.is_empty() {
        return None;
    }
    let normalized = tag.to_lowercase().replace('_', "-");
    let subtags: Vec<&str> = normalized.split('-').collect();
    let language = subtags[0];
    let is_hant = subtags.contains(&"hant");
    let is_hans = subtags.contains(&"hans");
    let traditional_region = ["tw", "hk", "mo"].iter().any(|r| subtags.contains(r));

    let find = |code: &str| {
        LOCALE_FILES
            .iter()
            .map(|(c, _)| *c)
            .find(|c| c.to_lowercase() == code)
    };
    if let Some(code) = find(&normalized) {
        return Some(code);
    }
    let fallback = if language == "zh" && (is_hant || (!is_hans && traditional_region)) {
        "zh-tw"
    } else if language == "yue" {
        if is_hans {
            "zh"
        } else {
            "zh-tw"
        }
    } else {
        language
    };
    find(fallback)
}

pub fn is_rtl(code: &str) -> bool {
    let base = code.split('-').next().unwrap_or("").to_lowercase();
    LANGUAGE_METADATA.iter().any(|m| m.code == base && m.rtl)
}

fn current() -> &'static RwLock<String> {
    static CURRENT: OnceLock<RwLock<String>> = OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(FALLBACK_LANGUAGE.to_string()))
}

/// Switch the active language (unsupported tags fall back to English).
pub fn set_language(tag: &str) {
    let code = get_supported_language(tag).unwrap_or(FALLBACK_LANGUAGE);
    *current().write() = code.to_string();
}

pub fn language() -> String {
    current().read().clone()
}

/// Languages whose CLDR plural rules have no singular form (i18next always
/// uses `_other` for them).
const NO_SINGULAR: &[&str] = &["ja", "ko", "zh", "zh-TW", "vi", "id"];

fn plural_suffix(lang: &str, count: f64) -> &'static str {
    if NO_SINGULAR.contains(&lang) {
        return "other";
    }
    match lang {
        // French (and Portuguese in CLDR) treat 0 and 1 as singular.
        "fr" | "pt" if count == 0.0 || count == 1.0 => "one",
        _ if count == 1.0 => "one",
        _ => "other",
    }
}

fn lookup(lang: &str, key: &str) -> Option<&'static String> {
    catalogs().get(lang).and_then(|c| c.get(key))
}

fn resolve(lang: &str, key: &str, count: Option<f64>) -> Option<&'static String> {
    if let Some(count) = count {
        let suffixed = format!("{key}_{}", plural_suffix(lang, count));
        if let Some(s) = lookup(lang, &suffixed) {
            return Some(s);
        }
    }
    lookup(lang, key)
}

/// Translate `key` in `lang` with `{{name}}` arguments.
pub fn translate_in(lang: &str, key: &str, args: &[(&str, &str)]) -> String {
    let count = args
        .iter()
        .find(|(k, _)| *k == "count")
        .and_then(|(_, v)| v.parse::<f64>().ok());
    let template = resolve(lang, key, count)
        .or_else(|| resolve(FALLBACK_LANGUAGE, key, count))
        .map(|s| s.as_str())
        .unwrap_or(key);
    interpolate(template, args)
}

fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if !template.contains("{{") {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Translate in the active language.
pub fn t(key: &str) -> String {
    translate_in(&language(), key, &[])
}

/// Translate in the active language with arguments.
pub fn t_args(key: &str, args: &[(&str, &str)]) -> String {
    translate_in(&language(), key, args)
}

/// Whether `key` exists (in the active language or English).
pub fn exists(key: &str) -> bool {
    lookup(&language(), key).is_some() || lookup(FALLBACK_LANGUAGE, key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_26_locales_are_embedded_with_metadata() {
        assert_eq!(LOCALE_FILES.len(), 26);
        for (code, _) in LOCALE_FILES {
            assert!(
                LANGUAGE_METADATA.iter().any(|m| m.code == *code),
                "missing metadata for {code}"
            );
        }
        let langs = supported_languages();
        assert_eq!(langs[0].code, "en");
        assert_eq!(langs[1].code, "zh");
        assert_eq!(langs[2].code, "zh-TW");
    }

    #[test]
    fn english_has_the_full_key_set() {
        assert_eq!(keys("en").len(), 908);
    }

    #[test]
    fn interpolates_and_keeps_unknown_placeholders() {
        assert_eq!(interpolate("v{{version}}!", &[("version", "1.2")]), "v1.2!");
        assert_eq!(interpolate("{{a}} {{b}}", &[("a", "x")]), "x {{b}}");
        assert_eq!(interpolate("open {{", &[]), "open {{");
    }

    #[test]
    fn falls_back_to_english_then_key() {
        assert_eq!(translate_in("de", "no.such.key", &[]), "no.such.key");
        assert_eq!(
            translate_in("en", "whatsNew.title", &[("version", "0.1.0")]),
            "New in Guidy v0.1.0"
        );
    }

    #[test]
    fn plurals_select_one_and_other() {
        let en = catalogs().get("en").unwrap();
        let (key, _) = en
            .iter()
            .find(|(k, _)| k.ends_with("_one"))
            .expect("en has plural keys");
        let base = key.trim_end_matches("_one");
        let one = translate_in("en", base, &[("count", "1")]);
        let other = translate_in("en", base, &[("count", "3")]);
        assert_eq!(&one, &interpolate(&en[key], &[("count", "1")]));
        assert_eq!(
            &other,
            &interpolate(&en[&format!("{base}_other")], &[("count", "3")])
        );
        assert_eq!(plural_suffix("ja", 1.0), "other");
        assert_eq!(plural_suffix("fr", 0.0), "one");
    }

    #[test]
    fn language_resolution_follows_locale_rules() {
        assert_eq!(get_supported_language("en-US"), Some("en"));
        assert_eq!(get_supported_language("de_DE"), Some("de"));
        assert_eq!(get_supported_language("zh-TW"), Some("zh-TW"));
        assert_eq!(get_supported_language("zh-Hant"), Some("zh-TW"));
        assert_eq!(get_supported_language("zh-HK"), Some("zh-TW"));
        assert_eq!(get_supported_language("zh-Hans-HK"), Some("zh"));
        assert_eq!(get_supported_language("zh-CN"), Some("zh"));
        assert_eq!(get_supported_language("yue"), Some("zh-TW"));
        assert_eq!(get_supported_language("yue-Hans"), Some("zh"));
        assert_eq!(get_supported_language("xx"), None);
        assert_eq!(get_supported_language(""), None);
    }

    #[test]
    fn rtl_languages() {
        assert!(is_rtl("ar"));
        assert!(is_rtl("he-IL"));
        assert!(!is_rtl("en"));
    }

    #[test]
    fn locales_have_no_brand_leftovers_and_match_english_keys() {
        let en: std::collections::BTreeSet<String> = keys("en").into_iter().collect();
        for (code, json) in LOCALE_FILES {
            assert!(!json.contains("Handy"), "{code} still mentions Handy");
            let k: std::collections::BTreeSet<String> = keys(code).into_iter().collect();
            let extra: Vec<_> = k.difference(&en).collect();
            assert!(
                extra.is_empty(),
                "{code} has keys missing from en: {extra:?}"
            );
        }
    }
}
