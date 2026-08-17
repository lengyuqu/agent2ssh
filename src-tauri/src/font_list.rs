//! System font enumeration for terminal font selection.
//!
//! Uses `fontdb` to scan installed system fonts and return a deduplicated
//! list of font families with monospace detection. Mirrors rssh's
//! `commands/settings.rs:list_fonts()` pattern.

use serde::{Deserialize, Serialize};

/// A system font family entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    /// Font family name (e.g. "Consolas", "Menlo").
    pub family: String,
    /// Whether the font is monospaced (suitable for terminal use).
    pub monospaced: bool,
}

/// Monospace font family names that are known to be fixed-width.
/// Used as a fallback heuristic when fontdb's monospace flag is unreliable.
const KNOWN_MONOSPACE: &[&str] = &[
    "consolas",
    "menlo",
    "monaco",
    "courier new",
    "courier",
    "monospace",
    "lucida console",
    "andale mono",
    "dejavu sans mono",
    "liberation mono",
    "noto sans mono",
    "noto mono",
    "ubuntu mono",
    "cascadia code",
    "cascadia mono",
    "jetbrains mono",
    "fira code",
    "fira mono",
    "source code pro",
    "sf mono",
    "sfmono-regular",
    "inconsolata",
    "roboto mono",
    "ibm plex mono",
    "hack",
    "meslo",
    "meslo lg",
    "sauce code pro",
    "anonymous pro",
    "go mono",
    "droid sans mono",
    "pt mono",
    "cousine",
    "nanum gothic coding",
    "nanumgothiccoding",
];

/// Enumerate all system font families. Deduplicates by family name and
/// marks known monospace families.
///
/// Runs in a blocking context — callers should wrap in `spawn_blocking`
/// when called from async code.
pub fn list_fonts() -> Vec<FontInfo> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut families: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // Collect all unique family names.
    db.faces().for_each(|face| {
        for (family, _lang) in &face.families {
            families.insert(family.clone());
        }
    });

    families
        .into_iter()
        .map(|family| {
            let lower = family.to_lowercase();
            let monospaced = KNOWN_MONOSPACE.contains(&lower.as_str())
                || lower.contains("mono")
                || lower.contains("consol")
                || lower.contains("fixed");
            FontInfo {
                family,
                monospaced,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_fonts_returns_non_empty() {
        let fonts = list_fonts();
        // On any real system there should be at least one font.
        assert!(!fonts.is_empty(), "system should have at least one font");
    }

    #[test]
    fn list_fonts_deduplicates_families() {
        let fonts = list_fonts();
        let mut families: Vec<&str> = fonts.iter().map(|f| f.family.as_str()).collect();
        families.sort();
        let before = families.len();
        families.dedup();
        assert_eq!(before, families.len(), "no duplicate family names");
    }

    #[test]
    fn known_monospace_fonts_detected() {
        let fonts = list_fonts();
        // At least one known monospace font should exist on the system
        // (Consolas on Windows, Menlo on macOS, DejaVu Sans Mono on Linux).
        let has_mono = fonts.iter().any(|f| f.monospaced);
        assert!(has_mono, "system should have at least one monospace font");
    }
}
