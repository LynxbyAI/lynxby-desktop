//! The window's state and the transitions between screens.
//!
//! Nothing here draws; the screens do. Everything a screen needs is a field on
//! `LynxbyApp`, and every change goes through a method, so the flow
//! splash → sign-in → assistants → chat is readable in one place.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::*;
use lynxby_client::api::ApiClient;
use lynxby_client::composer::Composer;
use lynxby_client::config::Config;
use lynxby_client::error::Error;
use lynxby_client::intake::{Answer, IntakeRun};
use lynxby_client::signin;
use lynxby_client::store::{Session, Store, StoredUser};
use lynxby_client::types::{
    Assistant, Citation, Conversation, Me, Reply, ShareDraft, StoredMessage, StreamEvent, Tool,
    ToolCall,
};
use lynxby_client::{oauth, USER_AGENT};

use crate::i18n::{self, t, tf};
use crate::screens;
use crate::theme;

/// How many past conversations the assistant screen offers.
const RECENT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Splash,
    Login,
    Assistants,
    Chat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Error(String),
    Info(String),
}

impl Notice {
    fn from_error(error: &Error) -> Self {
        match error {
            Error::Network(_) => Self::Error(format!("{error}. Is the deployment running?")),
            _ => Self::Error(error.to_string()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub user: String,
    pub answer: String,
    pub thinking: String,
    pub tools: Vec<ToolCall>,
    pub error: Option<String>,
    /// The stored answer's id, which sharing needs. Known once the turn is done.
    pub message_id: Option<i64>,
    pub model: Option<String>,
    /// What the turn used, in tokens. Money is the deployment's business, not
    /// the reader's: people buy tokens in advance and read them here.
    pub tokens: Option<u64>,
    pub citations: Vec<Citation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chat {
    pub assistant: Assistant,
    pub session_id: String,
    pub turns: Vec<Turn>,
    pub streaming: bool,
    /// Set while the stored turns of an old conversation are being fetched.
    pub loading: bool,
    /// A failure that belongs to the conversation itself, not to one answer.
    pub error: Option<String>,
    /// The assistant's form, asked before the first message of a new chat.
    pub intake: Option<IntakeRun>,
    /// Set once the form's answers went out with the first message.
    pub intake_sent: bool,
    /// The choices ticked on a multiple-choice question, and the consent box.
    pub selection: Vec<String>,
    pub agreed: bool,
    /// Turns whose tool calls are unfolded.
    pub tools_open: HashSet<usize>,
    /// A line about sharing, under the conversation.
    pub note: Option<String>,
}

impl Chat {
    pub fn new(assistant: Assistant) -> Self {
        let intake = assistant
            .intake
            .clone()
            .filter(|form| !form.fields.is_empty())
            .map(IntakeRun::new);

        Self {
            assistant,
            session_id: new_session_id(),
            turns: Vec::new(),
            streaming: false,
            loading: false,
            error: None,
            intake,
            intake_sent: false,
            selection: Vec::new(),
            agreed: false,
            tools_open: HashSet::new(),
            note: None,
        }
    }

    /// Whether the form still stands between the person and the composer.
    pub fn asking(&self) -> bool {
        self.intake.as_ref().is_some_and(|run| !run.is_done())
    }

    /// Whether the current question is answered by typing in the composer.
    pub fn asking_text(&self) -> bool {
        self.intake
            .as_ref()
            .and_then(IntakeRun::current)
            .is_some_and(|field| {
                !matches!(
                    field.kind.as_str(),
                    "single_choice" | "multiple_choice" | "consent"
                )
            })
    }

    /// Moves the form on with `answer`, and prepares the next question's
    /// choices from what was said before, so Back shows the earlier pick.
    pub fn answer_intake(&mut self, answer: Answer) -> bool {
        let Some(run) = self.intake.as_mut() else {
            return false;
        };

        let moved = run.answer(answer);
        self.restore_selection();
        moved
    }

    pub fn skip_intake(&mut self) {
        if let Some(run) = self.intake.as_mut() {
            run.skip();
        }
        self.restore_selection();
    }

    pub fn back_intake(&mut self) {
        if let Some(run) = self.intake.as_mut() {
            run.back();
        }
        self.restore_selection();
    }

    pub fn toggle_choice(&mut self, option: &str) {
        if let Some(index) = self.selection.iter().position(|picked| picked == option) {
            self.selection.remove(index);
        } else {
            self.selection.push(option.to_string());
        }
    }

    fn restore_selection(&mut self) {
        let answer = self
            .intake
            .as_ref()
            .and_then(|run| run.current().and_then(|field| run.answer_of(&field.key)))
            .cloned();

        self.selection = match &answer {
            Some(Answer::Choices(choices)) => choices.clone(),
            _ => Vec::new(),
        };
        self.agreed = answer == Some(Answer::Agreed);
    }

    /// The answers to send with this message: once, with the first one.
    pub fn intake_payload(&self) -> Option<serde_json::Value> {
        match &self.intake {
            Some(run) if run.is_done() && !self.intake_sent => Some(run.payload()),
            _ => None,
        }
    }

    /// A conversation the deployment already stored, rebuilt into turns: each
    /// question opens a turn and the answer after it fills that turn.
    pub fn from_stored(
        assistant: Assistant,
        session_id: String,
        messages: &[StoredMessage],
    ) -> Self {
        let mut chat = Self {
            session_id,
            intake: None,
            ..Self::new(assistant)
        };

        for message in messages {
            match message.role.as_str() {
                "user" => chat.turns.push(Turn {
                    user: message.content.clone(),
                    ..Turn::default()
                }),
                "assistant" => {
                    let Some(turn) = chat.turns.last_mut() else {
                        continue;
                    };

                    turn.answer = message.content.clone();
                    turn.message_id = Some(message.id).filter(|id| *id > 0);
                    turn.tools = message.tool_calls.clone();
                    turn.thinking = message.reasoning.clone().unwrap_or_default();
                    turn.citations = message.citations.clone();
                    turn.model = message.model.clone();
                    turn.tokens = token_total(message.prompt_tokens, message.completion_tokens);
                }
                _ => {}
            }
        }

        chat
    }

    pub fn begin(&mut self, message: String) {
        self.turns.push(Turn {
            user: message,
            ..Turn::default()
        });
        self.streaming = true;
        self.error = None;
        self.note = None;
    }

    fn current(&mut self) -> Option<&mut Turn> {
        self.turns.last_mut()
    }

    pub fn apply(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Start { model, .. } => {
                if let Some(turn) = self.current() {
                    turn.model = model;
                }
            }
            StreamEvent::Thinking { delta } => {
                if let Some(turn) = self.current() {
                    turn.thinking.push_str(&delta);
                }
            }
            StreamEvent::Tool(call) => {
                // Calls unfold while they arrive, as they do on the web.
                let index = self.turns.len().saturating_sub(1);
                if let Some(turn) = self.current() {
                    turn.tools.push(*call);
                    self.tools_open.insert(index);
                }
            }
            StreamEvent::Delta { delta } => {
                if let Some(turn) = self.current() {
                    turn.answer.push_str(&delta);
                }
            }
            StreamEvent::Error { message, .. } => {
                self.fail(message);
            }
            StreamEvent::Done(reply) => {
                self.finish(&reply);
            }
            StreamEvent::Unknown { .. } => {}
        }
    }

    pub fn finish(&mut self, reply: &Reply) {
        self.streaming = false;

        if let Some(turn) = self.current() {
            if turn.answer.is_empty() {
                turn.answer = reply.content.clone();
            }
            if reply.model.is_some() {
                turn.model = reply.model.clone();
            }
            if !reply.citations.is_empty() {
                turn.citations = reply.citations.clone();
            }
            if turn.tools.is_empty() {
                turn.tools = reply.tool_calls.clone();
            }
            turn.message_id = Some(reply.id).filter(|id| *id > 0);
            turn.tokens = token_total(reply.prompt_tokens, reply.completion_tokens);
        }
    }

    pub fn fail(&mut self, message: String) {
        self.streaming = false;

        if let Some(turn) = self.current() {
            turn.error = Some(message);
        }
    }
}

enum StreamItem {
    Event(StreamEvent),
    Finished(Result<Reply, Error>),
}

pub struct LynxbyApp {
    config: Config,
    store: Store,
    screen: Screen,
    session: Option<Session>,
    me: Option<Me>,
    assistants: Vec<Assistant>,
    tools: Vec<Tool>,
    conversations: Vec<Conversation>,
    chat: Option<Chat>,
    composer: Composer,
    scroll: ScrollHandle,
    input: FocusHandle,
    notice: Option<Notice>,
    working: bool,
    /// Set while the browser is expected back; the cancel button flips it.
    sign_in_cancel: Arc<AtomicBool>,
    /// One refresh per failure. Without it, a deployment that keeps answering
    /// 401 would refresh in a loop.
    refresh_attempted: bool,
    splash_frame: usize,
    /// Set by `LYNXBY_DEMO` in a debug build: screens render sample data and
    /// nothing is sent anywhere.
    demo: bool,
}

impl LynxbyApp {
    pub fn new(config: Config, store: Store, cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            config,
            store,
            screen: Screen::Splash,
            session: None,
            me: None,
            assistants: Vec::new(),
            tools: Vec::new(),
            conversations: Vec::new(),
            chat: None,
            composer: Composer::default(),
            scroll: ScrollHandle::new(),
            input: cx.focus_handle(),
            notice: None,
            working: false,
            sign_in_cancel: Arc::new(AtomicBool::new(false)),
            refresh_attempted: false,
            splash_frame: 0,
            demo: false,
        };

        #[cfg(debug_assertions)]
        if let Ok(kind) = std::env::var("LYNXBY_DEMO") {
            app.seed_demo(&kind);
            return app;
        }

        app.play_splash(cx);
        app
    }

    /// Sample data for working on the screens without a deployment.
    #[cfg(debug_assertions)]
    fn seed_demo(&mut self, kind: &str) {
        self.demo = true;
        self.me = Some(Me {
            user_id: 1,
            name: "Ada Lovelace".to_string(),
            organisation_id: 1,
            organisation: Some(lynxby_client::types::Organisation {
                id: 1,
                name: "Lynxby".to_string(),
                slug: Some("lynxby".to_string()),
            }),
            scopes: vec!["assistant.read".to_string()],
            ..Me::default()
        });
        self.assistants = vec![
            Assistant {
                id: 1,
                name: "Contracts".to_string(),
                role: Some("Legal counsel".to_string()),
                model: Some("mistral-small".to_string()),
                ..Assistant::default()
            },
            Assistant {
                id: 2,
                name: "Support".to_string(),
                role: Some("First line".to_string()),
                model: Some("gpt-4.1-mini".to_string()),
                ..Assistant::default()
            },
            Assistant {
                id: 3,
                name: "Onboarding".to_string(),
                role: Some("People ops".to_string()),
                intake: Some(demo_form()),
                ..Assistant::default()
            },
        ];
        self.tools = vec![
            Tool {
                name: "ask_lynxby".to_string(),
                description: None,
            },
            Tool {
                name: "search_knowledge".to_string(),
                description: None,
            },
        ];

        self.conversations = vec![Conversation {
            session_id: "6f1c9a4e6a4b4c8f9d2e0b7a5c3d1e2f".to_string(),
            assistant_id: 1,
            title: "What is the notice period in the supplier contract?".to_string(),
            messages: 4,
            last_activity: Some("2026-11-17T14:41:09+00:00".to_string()),
        }];

        if kind == "login" || kind == "login-waiting" {
            self.screen = Screen::Login;

            if kind == "login-waiting" {
                self.working = true;
                self.notice = Some(Notice::Info(tf(
                    "Waiting for your browser on 127.0.0.1:{port}. Approve there, then come back.",
                    &[("port", &self.config.redirect_port.to_string())],
                )));
            }

            return;
        }

        self.screen = Screen::Assistants;

        if ["chat", "chat-empty", "chat-streaming", "chat-tools"].contains(&kind) {
            let mut chat = Chat::new(self.assistants[0].clone());
            chat.begin("What is the notice period in the supplier contract?".to_string());
            chat.apply(StreamEvent::Start {
                model: Some("mistral-small".to_string()),
                session_id: None,
            });
            chat.apply(StreamEvent::Tool(Box::new(ToolCall {
                name: "search_knowledge".to_string(),
                ok: true,
                status: Some(serde_json::json!(200)),
                arguments: Some(serde_json::json!(
                    r#"{"query":"supplier contract notice period"}"#
                )),
                result: Some(serde_json::json!(
                    r#"{"body":{"hits":2,"documents":["Supplier agreement","Framework terms"]}}"#
                )),
                duration_ms: Some(184),
            })));
            chat.apply(StreamEvent::Tool(Box::new(ToolCall {
                name: "crm_lookup_account".to_string(),
                ok: false,
                status: None,
                arguments: Some(serde_json::json!(r#"{"account":"[ORGANISATION_1]"}"#)),
                result: Some(serde_json::json!(
                    "The CRM did not answer within 10 seconds."
                )),
                duration_ms: Some(10_004),
            })));
            chat.apply(StreamEvent::Delta {
                delta: "The supplier contract sets a **notice period of 60 days**, counted from the first of the month.\n\n".to_string(),
            });
            chat.apply(StreamEvent::Delta {
                delta: "## What that means\n- Notice given on *14 March* takes effect on **1 June**.\n- Send it in writing; `clause 12.3` accepts email.\n\n> Either party may terminate with 60 days' notice.".to_string(),
            });
            chat.finish(&Reply {
                id: 41,
                content: String::new(),
                model: Some("mistral-small".to_string()),
                prompt_tokens: Some(1_200),
                completion_tokens: Some(171),
                citations: vec![Citation {
                    document: "Supplier agreement".to_string(),
                    quote: "Either party may terminate with 60 days' notice, counted from the first of the month.".to_string(),
                    score: Some(0.81),
                }],
                ..Reply::default()
            });
            if kind != "chat-tools" {
                chat.tools_open.clear();
            }

            if kind == "chat-streaming" {
                chat.begin("And for the framework terms?".to_string());
                chat.apply(StreamEvent::Start {
                    model: Some("mistral-small".to_string()),
                    session_id: None,
                });
            }

            self.chat = Some(chat);
            self.screen = Screen::Chat;
        }

        if kind == "intake" || kind == "intake-choice" {
            let mut chat = Chat::new(self.assistants[2].clone());
            chat.answer_intake(Answer::Text("Ada Lovelace".to_string()));

            if kind == "intake-choice" {
                chat.answer_intake(Answer::Text("ada@example.com".to_string()));
                chat.toggle_choice("Laptop");
                chat.toggle_choice("Phone");
            }

            self.chat = Some(chat);
            self.screen = Screen::Chat;
            self.composer
                .insert(if kind == "intake" { "ada@exa" } else { "" });
            self.scroll.scroll_to_bottom();
            return;
        }

        if kind != "chat-empty" {
            self.composer.insert("Thank you.");
        }

        self.scroll.scroll_to_bottom();
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn me(&self) -> Option<&Me> {
        self.me.as_ref()
    }

    pub fn assistants(&self) -> &[Assistant] {
        &self.assistants
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    pub fn conversations(&self) -> &[Conversation] {
        &self.conversations
    }

    pub fn chat(&self) -> Option<&Chat> {
        self.chat.as_ref()
    }

    pub fn composer(&self) -> &Composer {
        &self.composer
    }

    pub fn scroll(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn input(&self) -> &FocusHandle {
        &self.input
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn working(&self) -> bool {
        self.working
    }

    pub fn splash_frame(&self) -> usize {
        self.splash_frame
    }

    /// Splash, then straight to the assistant list when a usable session is
    /// already on disk, and to sign-in otherwise.
    fn play_splash(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..8 {
                cx.background_executor()
                    .timer(Duration::from_millis(70))
                    .await;

                if this
                    .update(cx, |app, cx| {
                        app.splash_frame += 1;
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }

            this.update(cx, |app, cx| app.after_splash(cx)).ok();
        })
        .detach();
    }

    fn after_splash(&mut self, cx: &mut Context<Self>) {
        self.session = self.store.load_for(&self.config.base_url);

        match &self.session {
            Some(session) if !session.is_expired(now()) => {
                self.screen = Screen::Assistants;
                self.load(cx);
            }
            Some(session) if session.refreshable() => {
                self.screen = Screen::Assistants;
                self.refresh(cx);
            }
            _ => {
                self.screen = Screen::Login;
                cx.notify();
            }
        }
    }

    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }

        self.working = true;
        self.sign_in_cancel.store(false, Ordering::Relaxed);
        self.notice = Some(Notice::Info(tf(
            "Waiting for your browser on 127.0.0.1:{port}. Approve there, then come back.",
            &[("port", &self.config.redirect_port.to_string())],
        )));
        cx.notify();

        let config = self.config.clone();
        let store = self.store.clone();
        let client_id = config.client_id.clone();
        let cancel = self.sign_in_cancel.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    signin::sign_in(
                        &config.base_url,
                        config.redirect_port,
                        client_id,
                        &store,
                        &cancel,
                    )
                })
                .await;

            this.update(cx, |app, cx| app.signed_in(result, cx)).ok();
        })
        .detach();
    }

    /// Stops waiting for the browser. The listener closes when the task ends.
    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        self.sign_in_cancel.store(true, Ordering::Relaxed);
        cx.notify();
    }

    fn signed_in(&mut self, result: Result<Session, Error>, cx: &mut Context<Self>) {
        self.working = false;

        if result.as_ref().is_err_and(Error::is_cancelled) {
            self.notice = None;
            cx.notify();

            return;
        }

        match result {
            Ok(session) => {
                self.session = Some(session);
                self.refresh_attempted = false;
                self.notice = None;
                self.screen = Screen::Assistants;
                self.load(cx);
            }
            Err(error) if error.code() == Some("registration_not_supported") => {
                self.notice = Some(Notice::Error(
                    "This deployment only accepts pre-registered clients. Ask your administrator for a client id and start the app with LYNXBY_CLIENT_ID set.".to_string(),
                ));
                cx.notify();
            }
            Err(error) => {
                self.notice = Some(Notice::from_error(&error));
                cx.notify();
            }
        }
    }

    /// Everything the person may see, from the deployment, in one round trip.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            self.screen = Screen::Login;
            cx.notify();
            return;
        };

        self.working = true;
        cx.notify();

        let base_url = session.base_url.clone();
        let token = session.access_token.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let api = ApiClient::new(base_url, token);
                    let me = api.me()?;
                    let assistants = api.assistants()?;
                    let tools = api.tools().unwrap_or_default();
                    let conversations = api.conversations(RECENT).unwrap_or_default();

                    Ok::<_, Error>((me, assistants, tools, conversations))
                })
                .await;

            this.update(cx, |app, cx| app.loaded(result, cx)).ok();
        })
        .detach();
    }

    fn loaded(&mut self, result: Result<Loaded, Error>, cx: &mut Context<Self>) {
        self.working = false;

        match result {
            Ok((me, assistants, tools, conversations)) => {
                self.refresh_attempted = false;
                if let Some(locale) = me.locale.as_deref() {
                    i18n::set(locale);
                }
                self.remember_user(&me);
                self.me = Some(me);
                self.assistants = assistants;
                self.tools = tools;
                self.conversations = conversations;
                self.notice = None;
            }
            Err(error) if error.is_unauthorized() => {
                if !self.refresh_attempted
                    && self.session.as_ref().is_some_and(Session::refreshable)
                {
                    self.refresh(cx);
                    return;
                }

                self.sign_out(Some(Notice::Info(
                    t("Your session ended. Sign in again.").to_string(),
                )));
            }
            Err(error) if error.is_forbidden() => {
                self.notice = Some(Notice::Error(format!(
                    "{error}. Ask an administrator for access."
                )));
            }
            Err(error) => self.notice = Some(Notice::from_error(&error)),
        }

        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(refresh_token) = session.refresh_token.clone() else {
            self.sign_out(Some(Notice::Info(
                t("Your session ended. Sign in again.").to_string(),
            )));
            return;
        };

        self.working = true;
        self.refresh_attempted = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let base_url = session.base_url.clone();
            let client_id = session.client_id.clone();

            let result = cx
                .background_executor()
                .spawn(async move { oauth::refresh(&base_url, &client_id, &refresh_token) })
                .await;

            this.update(cx, |app, cx| match result {
                Ok(tokens) => {
                    if let Some(current) = app.session.as_mut() {
                        current.access_token = tokens.access_token;

                        if tokens.refresh_token.is_some() {
                            current.refresh_token = tokens.refresh_token;
                        }

                        current.expires_at = tokens.expires_in.map(|seconds| now() + seconds);

                        let _ = app.store.save(current);
                    }

                    app.load(cx);
                }
                Err(error) => {
                    app.working = false;
                    let detail = error.to_string();
                    app.sign_out(Some(Notice::Info(format!(
                        "{} ({detail})",
                        t("Your session ended. Sign in again.")
                    ))));
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn sign_out(&mut self, notice: Option<Notice>) {
        let _ = self.store.clear();

        self.session = None;
        self.me = None;
        self.assistants.clear();
        self.tools.clear();
        self.conversations.clear();
        self.chat = None;
        self.composer.clear();
        self.notice = notice;
        self.screen = Screen::Login;
    }

    pub fn open_assistant(
        &mut self,
        assistant: Assistant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.chat = Some(Chat::new(assistant));
        self.composer.clear();
        self.notice = None;
        self.screen = Screen::Chat;
        window.focus(&self.input);
        cx.notify();
    }

    /// Picks a conversation up where it was left. The assistant has to be one
    /// this person may still use: a stored conversation never grants access to
    /// an assistant the deployment stopped listing.
    pub fn open_conversation(
        &mut self,
        conversation: Conversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(assistant) = self
            .assistants
            .iter()
            .find(|assistant| assistant.id == conversation.assistant_id)
            .cloned()
        else {
            self.notice = Some(Notice::Info(
                t("That conversation's assistant is no longer available to you.").to_string(),
            ));
            cx.notify();

            return;
        };

        // Only a real deployment has turns to fetch; the sample screens do not.
        let fetching = self.session.is_some() && !self.demo;

        let mut chat = Chat::new(assistant);
        chat.session_id = conversation.session_id.clone();
        // The form belongs to a new conversation; this one already started.
        chat.intake = None;
        chat.loading = fetching;
        self.chat = Some(chat);
        self.notice = None;
        self.screen = Screen::Chat;
        window.focus(&self.input);
        cx.notify();

        if fetching {
            self.load_conversation(conversation.session_id, cx);
        }
    }

    fn load_conversation(&mut self, session_id: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };

        cx.spawn(async move |this, cx| {
            let requested = session_id.clone();

            let result = cx
                .background_executor()
                .spawn(async move {
                    let api = ApiClient::new(session.base_url, session.access_token);
                    api.conversation(&requested)
                })
                .await;

            this.update(cx, |app, cx| {
                app.conversation_loaded(&session_id, result, cx)
            })
            .ok();
        })
        .detach();
    }

    fn conversation_loaded(
        &mut self,
        session_id: &str,
        result: Result<Vec<StoredMessage>, Error>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chat.as_mut() else {
            return;
        };

        // The person may have gone back or started another chat while this
        // was in flight; their newer choice wins.
        if chat.session_id != session_id {
            return;
        }

        chat.loading = false;

        match result {
            Ok(messages) => {
                *chat =
                    Chat::from_stored(chat.assistant.clone(), chat.session_id.clone(), &messages);
            }
            Err(error) => chat.error = Some(error.to_string()),
        }

        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    pub fn back_to_assistants(&mut self, cx: &mut Context<Self>) {
        self.chat = None;
        self.composer.clear();
        self.screen = Screen::Assistants;
        cx.notify();
    }

    pub fn new_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            *chat = Chat::new(chat.assistant.clone());
        }
        self.composer.clear();
        cx.notify();
    }

    pub fn toggle_tools(&mut self, turn: usize, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            if !chat.tools_open.remove(&turn) {
                chat.tools_open.insert(turn);
            }
        }
        cx.notify();
    }

    pub fn intake_choose(&mut self, option: String, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            chat.answer_intake(Answer::Text(option));
        }
        self.after_intake_step(cx);
    }

    pub fn intake_toggle(&mut self, option: String, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            chat.toggle_choice(&option);
        }
        cx.notify();
    }

    pub fn intake_agree(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            chat.agreed = !chat.agreed;
        }
        cx.notify();
    }

    /// Continue on a multiple-choice or consent question.
    pub fn intake_continue(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            let kind = chat
                .intake
                .as_ref()
                .and_then(IntakeRun::current)
                .map(|field| field.kind.clone());

            let answer = match kind.as_deref() {
                Some("consent") if chat.agreed => Answer::Agreed,
                Some("consent") => Answer::Skipped,
                _ => Answer::Choices(chat.selection.clone()),
            };

            chat.answer_intake(answer);
        }
        self.after_intake_step(cx);
    }

    pub fn intake_skip(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut() {
            chat.skip_intake();
        }
        self.after_intake_step(cx);
    }

    pub fn intake_back(&mut self, cx: &mut Context<Self>) {
        let previous = self.chat.as_mut().and_then(|chat| {
            chat.back_intake();
            let run = chat.intake.as_ref()?;
            match run.current().and_then(|field| run.answer_of(&field.key)) {
                Some(Answer::Text(text)) if chat.asking_text() => Some(text.clone()),
                _ => None,
            }
        });

        // A typed answer comes back into the box, to be corrected.
        self.composer.clear();
        if let Some(text) = previous {
            self.composer.insert(&text);
        }
        self.after_intake_step(cx);
    }

    fn after_intake_step(&mut self, cx: &mut Context<Self>) {
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Drafts a share of one answer and opens the draft in the browser, where
    /// the person chooses who may see it and publishes it.
    pub fn share(&mut self, turn: usize, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_mut() else {
            return;
        };
        let Some(message_id) = chat.turns.get(turn).and_then(|turn| turn.message_id) else {
            return;
        };
        let Some(session) = self.session.clone().filter(|_| !self.demo) else {
            chat.note = Some(t("Open the draft in your browser to publish it. Nothing is reachable until you do.").to_string());
            cx.notify();
            return;
        };

        chat.note = Some(t("Opening the share draft…").to_string());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result: Result<ShareDraft, Error> = cx
                .background_executor()
                .spawn(async move {
                    ApiClient::new(session.base_url, session.access_token).share(message_id)
                })
                .await;

            this.update(cx, |app, cx| {
                let note = match result {
                    Ok(draft) => {
                        cx.open_url(&draft.confirm_url);
                        t("Open the draft in your browser to publish it. Nothing is reachable until you do.").to_string()
                    }
                    Err(error) => tf(
                        "The share could not be made: {error}",
                        &[("error", &error.to_string())],
                    ),
                };

                if let Some(chat) = app.chat.as_mut() {
                    chat.note = Some(note);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn send(&mut self, cx: &mut Context<Self>) {
        if self.working {
            return;
        }

        if !self.demo && self.chat.as_ref().is_some_and(|chat| chat.streaming) {
            return;
        }

        if let Some(chat) = self.chat.as_mut() {
            if chat.asking() {
                if chat.asking_text() {
                    let typed = self.composer.text().trim().to_string();
                    if chat.answer_intake(Answer::Text(typed)) {
                        self.composer.clear();
                    }
                    self.scroll.scroll_to_bottom();
                    cx.notify();
                }
                return;
            }
        }

        let message = self.composer.take();
        if message.is_empty() {
            return;
        }

        if self.demo {
            if let Some(chat) = self.chat.as_mut() {
                chat.intake_sent |= chat.intake_payload().is_some();
                chat.begin(message);
                chat.apply(StreamEvent::Delta {
                    delta: "Sample answer: this build has no deployment behind it.".to_string(),
                });
                chat.finish(&Reply::default());
            }

            self.scroll.scroll_to_bottom();
            cx.notify();
            return;
        }

        let (Some(session), Some(chat)) = (self.session.clone(), self.chat.as_mut()) else {
            return;
        };

        if chat.streaming {
            return;
        }

        chat.begin(message.clone());
        let assistant_id = chat.assistant.id;
        let session_id = chat.session_id.clone();
        let intake = chat.intake_payload();
        chat.intake_sent |= intake.is_some();
        self.scroll.scroll_to_bottom();
        cx.notify();

        let (sender, receiver) = async_channel::unbounded::<StreamItem>();

        cx.background_executor()
            .spawn(async move {
                let api = ApiClient::new(session.base_url, session.access_token);

                let result = api.stream_reply(
                    assistant_id,
                    &message,
                    &session_id,
                    intake.as_ref(),
                    &mut |event| {
                        let _ = sender.send_blocking(StreamItem::Event(event));
                    },
                );

                let _ = sender.send_blocking(StreamItem::Finished(result));
            })
            .detach();

        cx.spawn(async move |this, cx| {
            while let Ok(item) = receiver.recv().await {
                let done = this
                    .update(cx, |app, cx| app.on_stream(item, cx))
                    .unwrap_or(true);

                if done {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_stream(&mut self, item: StreamItem, cx: &mut Context<Self>) -> bool {
        match item {
            StreamItem::Event(event) => {
                if let Some(chat) = self.chat.as_mut() {
                    chat.apply(event);
                }

                self.scroll.scroll_to_bottom();
                cx.notify();

                false
            }
            StreamItem::Finished(result) => {
                match result {
                    Ok(reply) => {
                        if let Some(chat) = self.chat.as_mut() {
                            chat.finish(&reply);
                        }
                    }
                    Err(error) if error.is_unauthorized() => {
                        self.sign_out(Some(Notice::Info(
                            t("Your session ended. Sign in again.").to_string(),
                        )));
                    }
                    Err(error) => {
                        if let Some(chat) = self.chat.as_mut() {
                            chat.fail(error.to_string());
                            // A 422 is refused before anything is stored, so the
                            // form's answers go again with the next attempt.
                            if error.status() == Some(422) && chat.turns.len() == 1 {
                                chat.intake_sent = false;
                            }
                        }
                    }
                }

                self.scroll.scroll_to_bottom();
                cx.notify();

                true
            }
        }
    }

    /// Keyboard editing for the composer. Platform and control chords are left
    /// to the window, so Cmd+Q and Ctrl+C still work.
    ///
    /// shortcut: Latin text only; there is no IME or clipboard paste yet.
    pub fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;

        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }

        match keystroke.key.as_str() {
            "enter" if keystroke.modifiers.shift => self.composer.insert("\n"),
            "enter" => self.send(cx),
            "backspace" => self.composer.backspace(),
            "space" => self.composer.insert(" "),
            _ => {
                if let Some(character) = keystroke.key_char.as_deref() {
                    self.composer.insert(character);
                }
            }
        }

        cx.notify();
    }

    fn remember_user(&mut self, me: &Me) {
        let Some(session) = self.session.as_mut() else {
            return;
        };

        session.user = Some(StoredUser {
            name: me.display_name(),
            organisation: me.organisation_name(),
        });

        let _ = self.store.save(session);
    }
}

impl Render for LynxbyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = match self.screen {
            Screen::Splash => screens::splash::render(self, cx),
            Screen::Login => screens::login::render(self, cx),
            Screen::Assistants => screens::assistants::render(self, cx),
            Screen::Chat => screens::chat::render(self, window, cx),
        };

        div()
            .size_full()
            .bg(theme::surface())
            .text_color(theme::text())
            .font_family("Manrope")
            .child(screen)
    }
}

#[cfg(debug_assertions)]
fn demo_form() -> lynxby_client::types::IntakeForm {
    use lynxby_client::types::{IntakeField, IntakeForm};

    let field = |key: &str, kind: &str, label: &str, required: bool| IntakeField {
        key: key.to_string(),
        kind: kind.to_string(),
        label: label.to_string(),
        required,
        ..IntakeField::default()
    };

    IntakeForm {
        fields: vec![
            field("name", "single_line", "What is your name?", true),
            IntakeField {
                help: Some("We send the welcome pack here.".to_string()),
                ..field("email", "email", "Which email address should we use?", true)
            },
            IntakeField {
                options: vec!["Laptop".into(), "Phone".into(), "Second screen".into()],
                ..field(
                    "kit",
                    "multiple_choice",
                    "What do you need on day one?",
                    false,
                )
            },
            field(
                "privacy",
                "consent",
                "May we keep these answers with the chat?",
                true,
            ),
        ],
    }
}

/// One round trip to the deployment, at sign-in and on every refresh.
type Loaded = (Me, Vec<Assistant>, Vec<Tool>, Vec<Conversation>);

/// The tokens a turn used, in and out together, when the deployment counted them.
fn token_total(prompt: Option<u64>, completion: Option<u64>) -> Option<u64> {
    match (prompt, completion) {
        (None, None) => None,
        (prompt, completion) => Some(prompt.unwrap_or_default() + completion.unwrap_or_default()),
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn new_session_id() -> String {
    use rand::RngCore;

    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);

    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The version line under the mark, so a support question can start with it.
pub fn version_line() -> String {
    format!("{} · {}", USER_AGENT, std::env::consts::OS)
}

#[cfg(test)]
mod tests {
    use super::{token_total, Chat, Notice, Turn};
    use lynxby_client::error::Error;
    use lynxby_client::intake::Answer;
    use lynxby_client::types::{
        Assistant, Citation, IntakeField, IntakeForm, Reply, StoredMessage, StreamEvent, ToolCall,
    };

    fn assistant() -> Assistant {
        Assistant {
            id: 3,
            name: "Contracts".to_string(),
            ..Assistant::default()
        }
    }

    fn reply(content: &str) -> Reply {
        Reply {
            content: content.to_string(),
            model: Some("m".to_string()),
            prompt_tokens: Some(1_200),
            completion_tokens: Some(171),
            ..Reply::default()
        }
    }

    #[test]
    fn a_new_chat_starts_with_its_own_session() {
        let first = Chat::new(assistant());
        let second = Chat::new(assistant());

        assert_ne!(first.session_id, second.session_id);
        assert!(first.turns.is_empty());
        assert!(!first.streaming);
    }

    #[test]
    fn sending_adds_a_turn_and_marks_it_streaming() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].user, "Hi");
        assert!(chat.streaming);
    }

    #[test]
    fn deltas_accumulate_in_order() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        chat.apply(StreamEvent::Delta {
            delta: "Hel".into(),
        });
        chat.apply(StreamEvent::Delta { delta: "lo".into() });

        assert_eq!(chat.turns[0].answer, "Hello");
    }

    #[test]
    fn thinking_and_tools_are_kept_apart_from_the_answer() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        chat.apply(StreamEvent::Thinking {
            delta: "hmm".into(),
        });
        for _ in 0..2 {
            chat.apply(StreamEvent::Tool(Box::new(ToolCall {
                name: "search_knowledge".into(),
                ok: true,
                ..ToolCall::default()
            })));
        }

        assert_eq!(chat.turns[0].thinking, "hmm");
        assert_eq!(chat.turns[0].answer, "");
        // Two runs of the same tool are two calls, and they unfold as they arrive.
        assert_eq!(chat.turns[0].tools.len(), 2);
        assert!(chat.tools_open.contains(&0));
    }

    #[test]
    fn a_stored_answer_brings_its_tool_calls_folded_and_its_id() {
        let messages = vec![
            stored("user", "Notice period?"),
            StoredMessage {
                id: 41,
                tool_calls: vec![ToolCall {
                    name: "search_knowledge".into(),
                    ..ToolCall::default()
                }],
                ..stored("assistant", "60 days.")
            },
        ];

        let chat = Chat::from_stored(assistant(), "s".to_string(), &messages);

        assert_eq!(chat.turns[0].message_id, Some(41));
        assert_eq!(chat.turns[0].tools[0].name, "search_knowledge");
        assert!(chat.tools_open.is_empty());
        assert!(chat.intake.is_none());
    }

    #[test]
    fn the_form_comes_first_and_its_answers_go_with_the_first_message_once() {
        let mut chat = Chat::new(Assistant {
            intake: Some(IntakeForm {
                fields: vec![
                    IntakeField {
                        key: "name".into(),
                        kind: "single_line".into(),
                        required: true,
                        ..IntakeField::default()
                    },
                    IntakeField {
                        key: "kit".into(),
                        kind: "multiple_choice".into(),
                        options: vec!["Laptop".into(), "Phone".into()],
                        ..IntakeField::default()
                    },
                ],
            }),
            ..assistant()
        });

        assert!(chat.asking() && chat.asking_text());
        assert_eq!(chat.intake_payload(), None);

        chat.answer_intake(Answer::Text("Ada".into()));
        assert!(chat.asking() && !chat.asking_text());

        chat.toggle_choice("Phone");
        chat.answer_intake(Answer::Choices(chat.selection.clone()));
        assert!(!chat.asking());

        chat.back_intake();
        assert_eq!(chat.selection, vec!["Phone"], "Back shows the earlier pick");
        chat.answer_intake(Answer::Choices(chat.selection.clone()));

        assert_eq!(
            chat.intake_payload(),
            Some(serde_json::json!({"name": "Ada", "kit": ["Phone"]}))
        );
        chat.intake_sent = true;
        assert_eq!(chat.intake_payload(), None);
    }

    #[test]
    fn the_model_is_taken_from_the_start_event() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        chat.apply(StreamEvent::Start {
            model: Some("mistral".into()),
            session_id: None,
        });

        assert_eq!(chat.turns[0].model.as_deref(), Some("mistral"));
    }

    #[test]
    fn done_fills_a_missing_answer_and_stops_the_spinner() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        chat.finish(&reply("Full answer"));

        assert_eq!(chat.turns[0].answer, "Full answer");
        assert_eq!(chat.turns[0].model.as_deref(), Some("m"));
        assert!(!chat.streaming);
    }

    #[test]
    fn done_does_not_overwrite_a_streamed_answer() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());
        chat.apply(StreamEvent::Delta {
            delta: "already".into(),
        });

        chat.finish(&reply("different"));

        assert_eq!(chat.turns[0].answer, "already");
    }

    #[test]
    fn a_failed_turn_keeps_what_arrived_and_says_why() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());
        chat.apply(StreamEvent::Delta {
            delta: "half".into(),
        });

        chat.apply(StreamEvent::Error {
            code: "model_error".into(),
            message: "upstream fell over".into(),
        });

        assert_eq!(chat.turns[0].answer, "half");
        assert_eq!(chat.turns[0].error.as_deref(), Some("upstream fell over"));
        assert!(!chat.streaming);
    }

    #[test]
    fn an_unknown_event_changes_nothing() {
        let mut chat = Chat::new(assistant());
        chat.begin("Hi".to_string());

        chat.apply(StreamEvent::Unknown {
            name: "future".into(),
            data: "{}".into(),
        });

        assert_eq!(
            chat.turns[0],
            Turn {
                user: "Hi".to_string(),
                ..Turn::default()
            }
        );
    }

    fn stored(role: &str, content: &str) -> StoredMessage {
        StoredMessage {
            role: role.to_string(),
            content: content.to_string(),
            ..StoredMessage::default()
        }
    }

    #[test]
    fn a_stored_conversation_is_rebuilt_into_turns() {
        let messages = vec![
            stored("user", "Notice period?"),
            StoredMessage {
                citations: vec![Citation {
                    document: "Supplier agreement".to_string(),
                    quote: "60 days".to_string(),
                    score: Some(0.8),
                }],
                reasoning: Some("clause 12".to_string()),
                model: Some("mistral".to_string()),
                prompt_tokens: Some(1200),
                completion_tokens: Some(171),
                ..stored("assistant", "60 days.")
            },
            stored("user", "And notice by email?"),
            stored("assistant", "Also 60 days."),
        ];

        let chat = Chat::from_stored(assistant(), "session-1".to_string(), &messages);

        assert_eq!(chat.session_id, "session-1");
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].user, "Notice period?");
        assert_eq!(chat.turns[0].answer, "60 days.");
        assert_eq!(chat.turns[0].thinking, "clause 12");
        assert_eq!(chat.turns[0].model.as_deref(), Some("mistral"));
        assert_eq!(chat.turns[0].tokens, Some(1371));
        assert_eq!(chat.turns[0].citations[0].document, "Supplier agreement");
        assert_eq!(chat.turns[1].answer, "Also 60 days.");
        assert!(chat.turns[1].citations.is_empty());
        assert!(!chat.streaming);
        assert!(!chat.loading);
    }

    #[test]
    fn an_answer_without_a_question_does_not_become_a_turn() {
        let messages = vec![
            stored("assistant", "orphan"),
            stored("user", "Then this?"),
            stored("assistant", "That."),
        ];

        let chat = Chat::from_stored(assistant(), "s".to_string(), &messages);

        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].user, "Then this?");
        assert_eq!(chat.turns[0].answer, "That.");
    }

    #[test]
    fn a_turn_counts_the_tokens_it_used() {
        assert_eq!(token_total(Some(1200), Some(171)), Some(1371));
        assert_eq!(token_total(Some(1200), None), Some(1200));
        assert_eq!(token_total(None, None), None);
    }

    #[test]
    fn a_network_error_is_explained_not_dumped() {
        let notice = Notice::from_error(&Error::Network("connection refused".into()));

        assert_eq!(
            notice,
            Notice::Error(
                "Cannot reach Lynxby: connection refused. Is the deployment running?".to_string()
            )
        );
    }
}
