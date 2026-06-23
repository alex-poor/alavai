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
    WARN => "triangle-alert.svg",
    ACTIVITY => "activity.svg",
    WIFI_OFF => "wifi-off.svg",
}

/// Builds a square, single-colour icon at the given pixel size.
pub fn icon<'a, Message: 'a>(bytes: &'static [u8], size: f32, color: Color) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

// --- Brand mesh icons (rasterized for the tray + window) ---

pub const TRAY_CONNECTED: &[u8] = include_bytes!("../assets/icons/tray-connected.svg");
pub const TRAY_DISCONNECTED: &[u8] = include_bytes!("../assets/icons/tray-disconnected.svg");
pub const TRAY_EXIT: &[u8] = include_bytes!("../assets/icons/tray-exit.svg");

/// Rasterizes an SVG into a square buffer. Returns `(size, size, pixels)`.
/// `argb` selects ARGB32 byte order (for the SNI tray) vs RGBA (window icon).
fn rasterize(svg: &[u8], size: u32, argb: bool) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let ts = tree.size();
    let scale = size as f32 / ts.width().max(ts.height());
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        if argb {
            data.extend_from_slice(&[c.alpha(), c.red(), c.green(), c.blue()]);
        } else {
            data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
    }
    Some((size, size, data))
}

/// ARGB32 buffer for an SNI tray icon.
pub fn render_argb(svg: &[u8], size: u32) -> Option<(i32, i32, Vec<u8>)> {
    rasterize(svg, size, true).map(|(w, h, d)| (w as i32, h as i32, d))
}

/// RGBA buffer for a window icon.
pub fn render_rgba(svg: &[u8], size: u32) -> Option<(u32, u32, Vec<u8>)> {
    rasterize(svg, size, false)
}
