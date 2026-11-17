//! Small pieces every screen shares. Style only; behaviour stays with the
//! screen that owns it.

use gpui::*;

use crate::theme;

/// The lynx mark, from the same geometry as `x-public.mark` on the web: head,
/// inner face, ear tufts, two eyes and the nose, in a 100 × 100 box.
pub fn mark(size: f32) -> Div {
    div().flex_shrink_0().child(
        canvas(
            move |_bounds, _window, _cx| (),
            move |bounds, _state, window, _cx| {
                for (path, colour) in lynx(bounds) {
                    window.paint_path(path, colour);
                }
            },
        )
        .size(px(size)),
    )
}

/// The mark's shapes in paint order, in the colours the web gives them.
fn lynx(bounds: Bounds<Pixels>) -> Vec<(Path<Pixels>, Rgba)> {
    let scale = f32::from(bounds.size.width) / 100.0;
    let at = move |x: f32, y: f32| {
        point(
            bounds.origin.x + px(x * scale),
            bounds.origin.y + px(y * scale),
        )
    };

    let head = [
        (59., 36.),
        (76., 17.),
        (82., 46.),
        (88., 63.),
        (76., 87.),
        (61., 79.),
        (39., 79.),
        (24., 87.),
        (12., 63.),
        (18., 46.),
        (24., 17.),
        (41., 36.),
    ];

    let face = [
        (61.9, 42.5),
        (72.2, 30.9),
        (75.7, 47.7),
        (81., 62.6),
        (73.2, 78.1),
        (62.6, 72.5),
        (37.4, 72.5),
        (26.8, 78.1),
        (19., 62.6),
        (24.3, 47.7),
        (27.8, 30.9),
        (38.1, 42.5),
    ];

    let mut layers = Vec::new();
    let mut add = |path: Option<Path<Pixels>>, colour: Rgba| {
        if let Some(path) = path {
            layers.push((path, colour));
        }
    };

    add(polygon(&at, &head), theme::brand_outline());
    add(polygon(&at, &face), theme::brand_inner());

    for tuft in [
        [(22.9, 22.2), (26.8, 3.), (29.6, 23.3)],
        [(77.1, 22.2), (73.2, 3.), (70.4, 23.3)],
    ] {
        add(polygon(&at, &tuft), theme::brand_outline());
    }

    for eye in [(37.5, 54.), (62.5, 54.)] {
        add(disc(&at, eye.0, eye.1, 3.4), theme::brand_outline());
    }

    add(
        polygon(&at, &[(46.2, 66.), (53.8, 66.), (50., 71.)]),
        theme::brand_outline(),
    );

    layers
}

fn polygon(at: &impl Fn(f32, f32) -> Point<Pixels>, points: &[(f32, f32)]) -> Option<Path<Pixels>> {
    let mut builder = PathBuilder::fill();

    for (index, (x, y)) in points.iter().enumerate() {
        if index == 0 {
            builder.move_to(at(*x, *y));
        } else {
            builder.line_to(at(*x, *y));
        }
    }

    builder.close();
    builder.build().ok()
}

/// A filled circle, as a polygon: the eyes are 3.4 units across in a 100 unit
/// box, so twenty segments are already rounder than the pixels.
fn disc(
    at: &impl Fn(f32, f32) -> Point<Pixels>,
    cx: f32,
    cy: f32,
    radius: f32,
) -> Option<Path<Pixels>> {
    let points = (0..20)
        .map(|step| {
            let angle = step as f32 / 20.0 * std::f32::consts::TAU;

            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect::<Vec<_>>();

    polygon(at, &points)
}

pub fn wordmark() -> &'static str {
    "Lynxby"
}

/// Which organisation this window is acting in. The deployment's name for it,
/// never a local one: white-label themes make it the tenant's brand.
pub fn organisation(name: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .text_color(theme::text_muted())
        .child(name.into())
}

/// A primary action. The caller adds `.on_click()`.
pub fn primary_button(id: impl Into<ElementId>, label: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .px_5()
        .py_2()
        .rounded_lg()
        .bg(theme::primary())
        .text_color(theme::primary_foreground())
        .text_sm()
        .font_weight(theme::SEMIBOLD)
        .cursor_pointer()
        .hover(|style| style.bg(theme::primary_hover()))
        .child(label)
}

/// A secondary action on a dark surface.
pub fn ghost_button(id: impl Into<ElementId>, label: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme::border())
        .text_color(theme::text_muted())
        .text_xs()
        .hover(|style| {
            style
                .border_color(theme::border_strong())
                .text_color(theme::text())
        })
        .child(label)
}

pub fn pill(label: impl Into<SharedString>, tint: Rgba) -> Div {
    div()
        .px_2()
        .py_0p5()
        .rounded_full()
        .bg(theme::surface_sunken())
        .text_color(tint)
        .text_xs()
        .child(label.into())
}

pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(theme::surface_raised())
        .border_1()
        .border_color(theme::border())
}

pub fn label(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(theme::SEMIBOLD)
        .text_color(theme::text_subtle())
        .child(text.into())
}

pub fn error_box(message: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .p_3()
        .rounded_lg()
        .bg(theme::danger_surface())
        .text_color(theme::danger())
        .text_sm()
        .child(message.into())
}

pub fn info_box(message: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .p_3()
        .rounded_lg()
        .bg(theme::surface_sunken())
        .border_1()
        .border_color(theme::border())
        .text_color(theme::text_subtle())
        .text_sm()
        .child(message.into())
}

#[cfg(test)]
mod tests {
    use super::lynx;
    use crate::theme;
    use gpui::{point, px, size, Bounds, Pixels};

    fn boxed(edge: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(edge), px(edge)),
        }
    }

    #[test]
    fn the_mark_is_the_whole_animal_in_both_colours() {
        let layers = lynx(boxed(100.));

        // Head, face, two tufts, two eyes, nose.
        assert_eq!(layers.len(), 7);
        assert_eq!(layers[1].1, theme::brand_inner());
        assert_eq!(layers[0].1, theme::brand_outline());
    }

    #[test]
    fn every_size_draws_every_shape() {
        for edge in [16., 28., 44., 280.] {
            assert_eq!(lynx(boxed(edge)).len(), 7, "at {edge}px");
        }
    }
}
