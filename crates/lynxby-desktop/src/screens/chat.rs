use gpui::*;

use crate::app::{LynxbyApp, Turn};
use crate::theme;
use crate::widgets;

pub fn render(app: &mut LynxbyApp, window: &mut Window, cx: &mut Context<LynxbyApp>) -> Div {
    let Some(chat) = app.chat() else {
        return div();
    };

    let assistant_name = chat.assistant.name.clone();
    let model = chat
        .assistant
        .model
        .clone()
        .unwrap_or_else(|| "routed by Lynxby".to_string());
    let streaming = chat.streaming;
    let loading = chat.loading;
    let error = chat.error.clone();
    let turns = chat.turns.clone();
    let session = chat.session_id.clone();
    let organisation = app
        .me()
        .map(|me| me.organisation_name())
        .unwrap_or_default();
    let focused = app.input().is_focused(window);
    let composer_empty = app.composer().is_empty();
    let composer_text = app.composer().text().to_string();

    let messages = turns.iter().map(turn_view).collect::<Vec<_>>();

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header(&assistant_name, &model, &organisation, cx))
        .child(
            div()
                .id("messages")
                .flex_1()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(app.scroll())
                .child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .mx_auto()
                        .p_6()
                        .flex()
                        .flex_col()
                        .gap_6()
                        .children(messages)
                        .children(loading.then(|| {
                            widgets::info_box("Loading this conversation…").into_any_element()
                        }))
                        .children(
                            error.map(|message| widgets::error_box(message).into_any_element()),
                        )
                        .child(
                            div()
                                .pt_2()
                                .text_xs()
                                .text_color(theme::text_subtle())
                                .child(format!(
                                    "Session {session} · usage is recorded on your organisation"
                                )),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .border_t_1()
                .border_color(theme::border())
                .bg(theme::surface_raised())
                .child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .mx_auto()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_end()
                                .gap_3()
                                .p_3()
                                .rounded_xl()
                                .border_1()
                                .border_color(if focused {
                                    theme::border_strong()
                                } else {
                                    theme::border()
                                })
                                .bg(theme::surface_sunken())
                                .id("composer")
                                .track_focus(app.input())
                                .on_key_down(cx.listener(
                                    |this, event: &KeyDownEvent, _window, cx| {
                                        this.on_key(event, cx)
                                    },
                                ))
                                .cursor_text()
                                .on_click(cx.listener(|this, _: &ClickEvent, window, _cx| {
                                    window.focus(this.input());
                                }))
                                .child(composer_row(focused, composer_empty, composer_text))
                                .child(send_button(streaming, composer_empty, cx)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_subtle())
                                .child("Enter to send · Shift+Enter for a new line"),
                        ),
                ),
        )
}

fn header(assistant: &str, model: &str, organisation: &str, cx: &mut Context<LynxbyApp>) -> Div {
    div()
        .w_full()
        .h(px(64.))
        .px_6()
        .flex()
        .items_center()
        .justify_between()
        .bg(theme::surface_raised())
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(widgets::ghost_button("back", "← Assistants").on_click(
                    cx.listener(|this, _: &ClickEvent, _window, cx| this.back_to_assistants(cx)),
                ))
                .child(
                    div()
                        .text_lg()
                        .font_weight(theme::SEMIBOLD)
                        .child(assistant.to_string()),
                )
                .child(widgets::pill(model.to_string(), theme::accent_foreground())),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .children((!organisation.is_empty()).then(|| {
                    widgets::organisation(organisation.to_string())
                        .text_xs()
                        .into_any_element()
                }))
                .child(
                    widgets::ghost_button("new-chat", "New chat").on_click(
                        cx.listener(|this, _: &ClickEvent, _window, cx| this.new_chat(cx)),
                    ),
                ),
        )
}

/// The text and the cursor, in the order they are read: the cursor comes before
/// the placeholder while the box is empty, and after the text once there is any.
fn composer_row(focused: bool, empty: bool, text: String) -> Div {
    let body = div()
        .min_h(px(22.))
        .text_sm()
        .line_height(px(22.))
        .text_color(if empty {
            theme::text_subtle()
        } else {
            theme::text()
        })
        .whitespace_normal()
        .child(if empty {
            "Ask this assistant…".to_string()
        } else {
            text
        });

    let row = div().flex_1().flex().items_center();

    if empty {
        row.child(caret(focused)).child(body)
    } else {
        row.child(body).child(caret(focused))
    }
}

/// The text cursor: right after the text, where the next character goes.
fn caret(focused: bool) -> Div {
    div()
        .w(px(6.))
        .flex()
        .justify_end()
        .child(div().w(px(2.)).h(px(18.)).rounded(px(1.)).bg(if focused {
            theme::accent()
        } else {
            theme::surface_sunken()
        }))
}

fn send_button(streaming: bool, empty: bool, cx: &mut Context<LynxbyApp>) -> Stateful<Div> {
    let label = if streaming { "…" } else { "Send" };

    let element = div()
        .id("send")
        .px_4()
        .py_2()
        .rounded_lg()
        .text_sm()
        .font_weight(theme::SEMIBOLD)
        .child(label.to_string());

    let element = if empty || streaming {
        element
            .bg(theme::surface())
            .text_color(theme::text_subtle())
    } else {
        element
            .bg(theme::primary())
            .text_color(theme::primary_foreground())
            .hover(|style| style.bg(theme::primary_hover()))
    };

    element
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.send(cx);
            window.focus(this.input());
        }))
}

fn turn_view(turn: &Turn) -> Div {
    let mut block = div().flex().flex_col().gap_3().w_full();

    block = block.child(
        div().w_full().flex().justify_end().child(
            div()
                .max_w(px(560.))
                .px_4()
                .py_3()
                .rounded_xl()
                .bg(theme::surface_raised())
                .text_sm()
                .whitespace_normal()
                .child(turn.user.clone()),
        ),
    );

    let mut answer = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(theme::border())
        .bg(theme::surface_sunken());

    if !turn.tools.is_empty() {
        let mut chips = div().flex().flex_wrap().gap_2();

        for tool in &turn.tools {
            chips = chips.child(widgets::pill(tool.clone(), theme::accent_foreground()));
        }

        answer = answer.child(chips);
    }

    if !turn.thinking.is_empty() {
        answer = answer.child(
            div()
                .text_xs()
                .italic()
                .text_color(theme::text_subtle())
                .whitespace_normal()
                .child(turn.thinking.clone()),
        );
    }

    if !turn.answer.is_empty() {
        answer = answer.child(
            div()
                .text_sm()
                .line_height(px(22.))
                .whitespace_normal()
                .child(turn.answer.clone()),
        );
    }

    if let Some(error) = &turn.error {
        answer = answer.child(
            div()
                .text_sm()
                .text_color(theme::danger())
                .child(format!("Could not finish: {error}")),
        );
    }

    if !turn.citations.is_empty() {
        let mut sources = div()
            .flex()
            .flex_col()
            .gap_1()
            .mt_2()
            .pt_3()
            .border_t_1()
            .border_color(theme::border())
            .child(widgets::label("SOURCES"));

        for citation in &turn.citations {
            let document = if citation.document.is_empty() {
                "Knowledge base".to_string()
            } else {
                citation.document.clone()
            };

            let mut row = div().flex().flex_col().gap_0p5().pt_2().child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(document),
            );

            if !citation.quote.is_empty() {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme::text_subtle())
                        .whitespace_normal()
                        .child(format!("“{}”", citation.quote.trim())),
                );
            }

            sources = sources.child(row);
        }

        answer = answer.child(sources);
    }

    if turn.answer.is_empty() && turn.error.is_none() {
        answer = answer.child(
            div()
                .text_sm()
                .text_color(theme::text_subtle())
                .child("Thinking…"),
        );
    }

    let mut footer = div()
        .flex()
        .items_center()
        .gap_3()
        .text_xs()
        .text_color(theme::text_subtle());

    if let Some(model) = &turn.model {
        footer = footer.child(model.clone());
    }

    if let Some(tokens) = turn.tokens {
        footer = footer.child(format!("{tokens} tokens"));
    }

    block.child(answer).child(footer)
}
