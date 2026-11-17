//! Wire types for the deployment's public API.
//!
//! Fields the desktop can live without are optional with a default, so a newer
//! deployment that adds a field, or an older one that lacks it, does not break
//! the screen.

use serde::Deserialize;

use crate::sse::SseEvent;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Me {
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub organisation_id: i64,
    #[serde(default)]
    pub organisation: Option<Organisation>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl Me {
    pub fn organisation_name(&self) -> String {
        self.organisation
            .as_ref()
            .map(|organisation| organisation.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Organisation {}", self.organisation_id))
    }

    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            format!("User {}", self.user_id)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Organisation {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Assistant {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

impl Assistant {
    pub fn role_label(&self) -> String {
        self.role
            .clone()
            .filter(|role| !role.is_empty())
            .unwrap_or_else(|| "Assistant".to_string())
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AssistantPage {
    #[serde(default)]
    pub data: Vec<Assistant>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

/// One passage an answer leaned on, as the deployment stored it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Citation {
    #[serde(default)]
    pub document: String,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub score: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Reply {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub citations: Vec<Citation>,
}

/// A conversation the deployment kept, from `GET /api/v1/sessions`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Conversation {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub assistant_id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub messages: u32,
    #[serde(default)]
    pub last_activity: Option<String>,
}

impl Conversation {
    pub fn title_line(&self) -> String {
        let title = self.title.trim();

        if title.is_empty() {
            "Untitled conversation".to_string()
        } else {
            title.to_string()
        }
    }

    /// The date and minute of the last turn, out of the ISO 8601 the API sends.
    pub fn last_seen(&self) -> String {
        let Some(stamp) = self.last_activity.as_deref() else {
            return String::new();
        };

        if stamp.len() < 16 {
            return stamp.to_string();
        }

        format!("{} {}", &stamp[0..10], &stamp[11..16])
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConversationPage {
    #[serde(default)]
    pub data: Vec<Conversation>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

/// One stored turn, from `GET /api/v1/sessions/{session}/messages`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StoredMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub citations: Vec<Citation>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MessagePage {
    #[serde(default)]
    pub data: Vec<StoredMessage>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ToolList {
    #[serde(default)]
    pub result: Option<ToolListResult>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ToolListResult {
    #[serde(default)]
    pub tools: Vec<Tool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Tool {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// One event from the assistant stream.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Start {
        model: Option<String>,
        session_id: Option<String>,
    },
    Thinking {
        delta: String,
    },
    Tool {
        label: String,
    },
    Delta {
        delta: String,
    },
    Error {
        code: String,
        message: String,
    },
    Done(Box<Reply>),
    /// An event this build does not know yet. Ignored by the screen, never a
    /// reason to drop the stream.
    Unknown {
        name: String,
        data: String,
    },
}

impl StreamEvent {
    pub fn parse(event: &SseEvent) -> Self {
        let value: serde_json::Value =
            serde_json::from_str(&event.data).unwrap_or(serde_json::Value::Null);

        match event.name.as_str() {
            "start" => Self::Start {
                model: string_field(&value, "model"),
                session_id: string_field(&value, "session_id"),
            },
            "thinking" => Self::Thinking {
                delta: string_field(&value, "delta").unwrap_or_default(),
            },
            "tool" => Self::Tool {
                label: tool_label(&value),
            },
            "delta" => Self::Delta {
                delta: string_field(&value, "delta").unwrap_or_default(),
            },
            "error" => Self::Error {
                code: string_field(&value, "code").unwrap_or_else(|| "model_error".to_string()),
                message: string_field(&value, "message").unwrap_or_default(),
            },
            "done" => match serde_json::from_value::<Reply>(value) {
                Ok(reply) => Self::Done(Box::new(reply)),
                Err(error) => Self::Unknown {
                    name: event.name.clone(),
                    data: format!("{error}: {}", event.data),
                },
            },
            _ => Self::Unknown {
                name: event.name.clone(),
                data: event.data.clone(),
            },
        }
    }
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn tool_label(value: &serde_json::Value) -> String {
    for key in ["name", "tool", "label"] {
        if let Some(name) = value.get(key).and_then(serde_json::Value::as_str) {
            return name.to_string();
        }
    }

    if let Some(tool) = value.get("tool") {
        return tool_label(tool);
    }

    "tool".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(name: &str, data: &str) -> StreamEvent {
        StreamEvent::parse(&SseEvent {
            name: name.to_string(),
            data: data.to_string(),
        })
    }

    #[test]
    fn reads_start() {
        assert_eq!(
            parse("start", r#"{"model":"m","session_id":"s"}"#),
            StreamEvent::Start {
                model: Some("m".into()),
                session_id: Some("s".into())
            }
        );
    }

    #[test]
    fn reads_a_delta() {
        assert_eq!(
            parse("delta", r#"{"delta":"Hello"}"#),
            StreamEvent::Delta {
                delta: "Hello".into()
            }
        );
    }

    #[test]
    fn a_delta_without_text_is_empty_not_a_crash() {
        assert_eq!(
            parse("delta", r#"{"other":1}"#),
            StreamEvent::Delta {
                delta: String::new()
            }
        );
    }

    #[test]
    fn reads_a_tool_run() {
        assert_eq!(
            parse(
                "tool",
                r#"{"tool":{"name":"search_knowledge","status":"ok"}}"#
            ),
            StreamEvent::Tool {
                label: "search_knowledge".into()
            }
        );
    }

    #[test]
    fn reads_done_into_a_reply() {
        match parse(
            "done",
            r#"{"id":4,"session_id":"s","content":"hi","model":"m","cost":0.01,"currency":"EUR"}"#,
        ) {
            StreamEvent::Done(reply) => {
                assert_eq!(reply.content, "hi");
                assert_eq!(reply.cost, 0.01);
                assert_eq!(reply.currency.as_deref(), Some("EUR"));
            }
            other => panic!("expected done, got {other:?}"),
        }
    }

    #[test]
    fn a_broken_done_is_unknown() {
        assert!(matches!(
            parse("done", "not json"),
            StreamEvent::Unknown { .. }
        ));
    }

    #[test]
    fn an_error_event_keeps_its_code() {
        assert_eq!(
            parse("error", r#"{"code":"model_error","message":"upstream"}"#),
            StreamEvent::Error {
                code: "model_error".into(),
                message: "upstream".into()
            }
        );
    }

    #[test]
    fn an_unknown_event_is_kept_as_is() {
        assert_eq!(
            parse("future", "{}"),
            StreamEvent::Unknown {
                name: "future".into(),
                data: "{}".into()
            }
        );
    }

    #[test]
    fn decodes_the_assistant_page() {
        let page: AssistantPage = serde_json::from_str(
            r#"{"data":[{"id":1,"name":"Contracts","role":"Lawyer","model":"m"}],"has_more":false}"#,
        )
        .unwrap();

        assert_eq!(page.data[0].name, "Contracts");
        assert_eq!(page.data[0].role_label(), "Lawyer");
        assert!(!page.has_more);
    }

    #[test]
    fn a_conversation_names_itself_and_says_when_it_last_moved() {
        let conversation: Conversation = serde_json::from_str(
            r#"{"session_id":"s","assistant_id":2,"title":"Notice periods","messages":4,"last_activity":"2026-11-17T14:41:09+00:00"}"#,
        )
        .unwrap();

        assert_eq!(conversation.title_line(), "Notice periods");
        assert_eq!(conversation.last_seen(), "2026-11-17 14:41");
    }

    #[test]
    fn an_untitled_or_undated_conversation_still_reads() {
        let conversation = Conversation::default();

        assert_eq!(conversation.title_line(), "Untitled conversation");
        assert_eq!(conversation.last_seen(), "");
    }

    #[test]
    fn a_stored_turn_keeps_its_sources() {
        let message: StoredMessage = serde_json::from_str(
            r#"{"role":"assistant","content":"60 days.","reasoning":"clause 12","model":"m","prompt_tokens":1200,"completion_tokens":171,"citations":[{"segment_id":9,"document":"Supplier agreement","score":0.81,"quote":"notice of 60 days"}]}"#,
        )
        .unwrap();

        assert_eq!(message.content, "60 days.");
        assert_eq!(message.reasoning.as_deref(), Some("clause 12"));
        assert_eq!(message.prompt_tokens, Some(1200));
        assert_eq!(message.completion_tokens, Some(171));
        assert_eq!(message.citations[0].document, "Supplier agreement");
        assert_eq!(message.citations[0].quote, "notice of 60 days");
    }

    #[test]
    fn done_carries_the_sources_it_was_given() {
        match parse(
            "done",
            r#"{"content":"hi","citations":[{"document":"Handbook","quote":"rule one"}]}"#,
        ) {
            StreamEvent::Done(reply) => {
                assert_eq!(reply.citations[0].document, "Handbook");
                assert_eq!(reply.citations[0].quote, "rule one");
            }
            other => panic!("expected done, got {other:?}"),
        }
    }

    #[test]
    fn decodes_the_tools_list() {
        let list: ToolList =
            serde_json::from_str(r#"{"result":{"tools":[{"name":"ask_lynxby"}]}}"#).unwrap();

        assert_eq!(list.result.unwrap().tools[0].name, "ask_lynxby");
    }

    #[test]
    fn a_missing_me_field_does_not_break_the_screen() {
        let me: Me = serde_json::from_str(r#"{"user_id":3,"organisation_id":9}"#).unwrap();

        assert_eq!(me.display_name(), "User 3");
        assert_eq!(me.organisation_name(), "Organisation 9");
    }
}
