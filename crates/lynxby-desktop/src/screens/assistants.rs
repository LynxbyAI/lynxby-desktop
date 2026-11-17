use gpui::*;

use crate::app::{LynxbyApp, Notice};
use crate::theme;
use crate::widgets;

pub fn render(app: &mut LynxbyApp, cx: &mut Context<LynxbyApp>) -> Div {
    let assistant_count = app.assistants().len();
    let tools = app
        .tools()
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>()
        .join("  ·  ");

    let mut cards = div().flex().flex_wrap().gap_4().w_full();

    for assistant in app.assistants().to_vec() {
        let model = assistant
            .model
            .clone()
            .unwrap_or_else(|| "routed by Lynxby".to_string());
        let id = assistant.id;

        cards = cards.child(
            widgets::card()
                .id(("assistant", id as usize))
                .w(px(300.))
                .p_5()
                .gap_3()
                .cursor_pointer()
                .hover(|style| style.border_color(theme::border_strong()))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    if let Some(assistant) = this
                        .assistants()
                        .iter()
                        .find(|assistant| assistant.id == id)
                        .cloned()
                    {
                        this.open_assistant(assistant, window, cx);
                    }
                }))
                .child(
                    div()
                        .text_lg()
                        .font_weight(theme::SEMIBOLD)
                        .child(assistant.name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_muted())
                        .child(assistant.role_label()),
                )
                .child(
                    div()
                        .flex()
                        .child(widgets::pill(model, theme::accent_foreground())),
                ),
        );
    }

    let empty_message = if app.working() {
        "Loading what your account may use…"
    } else {
        "You have access to no assistants yet. Ask an administrator for access."
    };

    let empty = div()
        .p_6()
        .rounded_xl()
        .border_1()
        .border_color(theme::border())
        .text_sm()
        .text_color(theme::text_subtle())
        .child(empty_message);

    let notice = match app.notice() {
        Some(Notice::Error(message)) => Some(widgets::error_box(message.clone())),
        Some(Notice::Info(message)) => Some(widgets::info_box(message.clone())),
        None => None,
    };

    let tools_line = if tools.is_empty() {
        "No tools are exposed to this account.".to_string()
    } else {
        tools
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header(app, cx))
        .child(
            div()
                .flex_1()
                .w_full()
                .id("assistants-scroll")
                .overflow_y_scroll()
                .child(
                    div()
                        .w_full()
                        .max_w(px(980.))
                        .mx_auto()
                        .p_8()
                        .flex()
                        .flex_col()
                        .gap_5()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(theme::SEMIBOLD)
                                .child("Your assistants"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_subtle())
                                .child(format!(
                                "{assistant_count} available to you, served and enforced by {}.",
                                app.config().host()
                            )),
                        )
                        .children(notice)
                        .child(if assistant_count == 0 { empty } else { cards })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .pt_4()
                                .border_t_1()
                                .border_color(theme::border())
                                .child(widgets::label("TOOLS THIS ACCOUNT MAY CALL"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_muted())
                                        .child(tools_line),
                                ),
                        )
                        .child(recent(app, cx)),
                ),
        )
}

/// What was already said, so a conversation can be picked up instead of
/// started again. Only conversations whose assistant is still listed appear:
/// an assistant the deployment stopped offering cannot be reopened here.
fn recent(app: &LynxbyApp, cx: &mut Context<LynxbyApp>) -> Div {
    let rows = app
        .conversations()
        .iter()
        .filter_map(|conversation| {
            let assistant = app
                .assistants()
                .iter()
                .find(|assistant| assistant.id == conversation.assistant_id)?;

            Some((conversation.clone(), assistant.name.clone()))
        })
        .collect::<Vec<_>>();

    let mut block = div()
        .flex()
        .flex_col()
        .gap_2()
        .pt_4()
        .border_t_1()
        .border_color(theme::border())
        .child(widgets::label("RECENT CONVERSATIONS"));

    if rows.is_empty() {
        return block.child(div().text_xs().text_color(theme::text_subtle()).child(
            if app.working() {
                "Loading what you talked about…"
            } else {
                "Nothing yet. Open an assistant to start."
            },
        ));
    }

    for (index, (conversation, assistant)) in rows.into_iter().enumerate() {
        let messages = match conversation.messages {
            1 => "1 message".to_string(),
            count => format!("{count} messages"),
        };

        let seen = conversation.last_seen();
        let detail = if seen.is_empty() {
            format!("{assistant}  ·  {messages}")
        } else {
            format!("{assistant}  ·  {messages}  ·  {seen}")
        };

        let session_id = conversation.session_id.clone();

        block = block.child(
            div()
                .id(("conversation", index))
                .w_full()
                .p_4()
                .rounded_xl()
                .border_1()
                .border_color(theme::border())
                .bg(theme::surface_raised())
                .flex()
                .flex_col()
                .gap_1()
                .cursor_pointer()
                .hover(|style| style.border_color(theme::border_strong()))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    let found = this
                        .conversations()
                        .iter()
                        .find(|conversation| conversation.session_id == session_id)
                        .cloned();

                    if let Some(conversation) = found {
                        this.open_conversation(conversation, window, cx);
                    }
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text())
                        .whitespace_normal()
                        .child(conversation.title_line()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_subtle())
                        .child(detail),
                ),
        );
    }

    block
}

fn header(app: &LynxbyApp, cx: &mut Context<LynxbyApp>) -> Div {
    let user = app
        .me()
        .map(|me| me.display_name())
        .unwrap_or_else(|| "Signed in".to_string());

    let organisation = app
        .me()
        .map(|me| me.organisation_name())
        .unwrap_or_default();

    let brand = div()
        .flex()
        .items_center()
        .gap_3()
        .child(widgets::mark(28.))
        .child(
            div()
                .text_lg()
                .font_weight(theme::SEMIBOLD)
                .child(widgets::wordmark()),
        );

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
        .child(brand)
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .child(div().text_sm().text_color(theme::text()).child(user))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_subtle())
                                .child(organisation),
                        ),
                )
                .child(
                    widgets::ghost_button("refresh", "Refresh")
                        .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.load(cx))),
                )
                .child(widgets::ghost_button("sign-out", "Sign out").on_click(
                    cx.listener(|this, _: &ClickEvent, _window, _cx| this.sign_out(None)),
                )),
        )
}
