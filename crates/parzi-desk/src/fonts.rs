//! Bundled faces (OFL, licences next to the files) loaded once into
//! `FontDefinitions`. Every face is hinted and subpixel-binned; weights come
//! from the variable axes, so one Inter file serves body, label and heading.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};
use epaint::text::{FontTweak, VariationCoords};

pub const INTER: &str = "inter";
pub const INTER_MEDIUM: &str = "inter-medium";
pub const INTER_SEMIBOLD: &str = "inter-semibold";
pub const MONO: &str = "jetbrains-mono";
pub const SERIF_ITALIC: &str = "instrument-serif-italic";

static INTER_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter[opsz,wght].ttf");
static MONO_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono[wght].ttf");
static SERIF_BYTES: &[u8] = include_bytes!("../assets/fonts/InstrumentSerif-Italic.ttf");

/// Optical size for Inter, pinned at the body size: tighter spacing and a
/// taller x-height than the display cut. Per-run overrides go through
/// `TextFormat::coords` once the transcript renderer exists (M1).
const INTER_OPSZ: f32 = 14.0;

/// A named family for one of our faces (`FontFamily::Name`).
pub fn family(name: &'static str) -> FontFamily {
    FontFamily::Name(name.into())
}

fn tweak(coords: VariationCoords) -> FontTweak {
    FontTweak {
        hinting: Some(true),
        subpixel_binning: Some(true),
        coords,
        ..FontTweak::default()
    }
}

fn inter(weight: f32) -> FontData {
    FontData::from_static(INTER_BYTES).tweak(tweak(VariationCoords::new([
        (b"wght", weight),
        (b"opsz", INTER_OPSZ),
    ])))
}

/// Our faces first; the fonts egui bundles stay behind them as glyph
/// fallback for symbols the three faces do not cover.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts
        .font_data
        .insert(INTER.to_owned(), Arc::new(inter(400.0)));
    fonts
        .font_data
        .insert(INTER_MEDIUM.to_owned(), Arc::new(inter(500.0)));
    fonts
        .font_data
        .insert(INTER_SEMIBOLD.to_owned(), Arc::new(inter(600.0)));
    fonts.font_data.insert(
        MONO.to_owned(),
        Arc::new(
            FontData::from_static(MONO_BYTES)
                .tweak(tweak(VariationCoords::new([(b"wght", 400.0)]))),
        ),
    );
    fonts.font_data.insert(
        SERIF_ITALIC.to_owned(),
        Arc::new(FontData::from_static(SERIF_BYTES).tweak(tweak(VariationCoords::default()))),
    );

    prepend(&mut fonts, FontFamily::Proportional, INTER);
    prepend(&mut fonts, FontFamily::Monospace, MONO);

    let proportional_chain = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for name in [INTER_MEDIUM, INTER_SEMIBOLD, SERIF_ITALIC] {
        let mut chain = Vec::with_capacity(proportional_chain.len() + 1);
        chain.push(name.to_owned());
        chain.extend(proportional_chain.iter().cloned());
        fonts.families.insert(family(name), chain);
    }
    fonts
}

fn prepend(fonts: &mut FontDefinitions, family: FontFamily, name: &str) {
    fonts
        .families
        .entry(family)
        .or_default()
        .insert(0, name.to_owned());
}
