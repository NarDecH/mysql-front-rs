//! Thai-capable font loading for the GUI.
//!
//! egui's bundled fonts have no Thai glyphs, so we look for a system font
//! that does (Windows: Leelawadee UI / Nirmala UI / Tahoma) and register it
//! as a fallback for both the proportional and monospace families.

use std::path::PathBuf;

use egui::FontDefinitions;

const CANDIDATES: &[&str] = &[
    // Windows Thai UI fonts, best first.
    "leelawui.ttf",
    "LeelawUI.ttf",
    "Nirmala.ttf",
    "tahoma.ttf",
    "Tahoma.ttf",
    "segoeui.ttf",
    "segoeui.ttf.bak",
    // Linux/Mac fallbacks that occasionally carry Thai.
    "NotoSansThai-Regular.ttf",
    "Norasi.ttf",
    "Garuda.ttf",
    "Thonburi.ttf",
];

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from(r"C:\Windows\Fonts"),
        PathBuf::from("/usr/share/fonts/truetype"),
        PathBuf::from("/System/Library/Fonts"),
    ];
    if let Some(fonts) = dirs::font_dir() {
        dirs.push(fonts);
    }
    dirs
}

/// Load the first existing Thai-capable font and add it as a fallback.
/// Returns the font file name when one was found.
pub fn install_fonts(ctx: &egui::Context) -> Option<String> {
    let mut found: Option<(String, Vec<u8>)> = None;
    'outer: for dir in font_dirs() {
        for name in CANDIDATES {
            let path = dir.join(name);
            if path.is_file() {
                if let Ok(bytes) = std::fs::read(&path) {
                    if !bytes.is_empty() {
                        found = Some((name.to_string(), bytes));
                        break 'outer;
                    }
                }
            }
        }
    }
    let (name, bytes) = found?;

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "thai".to_owned(),
        egui::FontData::from_owned(bytes).into(),
    );
    // Last in the fallback chain: only used when the earlier fonts miss a glyph.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("thai".to_owned());
    }
    ctx.set_fonts(fonts);
    Some(name)
}
