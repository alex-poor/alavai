// The icon set is an asset library; some glyphs are reserved for screens still
// being built (mullvad chevrons, add-tailnet, close buttons, …).
#![allow(dead_code)]

//! Bundled symbolic icons (Lucide, ISC-licensed) rendered as themeable SVGs.
//!
//! There's no system source for app UI icons, so we bundle a small set. The
//! SVGs use `stroke="currentColor"`, and iced's `svg` widget recolours the whole
//! glyph via [`svg::Style::color`], so one asset works on light and dark themes.

use iced::widget::svg;
use iced::{Color, Element};

macro_rules! icons {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(pub const $name: &[u8] = include_bytes!(concat!("../assets/icons/", $file));)*
    };
}

icons! {
    MONITOR => "monitor.svg",
    LAPTOP => "laptop.svg",
    GLOBE => "globe.svg",
    SEARCH => "search.svg",
    COPY => "copy.svg",
    EXTERNAL => "external-link.svg",
    CHECK => "check.svg",
    CHEVRON => "chevron-right.svg",
    CLOSE => "x.svg",
    PLUS => "plus.svg",
    SHIELD => "shield.svg",
    PIN => "map-pin.svg",
    POWER => "power.svg",
    REFRESH => "refresh-cw.svg",
}

/// Builds a square, single-colour icon at the given pixel size.
pub fn icon<'a, Message: 'a>(bytes: &'static [u8], size: f32, color: Color) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}
