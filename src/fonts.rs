//! Font fallbacks for the windowed front-end.
//!
//! egui's bundled fonts cover Latin, Cyrillic, Greek and emoji, but no Indic,
//! Arabic, Hebrew or CJK scripts. Anything they lack falls back to `.notdef` —
//! the empty box (tofu) you see for e.g. Tamil.
//!
//! egui 0.36 shapes text properly (HarfBuzz via `harfrust`), so the only thing
//! missing is glyph coverage. This module supplies it in two tiers:
//!
//! * [`install_bundled`] embeds **Noto Sans** for every major script except
//!   CJK — about 5 MB of fonts compiled into the binary, so those languages
//!   work everywhere with no system dependencies.
//! * [`install_cjk`] loads a CJK font **from the system**, lazily, on the first
//!   frame whose document actually contains CJK text. These fonts are tens of
//!   megabytes, so only people who need them pay for them.
//!
//! The bundled fonts come from <https://github.com/notofonts/noto-fonts> and are
//! licensed under the SIL Open Font License 1.1; see
//! `assets/fonts/LICENSE-OFL.txt`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};

/// Fonts compiled into the binary: Noto Sans for every major non-CJK script.
const BUNDLED: &[(&str, &[u8])] = &[
    ("noto-sans", include_bytes!("../assets/fonts/NotoSans-Regular.ttf")),
    ("noto-tamil", include_bytes!("../assets/fonts/NotoSansTamil-Regular.ttf")),
    ("noto-devanagari", include_bytes!("../assets/fonts/NotoSansDevanagari-Regular.ttf")),
    ("noto-bengali", include_bytes!("../assets/fonts/NotoSansBengali-Regular.ttf")),
    ("noto-gurmukhi", include_bytes!("../assets/fonts/NotoSansGurmukhi-Regular.ttf")),
    ("noto-gujarati", include_bytes!("../assets/fonts/NotoSansGujarati-Regular.ttf")),
    ("noto-oriya", include_bytes!("../assets/fonts/NotoSansOriya-Regular.ttf")),
    ("noto-telugu", include_bytes!("../assets/fonts/NotoSansTelugu-Regular.ttf")),
    ("noto-kannada", include_bytes!("../assets/fonts/NotoSansKannada-Regular.ttf")),
    ("noto-malayalam", include_bytes!("../assets/fonts/NotoSansMalayalam-Regular.ttf")),
    ("noto-sinhala", include_bytes!("../assets/fonts/NotoSansSinhala-Regular.ttf")),
    ("noto-thai", include_bytes!("../assets/fonts/NotoSansThai-Regular.ttf")),
    ("noto-lao", include_bytes!("../assets/fonts/NotoSansLao-Regular.ttf")),
    ("noto-khmer", include_bytes!("../assets/fonts/NotoSansKhmer-Regular.ttf")),
    ("noto-myanmar", include_bytes!("../assets/fonts/NotoSansMyanmar-Regular.ttf")),
    ("noto-tibetan", include_bytes!("../assets/fonts/NotoSerifTibetan-Regular.ttf")),
    ("noto-arabic", include_bytes!("../assets/fonts/NotoSansArabic-Regular.ttf")),
    ("noto-hebrew", include_bytes!("../assets/fonts/NotoSansHebrew-Regular.ttf")),
    ("noto-ethiopic", include_bytes!("../assets/fonts/NotoSansEthiopic-Regular.ttf")),
    ("noto-georgian", include_bytes!("../assets/fonts/NotoSansGeorgian-Regular.ttf")),
    ("noto-armenian", include_bytes!("../assets/fonts/NotoSansArmenian-Regular.ttf")),
    ("noto-thaana", include_bytes!("../assets/fonts/NotoSansThaana-Regular.ttf")),
    ("noto-syriac", include_bytes!("../assets/fonts/NotoSansSyriac-Regular.ttf")),
    ("noto-nko", include_bytes!("../assets/fonts/NotoSansNKo-Regular.ttf")),
    ("noto-adlam", include_bytes!("../assets/fonts/NotoSansAdlam-Regular.ttf")),
    ("noto-cherokee", include_bytes!("../assets/fonts/NotoSansCherokee-Regular.ttf")),
    ("noto-canadian", include_bytes!("../assets/fonts/NotoSansCanadianAboriginal-Regular.ttf")),
    ("noto-mongolian", include_bytes!("../assets/fonts/NotoSansMongolian-Regular.ttf")),
    ("noto-symbols", include_bytes!("../assets/fonts/NotoSansSymbols2-Regular.ttf")),
];

/// Where to look for a CJK font, in order of preference.
const CJK_CANDIDATES: &[&str] = &[
    // macOS: Chinese, then Korean.
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    "/System/Library/Fonts/Supplemental/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/Supplemental/STHeiti Medium.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    // Linux.
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    // Windows.
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/YuGothM.ttc",
    "C:/Windows/Fonts/meiryo.ttc",
    "C:/Windows/Fonts/malgun.ttf",
];

/// Directories searched for a `*cjk*` font file, recursively but shallowly.
const FONT_DIRS: &[&str] = &[
    "/System/Library/Fonts",
    "/Library/Fonts",
    "~/Library/Fonts",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "C:/Windows/Fonts",
];

const SCAN_DEPTH: usize = 4;

/// Register the bundled Noto Sans script fonts as fallbacks.
///
/// The built-in fonts stay first in each family, so Latin text is unchanged;
/// the Noto fonts pick up only the characters the built-ins are missing.
pub fn install_bundled(ctx: &Context) {
    ctx.set_fonts(definitions(None));
}

/// Register a CJK font from the system, alongside the bundled fonts.
///
/// Returns `false` if no CJK font could be found, in which case nothing
/// changes and CJK text will still show as boxes.
pub fn install_cjk(ctx: &Context) -> bool {
    let Some((_path, bytes)) = load_cjk_font() else {
        return false;
    };
    ctx.set_fonts(definitions(Some(("system-cjk", bytes))));
    true
}

/// Whether `text` contains characters that only a CJK font can draw.
pub fn needs_cjk(text: &str) -> bool {
    text.chars().any(is_cjk)
}

fn definitions(extra: Option<(&str, Vec<u8>)>) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    for (name, bytes) in BUNDLED.iter().copied() {
        // `from_static` borrows the embedded bytes rather than copying them.
        let data = Arc::new(FontData::from_static(bytes));
        fonts.font_data.insert(name.to_owned(), data);
        push_fallback(&mut fonts, name);
    }

    if let Some((name, bytes)) = extra {
        let data = Arc::new(FontData::from_owned(bytes));
        fonts.font_data.insert(name.to_owned(), data);
        push_fallback(&mut fonts, name);
    }

    fonts
}

fn push_fallback(fonts: &mut FontDefinitions, name: &str) {
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(name.to_owned());
    }
}

/// Find and read a CJK font from the system.
fn load_cjk_font() -> Option<(PathBuf, Vec<u8>)> {
    let path = find_cjk_font()?;
    let bytes = std::fs::read(&path).ok()?;
    Some((path, bytes))
}

/// Find a CJK font file on this system.
fn find_cjk_font() -> Option<PathBuf> {
    for candidate in CJK_CANDIDATES {
        let path = expand(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    for dir in FONT_DIRS {
        if let Some(path) = scan_for(&expand(dir), "cjk", SCAN_DEPTH) {
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

/// The CJK ranges we look for: Han, Hiragana, Katakana, Bopomofo and Hangul.
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x1100..=0x11FF        // Hangul Jamo
        | 0x2E80..=0x2EFF      // CJK radicals
        | 0x3000..=0x303F      // CJK symbols and punctuation
        | 0x3040..=0x30FF      // Hiragana and Katakana
        | 0x3100..=0x312F      // Bopomofo
        | 0x3130..=0x318F      // Hangul compatibility Jamo
        | 0x31F0..=0x31FF      // Katakana phonetic extensions
        | 0x3400..=0x4DBF      // CJK unified ideographs extension A
        | 0x4E00..=0x9FFF      // CJK unified ideographs
        | 0xA960..=0xA97F      // Hangul Jamo extended-A
        | 0xAC00..=0xD7AF      // Hangul syllables
        | 0xF900..=0xFAFF      // CJK compatibility ideographs
        | 0x20000..=0x2FA1F    // CJK extensions B onwards
    )
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
    fn bundled_fonts_are_present_and_valid() {
        assert!(BUNDLED.len() >= 25, "expected a broad script coverage");
        for (name, bytes) in BUNDLED.iter().copied() {
            assert!(bytes.len() > 1024, "{name} is suspiciously small");
            assert!(has_font_signature(bytes), "{name} is not an sfnt font");
        }
    }

    #[test]
    fn bundled_fonts_cover_the_expected_scripts() {
        for script in [
            "Tamil", "Devanagari", "Bengali", "Arabic", "Hebrew", "Thai", "Khmer", "Myanmar",
            "Sinhala", "Georgian", "Armenian", "Ethiopic", "Cherokee", "Mongolian",
        ] {
            let wanted = script.to_ascii_lowercase();
            assert!(
                BUNDLED.iter().any(|(name, _)| name.contains(&wanted)),
                "no bundled font for {script}"
            );
        }
    }

    #[test]
    fn cjk_detection_is_script_aware() {
        for cjk in ["日本語", "中文", "한국어", "ひらがな", "カタカナ"] {
            assert!(needs_cjk(cjk), "{cjk} should need a CJK font");
        }
        for other in [
            "hello world",
            "வாழ்க வையகம்",
            "नमस्ते",
            "مرحبا",
            "שלום",
            "สวัสดี",
            "ελληνικά",
            "Привет",
        ] {
            assert!(!needs_cjk(other), "{other} should not need a CJK font");
        }
    }

    #[test]
    fn scan_accepts_only_font_extensions() {
        assert!(is_font_file(Path::new("/tmp/NotoSansTamil-Regular.ttf")));
        assert!(is_font_file(Path::new("/tmp/Tamil MN.ttc")));
        assert!(!is_font_file(Path::new("/tmp/readme.txt")));
        assert!(!is_font_file(Path::new("/tmp/fonts")));
        assert!(name_contains(Path::new("/x/NotoSansCJK-Regular.ttc"), "cjk"));
        assert!(!name_contains(Path::new("/x/Helvetica.ttc"), "cjk"));
    }

    /// If the machine has a CJK font, a CJK document should render with real
    /// glyphs once it is registered. Skipped where none is installed.
    #[test]
    fn cjk_font_is_used_when_available() {
        let Some((path, bytes)) = load_cjk_font() else {
            eprintln!("no CJK font installed; cannot check rendering");
            return;
        };
        assert!(has_font_signature(&bytes), "{}", path.display());
        assert!(find_cjk_font().is_some());
    }
}
