//! Colours from the product's own design tokens (`resources/css/app.css`,
//! dark theme), converted to sRGB once so the window matches the web app.

use gpui::{rgb, FontWeight, Rgba};

pub fn surface() -> Rgba {
    rgb(0x12303b)
}

pub fn surface_raised() -> Rgba {
    rgb(0x173a46)
}

pub fn surface_sunken() -> Rgba {
    rgb(0x0d252e)
}

pub fn border() -> Rgba {
    rgb(0x2b4a55)
}

pub fn border_strong() -> Rgba {
    rgb(0x6d90a0)
}

pub fn text() -> Rgba {
    rgb(0xeef7fa)
}

pub fn text_muted() -> Rgba {
    rgb(0xbfe3f2)
}

pub fn text_subtle() -> Rgba {
    rgb(0xa8ccd9)
}

pub fn primary() -> Rgba {
    rgb(0xbfe3f2)
}

pub fn primary_hover() -> Rgba {
    rgb(0xe1f2f8)
}

pub fn primary_foreground() -> Rgba {
    rgb(0x12303b)
}

/// The lynx mark's outline and inner shape, from `public.css` (`--color-glass`
/// and `--color-petrol`): the mark is two-tone, so it cannot take one tint.
pub fn brand_outline() -> Rgba {
    rgb(0xbfe3f2)
}

pub fn brand_inner() -> Rgba {
    rgb(0x12303b)
}

pub fn accent() -> Rgba {
    rgb(0xffb489)
}

pub fn accent_foreground() -> Rgba {
    rgb(0xffc9a6)
}

pub fn danger() -> Rgba {
    rgb(0xf09a93)
}

pub fn danger_surface() -> Rgba {
    rgb(0x3c1d1b)
}

pub fn success() -> Rgba {
    rgb(0x7fcaae)
}

pub fn accent_surface() -> Rgba {
    rgb(0x3b2a20)
}

pub const SEMIBOLD: FontWeight = FontWeight::SEMIBOLD;
