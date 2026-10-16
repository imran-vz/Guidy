//! Embedded assets: fonts, theme, release notes, plus the full Lucide icon
//! set from gpui-kit.

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets"]
#[include = "fonts/**/*.ttf"]
#[include = "themes/*.json"]
#[include = "release-notes/*.md"]
#[include = "icons/**/*.svg"]
pub struct Embedded;

/// App assets first, then gpui-kit's full Lucide catalog.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(f) = Embedded::get(path) {
            return Ok(Some(f.data));
        }
        match gpui_kit::assets::AllAssets.load(path) {
            Ok(v) => Ok(v),
            Err(_) => Ok(None),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_kit::assets::AllAssets.list(path).unwrap_or_default();
        v.extend(
            Embedded::iter()
                .filter(|p| p.starts_with(path))
                .map(|p| SharedString::from(p.to_string())),
        );
        Ok(v)
    }
}

pub fn text(path: &str) -> Option<String> {
    Embedded::get(path).map(|f| String::from_utf8_lossy(&f.data).into_owned())
}

/// Bundled release notes as (version, markdown).
pub fn release_notes() -> Vec<(String, String)> {
    let mut notes: Vec<(String, String)> = Embedded::iter()
        .filter_map(|p| {
            let name = p
                .strip_prefix("release-notes/")?
                .strip_suffix(".md")?
                .to_string();
            let body = text(&p)?;
            Some((name, body))
        })
        .collect();
    notes.sort();
    notes
}

pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Embedded::iter()
        .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
        .filter_map(|p| Embedded::get(&p).map(|f| f.data))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_fonts_theme_and_notes() {
        assert!(fonts().len() >= 5);
        assert!(text("themes/zed-one.json").unwrap().contains("One Dark"));
        let notes = release_notes();
        assert!(notes.iter().any(|(v, _)| v == "0.1.0"));
    }

    #[test]
    fn falls_back_to_kit_icons() {
        assert!(AppAssets.load("icons/mic.svg").unwrap().is_some());
        assert!(AppAssets
            .load("icons/definitely-missing.svg")
            .unwrap()
            .is_none());
    }
}
