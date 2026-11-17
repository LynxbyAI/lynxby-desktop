use gpui::*;

use crate::app::LynxbyApp;
use crate::theme;
use crate::widgets;

pub fn render(app: &LynxbyApp, _cx: &mut Context<LynxbyApp>) -> Div {
    let dots = ".".repeat(1 + app.splash_frame() % 3);

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_6()
        .child(widgets::mark(96.))
        .child(
            div()
                .text_3xl()
                .font_weight(theme::SEMIBOLD)
                .child(widgets::wordmark()),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme::text_subtle())
                .child(format!("Desktop{dots}")),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme::text_subtle())
                .child(app.config().host()),
        )
}
