use gpui::*;

use crate::app::{LynxbyApp, Notice};
use crate::theme;
use crate::widgets;

pub fn render(app: &mut LynxbyApp, cx: &mut Context<LynxbyApp>) -> Div {
    let working = app.working();

    let status = match app.notice() {
        Some(Notice::Info(message)) => message.clone(),
        _ => {
            "A browser window opens on this machine and asks you to approve. Nothing is typed here."
                .to_string()
        }
    };

    let error = match app.notice() {
        Some(Notice::Error(message)) => Some(message.clone()),
        _ => None,
    };

    let button = if working {
        widgets::primary_button("sign-in", "Continue in browser")
            .w_full()
            .bg(theme::surface_sunken())
            .text_color(theme::text_subtle())
    } else {
        widgets::primary_button("sign-in", "Continue in browser")
            .w_full()
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.sign_in(cx)))
    };

    let cancel: AnyElement = if working {
        widgets::ghost_button("cancel", "Cancel")
            .w_full()
            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.cancel_sign_in(cx)))
            .into_any_element()
    } else {
        div().h(px(0.)).into_any_element()
    };

    let card = widgets::card()
        .w_full()
        .max_w(px(460.))
        .p_8()
        .flex()
        .flex_col()
        .items_center()
        .gap_5()
        .child(widgets::mark(44.))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_xl()
                        .font_weight(theme::SEMIBOLD)
                        .child(widgets::wordmark()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_subtle())
                        .child("Sign in to continue"),
                ),
        )
        .child(
            div()
                .max_w(px(340.))
                .text_sm()
                .text_color(theme::text_muted())
                .text_center()
                .whitespace_normal()
                .child(format!(
                    "Your assistants, models and tools come from {}. Your password, second factor and single sign-on stay in your browser.",
                    app.config().host()
                )),
        )
        .children(
            error
                .map(|message| widgets::error_box(message).into_any_element()),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .child(button)
                .child(
                    div()
                        .min_h(px(34.))
                        .px_2()
                        .text_xs()
                        .text_color(theme::text_subtle())
                        .text_center()
                        .whitespace_normal()
                        .child(status),
                )
                .child(cancel),
        );

    let footer = div()
        .w_full()
        .max_w(px(460.))
        .px_1()
        .flex()
        .justify_between()
        .text_xs()
        .text_color(theme::text_subtle())
        .child(format!(
            "{} · {}",
            app.config().environment.label(),
            app.config().host()
        ))
        .child(crate::app::version_line());

    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .p_6()
        .child(card)
        .child(footer)
}
