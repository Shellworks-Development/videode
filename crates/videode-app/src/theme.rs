//! The "Test Card" design system (see DESIGN.md at the repo root).
//!
//! Surfaces are PLUGE near-black steps. The seven 75%-bar colours belong to
//! node categories only and never to chrome.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Pixels, px, rgb, rgba};

pub const FONT: &str = "Geist";
pub const MONO: &str = "Geist Mono";

/// The only two shell type sizes; keycaps use [`KEYCAP`].
pub const TEXT: Pixels = px(12.);
pub const TITLE: Pixels = px(13.);
pub const KEYCAP: Pixels = px(11.);

pub mod hex {
    pub const PLUGE_SUB: u32 = 0x09090a;
    pub const PLUGE_BLACK: u32 = 0x111113;
    pub const PLUGE_RAISED: u32 = 0x19191c;
    pub const PLUGE_CONTROL: u32 = 0x222226;
    pub const PLUGE_HOVER: u32 = 0x2b2b30;
    pub const LINE: u32 = 0x2a2a2f;
    pub const LINE_STRONG: u32 = 0x3a3a41;
    pub const INK: u32 = 0xe8e7e2;
    pub const INK_DIM: u32 = 0xa3a29d;
    pub const INK_FAINT: u32 = 0x8a8984;
    pub const BONE: u32 = 0xf1ede0;
    pub const WARN: u32 = 0xe0a03a;
    pub const ERROR: u32 = 0xe05a4f;
}

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub use videode_core::Category;

/// Node categories each own one 75% SMPTE bar colour.
pub trait CategoryExt {
    fn color(self) -> Hsla;
    fn label(self) -> &'static str;
}

impl CategoryExt for Category {
    fn color(self) -> Hsla {
        c(match self {
            Category::Source => 0xbdbdb8,
            Category::Transform => 0xc9c43c,
            Category::Look => 0x3cc4c4,
            Category::Composite => 0x45c45a,
            Category::Audio => 0xc43cc4,
            Category::Text => 0xd0463f,
            Category::Output => 0x4a62d0,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Category::Source => "Source",
            Category::Transform => "Transform",
            Category::Look => "Color & filter",
            Category::Composite => "Composite",
            Category::Audio => "Audio",
            Category::Text => "Text",
            Category::Output => "Output",
        }
    }
}

pub const ALL_CATEGORIES: [Category; 7] = [
    Category::Source,
    Category::Transform,
    Category::Look,
    Category::Composite,
    Category::Audio,
    Category::Text,
    Category::Output,
];

/// Points GPUI Kit's component theme at the Test Card palette so menus,
/// inputs and popovers from the kit match the shell.
pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        t.font_family = FONT.into();
        t.mono_font_family = MONO.into();
        t.font_size = TEXT;
        t.mono_font_size = TEXT;
        t.radius = px(5.);
        t.radius_lg = px(10.);

        t.background = c(hex::PLUGE_BLACK);
        t.foreground = c(hex::INK);
        t.border = c(hex::LINE);
        t.muted = c(hex::PLUGE_RAISED);
        t.muted_foreground = c(hex::INK_DIM);
        t.popover = c(hex::PLUGE_RAISED);
        t.popover_foreground = c(hex::INK);
        t.accent = c(hex::PLUGE_HOVER);
        t.accent_foreground = c(hex::INK);
        t.input = c(hex::LINE_STRONG);
        t.ring = c(hex::BONE);
        t.caret = c(hex::BONE);
        t.selection = rgba(0xf1ede040).into();
        t.primary = c(hex::BONE);
        t.primary_hover = c(0xfffaf0);
        t.primary_active = c(hex::BONE);
        t.primary_foreground = c(hex::PLUGE_SUB);
        t.secondary = c(hex::PLUGE_RAISED);
        t.secondary_hover = c(hex::PLUGE_CONTROL);
        t.secondary_active = c(hex::PLUGE_HOVER);
        t.secondary_foreground = c(hex::INK);
        t.title_bar = c(hex::PLUGE_BLACK);
        t.title_bar_border = c(hex::LINE);
        t.status_bar = c(hex::PLUGE_BLACK);
        t.status_bar_border = c(hex::LINE);
        t.scrollbar_thumb = c(hex::PLUGE_HOVER);
        t.scrollbar_thumb_hover = c(hex::LINE_STRONG);
        t.warning = c(hex::WARN);
        t.danger = c(hex::ERROR);
    });
}
