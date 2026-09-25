//! Script font fallbacks for the windowed front-end.
//!
//! egui's bundled fonts cover Latin, Cyrillic, Greek and emoji, but no Indic
//! (or Arabic, Hebrew, CJK, …) scripts. Anything they lack falls back to
//! `.notdef`, which is the empty box (tofu) you see for e.g. Tamil.
//!
//! egui 0.36 shapes text properly — it uses HarfBuzz via `harfrust` — so the
//! only missing piece is a font that actually contains the glyphs. This module
//! locates a suitable *system* font and registers it as a fallback in egui's
//! font definitions, leaving the built-in fonts in place for everything else.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};

/// Font files to try for Tamil, in order of preference.
///
/// Covers macOS, Linux and Windows; if none of these exist, the directories in
/// [`FONT_DIRS`] are scanned for a file whose name contains `tamil`.
const TAMIL_CANDIDATES: &[&str] = &[
    // macOS: a user-installed Noto, then the fonts macOS always ships.
    "~/Library/Fonts/Noto Sans Tamil.ttf",
    "/Library/Fonts/Noto Sans Tamil.ttf",
    "/System/Library/Fonts/Supplemental/Tamil MN.ttc",
    "/System/Library/Fonts/Supplemental/Tamil Sangam MN.ttc",
    // Linux.
    "/usr/share/fonts/truetype/noto/NotoSansTamil-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansTamilUI-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansTamil-Regular.otf",
    "/usr/share/fonts/truetype/lohit-tamil/Lohit-Tamil.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSerif.ttf",
    // Windows.
    "C:/Windows/Fonts/Nirmala.ttf",
    "C:/Windows/Fonts/NirmalaS.ttf",
    "C:/Windows/Fonts/latha.ttf",
];

/// Directories searched for a `*tamil*` font file, recursively but shallowly.
const FONT_DIRS: &[&str] = &[
    "/System/Library/Fonts",
    "/Library/Fonts",
    "~/Library/Fonts",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "C:/Windows/Fonts",
];

/// How deep to recurse when scanning [`FONT_DIRS`].
const SCAN_DEPTH: usize = 4;

/// Register a Tamil fallback font, if one can be found on this system.
pub fn install_tamil_fallback(ctx: &eframe::egui::Context) {
    let Some((_path, bytes)) = load_tamil_font() else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("tamil".to_owned(), Arc::new(FontData::from_owned(bytes)));

    // Appended, not prepended: the built-in fonts still handle Latin, and the
    // Tamil font picks up only the characters they are missing.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("tamil".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// Find and read a Tamil-capable font file.
pub fn load_tamil_font() -> Option<(PathBuf, Vec<u8>)> {
    let path = find_tamil_font()?;
    let bytes = std::fs::read(&path).ok()?;
    Some((path, bytes))
}

/// Find a Tamil-capable font file on this system.
pub fn find_tamil_font() -> Option<PathBuf> {
    for candidate in TAMIL_CANDIDATES {
        let path = expand(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    for dir in FONT_DIRS {
        if let Some(path) = scan_for(&expand(dir), "tamil", SCAN_DEPTH) {
            return Some(path);
        }
    }
    None
}

/// Recursively look for a font file whose name contains `needle`.
fn scan_for(dir: &Path, needle: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if is_font_file(&path) && name_contains(&path, needle) {
            return Some(path);
        }
    }
    for subdir in subdirs {
        if let Some(found) = scan_for(&subdir, needle, depth - 1) {
            return Some(found);
        }
    }
    None
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

fn name_contains(path: &Path, needle: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

/// Expand a leading `~` to the user's home directory.
fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first four bytes of a valid sfnt font: TrueType, OpenType/CFF, or a
    /// TrueType/OpenType collection.
    fn has_font_signature(bytes: &[u8]) -> bool {
        matches!(
            bytes.get(..4),
            Some([0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"ttcf" | b"true" | b"typ1")
        )
    }

    #[test]
    fn finds_a_real_font_file() {
        let Some(path) = find_tamil_font() else {
            eprintln!("no Tamil font installed; nothing to check");
            return;
        };
        let bytes = std::fs::read(&path).expect("the discovered font should be readable");
        assert!(bytes.len() > 1024, "{} looks truncated", path.display());
        assert!(
            has_font_signature(&bytes),
            "{} is not an sfnt font",
            path.display()
        );
    }

    #[test]
    fn scan_accepts_only_font_extensions() {
        assert!(is_font_file(Path::new("/tmp/NotoSansTamil-Regular.ttf")));
        assert!(is_font_file(Path::new("/tmp/Tamil MN.ttc")));
        assert!(!is_font_file(Path::new("/tmp/readme.txt")));
        assert!(!is_font_file(Path::new("/tmp/fonts")));
        assert!(name_contains(Path::new("/x/Tamil MN.ttc"), "tamil"));
        assert!(!name_contains(Path::new("/x/Helvetica.ttc"), "tamil"));
    }
}
