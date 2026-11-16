//! Uses the system's fonts so every language renders - Vietnamese, Korean,
//! Chinese, Japanese and so on. Nothing is bundled, keeping the binary small.
//!
//! egui's built-in font lacks many accented letters (most of Vietnamese), so
//! a complete system font becomes the main text font; fonts for other
//! scripts are added behind it as fallbacks.

use std::sync::Arc;

use egui::FontFamily;

/// Fonts with full Latin coverage (including Vietnamese); the first one
/// found is used for all interface text.
const TEXT_FONTS: &[&str] = &[
    // Windows
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\tahoma.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    // macOS
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    // Linux
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

/// Fonts for other scripts. Every group contributes its first font that
/// exists, so Korean, Chinese and Japanese are all covered where installed.
const SCRIPT_FONTS: &[&[&str]] = &[
    // Korean
    &[
        "C:\\Windows\\Fonts\\malgun.ttf",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ],
    // Chinese (these also carry Japanese kana and kanji)
    &[
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
    ],
];

fn first_existing(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|path| std::fs::read(path).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |name: &str, bytes: Vec<u8>| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    };

    let text = first_existing(TEXT_FONTS).map(|bytes| add("system-text", bytes));
    let scripts: Vec<String> = SCRIPT_FONTS
        .iter()
        .enumerate()
        .filter_map(|(i, group)| {
            let name = format!("system-script-{i}");
            first_existing(group)
                .map(|bytes| add(&name, bytes))
                .map(|()| name)
        })
        .collect();

    for (family, names) in &mut fonts.families {
        if text.is_some() {
            // Main font for normal text; for the monospace family only a
            // fallback, so paths keep their fixed-width look.
            match family {
                FontFamily::Proportional => names.insert(0, "system-text".to_owned()),
                _ => names.push("system-text".to_owned()),
            }
        }
        names.extend(scripts.iter().cloned());
    }
    ctx.set_fonts(fonts);
}
