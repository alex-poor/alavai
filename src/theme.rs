// This is the design-system module: it intentionally defines the full token and
// style set from the spec, some of which is consumed by screens still being
// built (switcher manage mode, account avatars, warning/danger surfaces).
#![allow(dead_code)]

//! Design tokens and reusable widget styles, from docs/design/DESIGN.md.
//!
//! The design specifies a full light + dark palette. We keep the tokens in a
//! `Palette` (all `Copy` colours) and pass it into `view` so style closures can
//! capture it. Built-in widgets (text inputs, pick lists, scrollbars) follow the
//! base `iced::Theme`; our cards/rows/pills are styled explicitly from these
//! tokens so they match the spec on either theme.

use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Theme};

/// Resolved colour tokens for one theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub raised: Color,
    pub line: Color,
    pub text: Color,
    pub text2: Color,
    pub text3: Color,
    pub accent: Color,
    pub accent_bg: Color,
    pub online: Color,
    pub offline: Color,
    pub exit: Color,
    pub exit_bg: Color,
    pub warn: Color,
    pub danger: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a)
}

pub fn dark() -> Palette {
    Palette {
        bg: rgb(0x14, 0x18, 0x1E),
        surface: rgb(0x1B, 0x21, 0x2A),
        raised: rgb(0x23, 0x2B, 0x36),
        line: rgb(0x2C, 0x35, 0x42),
        text: rgb(0xE9, 0xED, 0xF3),
        text2: rgb(0x98, 0xA2, 0xB3),
        text3: rgb(0x6A, 0x74, 0x85),
        accent: rgb(0x4D, 0x8D, 0xF5),
        accent_bg: rgba(0x4D, 0x8D, 0xF5, 0.13),
        online: rgb(0x34, 0xD3, 0x99),
        offline: rgb(0x5B, 0x65, 0x73),
        exit: rgb(0xA7, 0x8B, 0xFA),
        exit_bg: rgba(0xA7, 0x8B, 0xFA, 0.16),
        warn: rgb(0xFB, 0xBF, 0x24),
        danger: rgb(0xF8, 0x71, 0x71),
    }
}

pub fn light() -> Palette {
    Palette {
        bg: rgb(0xF4, 0xF6, 0xF9),
        surface: rgb(0xFF, 0xFF, 0xFF),
        raised: rgb(0xEE, 0xF1, 0xF5),
        line: rgb(0xE4, 0xE8, 0xEE),
        text: rgb(0x16, 0x1B, 0x22),
        text2: rgb(0x59, 0x61, 0x6E),
        text3: rgb(0x89, 0x90, 0x99),
        accent: rgb(0x25, 0x63, 0xEB),
        accent_bg: rgba(0x25, 0x63, 0xEB, 0.10),
        online: rgb(0x05, 0x96, 0x69),
        offline: rgb(0xAA, 0xB1, 0xBB),
        exit: rgb(0x7C, 0x3A, 0xED),
        exit_bg: rgba(0x7C, 0x3A, 0xED, 0.12),
        warn: rgb(0xB4, 0x53, 0x09),
        danger: rgb(0xDC, 0x26, 0x26),
    }
}

impl Palette {
    /// The matching base iced theme (for built-in widget chrome).
    pub fn base(&self, dark: bool) -> Theme {
        if dark { Theme::Dark } else { Theme::Light }
    }
}

// --- Container styles (each takes the palette by value via a capturing closure) ---

/// Window/detail background.
pub fn window(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.bg)),
        text_color: Some(p.text),
        ..Default::default()
    }
}

/// Sidebar surface with a hairline right border.
pub fn sidebar(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.surface)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// A rounded card (surface + hairline border).
pub fn card(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.surface)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

/// Header bar (surface + bottom hairline).
pub fn header(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.surface)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// A selected sidebar row (accent tint).
pub fn selected_row(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.accent_bg)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    }
}

/// A status/info pill, tinted by `accent`.
pub fn pill(bg: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        },
        ..Default::default()
    }
}

/// The switcher chip / secondary surface (raised + border).
pub fn chip(p: Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.raised)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

/// A small square avatar filled with `color`.
pub fn avatar(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

// --- Button styles ---

/// Primary (accent fill) button.
pub fn primary_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => lighten(p.accent, 0.08),
            _ => p.accent,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Secondary (raised) button.
pub fn secondary_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.line,
            _ => p.raised,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text,
            border: Border {
                color: p.line,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Danger (destructive) button.
pub fn danger_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => lighten(p.danger, 0.08),
            _ => p.danger,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        }
    }
}

/// A transparent ghost/list-row button (used for sidebar rows).
pub fn row_btn(p: Palette, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = if selected {
            Some(Background::Color(p.accent_bg))
        } else if matches!(status, button::Status::Hovered) {
            Some(Background::Color(p.raised))
        } else {
            None
        };
        button::Style {
            background: bg,
            text_color: p.text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 7.0.into(),
            },
            ..Default::default()
        }
    }
}

/// A small icon/copy button (hairline border).
pub fn small_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => p.raised,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text2,
            border: Border {
                color: p.line,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    }
}

/// Text input style matching the tokens.
pub fn input(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let border_color = match status {
            text_input::Status::Focused { .. } => p.accent,
            _ => p.line,
        };
        text_input::Style {
            background: Background::Color(p.raised),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 7.0.into(),
            },
            icon: p.text3,
            placeholder: p.text3,
            value: p.text,
            selection: p.accent_bg,
        }
    }
}

fn lighten(c: Color, amount: f32) -> Color {
    Color {
        r: (c.r + amount).min(1.0),
        g: (c.g + amount).min(1.0),
        b: (c.b + amount).min(1.0),
        a: c.a,
    }
}

/// A deterministic per-account avatar colour (blue / green / violet / amber …).
pub fn account_color(p: Palette, index: usize) -> Color {
    const_colors(p)[index % 4]
}

fn const_colors(p: Palette) -> [Color; 4] {
    [p.accent, p.online, p.exit, p.warn]
}
