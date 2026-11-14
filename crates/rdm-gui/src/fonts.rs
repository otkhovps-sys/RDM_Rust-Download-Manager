//! Adds a system font as fallback so Korean, Chinese, Japanese and other
//! non-Latin file names render. Nothing is bundled, keeping the binary small.

use std::sync::Arc;

const CANDIDATES: &[&str] = &[
    // Windows
    "C:\\Windows\\Fonts\\malgun.ttf",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    // macOS
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    // Linux
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
];

pub fn install(ctx: &egui::Context) {
    let Some(bytes) = CANDIDATES.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "system-fallback".into(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in fonts.families.values_mut() {
        family.push("system-fallback".into());
    }
    ctx.set_fonts(fonts);
}
