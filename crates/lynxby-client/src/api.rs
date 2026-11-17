//! The deployment's public API, as this app uses it.
//!
//! Every call carries the bearer token from sign-in. The deployment decides
//! what that token may reach: the assistant list is filtered by the same
//! policies the browser uses, and a refusal arrives as a normal 403 with a
//! code, never as an empty list.

use std::io::Read;

use rand::RngCore;
use serde::de::DeserializeOwned;

use crate::error::Error;
use crate::http::agent;
use crate::sse::{SseParser, Utf8Decoder};
use crate::types::{
    Assistant, AssistantPage, Conversation, ConversationPage, Me, MessagePage, Reply,
    StoredMessage, StreamEvent, Tool, ToolList,
};

pub struct ApiClient {
    agent: ureq::Agent,
    base_url: String,
    token: String,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            agent: agent(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn me(&self) -> Result<Me, Error> {
        self.get("/api/v1/me")
    }

    /// Every assistant this person may see, following the cursor.
    pub fn assistants(&self) -> Result<Vec<Assistant>, Error> {
        let mut assistants = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..20 {
            let path = match &cursor {
                Some(cursor) => format!("/api/v1/assistants?per_page=100&cursor={cursor}"),
                None => "/api/v1/assistants?per_page=100".to_string(),
            };

            let page: AssistantPage = self.get(&path)?;
            assistants.extend(page.data);

            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }

        Ok(assistants)
    }

    /// The most recent conversations this person had, newest first.
    ///
    /// The endpoint pages oldest first, so the newest sit at the end of the
    /// list and every page has to be read before that is known. Ten pages is
    /// plenty for a recent list and bounds the work on a busy account.
    pub fn conversations(&self, limit: usize) -> Result<Vec<Conversation>, Error> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..10 {
            let path = match &cursor {
                Some(cursor) => format!("/api/v1/sessions?per_page=100&cursor={cursor}"),
                None => "/api/v1/sessions?per_page=100".to_string(),
            };

            let page: ConversationPage = self.get(&path)?;
            all.extend(page.data);

            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }

        let mut newest = all.split_off(all.len().saturating_sub(limit));
        newest.reverse();

        Ok(newest)
    }

    /// Every stored turn of one conversation, in the order they were said.
    pub fn conversation(&self, session_id: &str) -> Result<Vec<StoredMessage>, Error> {
        let mut messages = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..10 {
            let path = match &cursor {
                Some(cursor) => {
                    format!("/api/v1/sessions/{session_id}/messages?per_page=100&cursor={cursor}")
                }
                None => format!("/api/v1/sessions/{session_id}/messages?per_page=100"),
            };

            let page: MessagePage = self.get(&path)?;
            messages.extend(page.data);

            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }

        Ok(messages)
    }

    /// The tools this token may call, straight from the deployment's MCP
    /// endpoint. This is the same list a harness would get, which is the point:
    /// the desktop shows what the person is actually allowed to use.
    pub fn tools(&self) -> Result<Vec<Tool>, Error> {
        let list: ToolList = self.post(
            "/mcp",
            &serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )?;

        Ok(list.result.map(|result| result.tools).unwrap_or_default())
    }

    /// Sends one message and streams the answer through `on_event`.
    ///
    /// Runs on a background thread: it blocks for as long as the model takes.
    pub fn stream_reply(
        &self,
        assistant_id: i64,
        message: &str,
        session_id: &str,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<Reply, Error> {
        let response = self
            .agent
            .post(&format!(
                "{}/api/v1/assistants/{assistant_id}/messages/stream",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "text/event-stream")
            .header("Idempotency-Key", &idempotency_key())
            .send_json(serde_json::json!({
                "message": message,
                "session_id": session_id,
            }))
            .map_err(Error::from)?;

        let status = response.status().as_u16();

        if status >= 300 {
            let text = read_body(response)?;

            return Err(api_error(status, &text));
        }

        let mut reader = response.into_body().into_reader();
        let mut decoder = Utf8Decoder::default();
        let mut parser = SseParser::new();
        let mut buffer = [0u8; 8192];
        let mut reply: Option<Reply> = None;

        loop {
            let read = reader.read(&mut buffer).map_err(Error::from)?;

            if read == 0 {
                break;
            }

            let text = decoder.push(&buffer[..read]);
            let events = parser.push(&text);

            if let Some(done) = dispatch(events, on_event, &mut reply)? {
                return Ok(done);
            }
        }

        if let Some(done) = dispatch(parser.finish().into_iter().collect(), on_event, &mut reply)? {
            return Ok(done);
        }

        Err(Error::Protocol(
            "The answer ended before it was finished.".to_string(),
        ))
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let response = self
            .agent
            .get(&format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .call()
            .map_err(Error::from)?;

        self.decode(response)
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &serde_json::Value) -> Result<T, Error> {
        let response = self
            .agent
            .post(&format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send_json(body)
            .map_err(Error::from)?;

        self.decode(response)
    }

    fn decode<T: DeserializeOwned>(
        &self,
        response: ureq::http::Response<ureq::Body>,
    ) -> Result<T, Error> {
        let status = response.status().as_u16();
        let text = read_body(response)?;

        if status >= 300 {
            return Err(api_error(status, &text));
        }

        serde_json::from_str(&text).map_err(Error::from)
    }
}

/// Feeds events to the caller and stops at `done`.
fn dispatch(
    events: Vec<crate::sse::SseEvent>,
    on_event: &mut dyn FnMut(StreamEvent),
    reply: &mut Option<Reply>,
) -> Result<Option<Reply>, Error> {
    for event in events {
        match StreamEvent::parse(&event) {
            StreamEvent::Error { code, message } => {
                let error = Error::http(502, &code, &message);
                on_event(StreamEvent::Error { code, message });

                return Err(error);
            }
            StreamEvent::Done(done) => {
                *reply = Some((*done).clone());
                on_event(StreamEvent::Done(done));

                return Ok(reply.take());
            }
            other => on_event(other),
        }
    }

    Ok(None)
}

/// The deployment's error body: `{code, message, request_id}`. A proxy that
/// answers HTML instead still leaves something to show.
fn api_error(status: u16, text: &str) -> Error {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => {
            let code = value
                .get("code")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("http_error");
            let message = value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("The deployment refused the request.");

            Error::http(status, code, message)
        }
        Err(_) => Error::http(
            status,
            "http_error",
            format!("The deployment answered {status} without a message."),
        ),
    }
}

fn read_body(response: ureq::http::Response<ureq::Body>) -> Result<String, Error> {
    let mut text = String::new();
    response
        .into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(Error::from)?;

    Ok(text)
}

fn idempotency_key() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);

    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_error_shape_the_api_promises() {
        let error = api_error(
            403,
            r#"{"code":"forbidden","message":"You may not use this assistant.","request_id":"r1"}"#,
        );

        assert_eq!(error.code(), Some("forbidden"));
        assert!(error.is_forbidden());
        assert!(error.to_string().contains("may not use"));
    }

    #[test]
    fn an_html_error_page_does_not_panic() {
        let error = api_error(502, "<html>Bad gateway</html>");

        assert_eq!(error.code(), Some("http_error"));
        assert!(error.to_string().contains("502"));
    }

    #[test]
    fn an_empty_body_still_names_the_status() {
        let error = api_error(500, "");

        assert_eq!(error.status(), Some(500));
    }

    #[test]
    fn idempotency_keys_look_like_one_per_attempt() {
        let first = idempotency_key();
        let second = idempotency_key();

        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }
}
