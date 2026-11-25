//! End-to-end checks against a real HTTP server on loopback.
//!
//! These are the tests that would catch the mistakes a mock type cannot: a
//! missing header, a body that is sent as JSON where the deployment expects a
//! form, a stream that is read as one buffer.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lynxby_client::api::ApiClient;
use lynxby_client::oauth::{self, Loopback};

use lynxby_client::types::StreamEvent;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

type Reply = (u16, &'static str, String);

struct Mock {
    base_url: String,
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

impl Mock {
    /// Serves every request with `handle`, and keeps what arrived.
    fn start(handle: impl Fn(&Recorded) -> Reply + Send + 'static) -> Self {
        let server = tiny_http::Server::http(("127.0.0.1", 0)).expect("bind mock");
        let port = server.server_addr().to_ip().expect("ip").port();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = recorded.clone();

        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);

                let record = Recorded {
                    method: request.method().to_string(),
                    url: request.url().to_string(),
                    headers: request
                        .headers()
                        .iter()
                        .map(|header| (header.field.to_string(), header.value.to_string()))
                        .collect(),
                    body,
                };

                sink.lock().unwrap().push(record.clone());

                let (status, content_type, body) = handle(&record);
                let response = tiny_http::Response::from_string(body)
                    .with_status_code(status)
                    .with_header(
                        format!("Content-Type: {content_type}")
                            .parse::<tiny_http::Header>()
                            .expect("header"),
                    );

                let _ = request.respond(response);
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            recorded,
        }
    }

    fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().unwrap().clone()
    }

    fn last(&self) -> Recorded {
        self.requests().pop().expect("a request arrived")
    }
}

fn json(body: &'static str) -> Reply {
    (200, "application/json", body.to_string())
}

#[test]
fn the_probe_carries_the_bearer_token() {
    let mock = Mock::start(|_| {
        json(r#"{"user_id":7,"name":"Ada","organisation_id":2,"scopes":["assistant.read"]}"#)
    });

    let me = ApiClient::new(&mock.base_url, "lynx_secret").me().unwrap();
    let request = mock.last();

    assert_eq!(me.user_id, 7);
    assert_eq!(me.name, "Ada");
    assert_eq!(request.url, "/api/v1/me");
    assert_eq!(request.header("authorization"), Some("Bearer lynx_secret"));
    assert!(request
        .header("user-agent")
        .unwrap_or_default()
        .starts_with("LynxbyDesktop/"));
}

#[test]
fn a_revoked_token_reads_as_signed_out() {
    let mock = Mock::start(|_| {
        (
            401,
            "application/json",
            r#"{"code":"token_revoked","message":"The token has been revoked.","request_id":"r"}"#
                .to_string(),
        )
    });

    let error = ApiClient::new(&mock.base_url, "t").me().unwrap_err();

    assert!(error.is_unauthorized());
    assert_eq!(error.code(), Some("token_revoked"));
}

#[test]
fn a_restricted_assistant_reads_as_forbidden() {
    let mock = Mock::start(|_| {
        (
            403,
            "application/json",
            r#"{"code":"forbidden","message":"You may not use this assistant."}"#.to_string(),
        )
    });

    let error = ApiClient::new(&mock.base_url, "t")
        .assistants()
        .unwrap_err();

    assert!(error.is_forbidden());
    assert!(error.to_string().contains("may not use"));
}

#[test]
fn the_assistant_list_follows_the_cursor_and_stops() {
    let mock = Mock::start(|request| {
        if request.url.contains("cursor=page-2") {
            json(r#"{"data":[{"id":2,"name":"Second"}],"has_more":false}"#)
        } else {
            json(
                r#"{"data":[{"id":1,"name":"First","role":"Lawyer","model":"m"}],"next_cursor":"page-2","has_more":true}"#,
            )
        }
    });

    let assistants = ApiClient::new(&mock.base_url, "t").assistants().unwrap();
    let urls: Vec<String> = mock.requests().into_iter().map(|r| r.url).collect();

    assert_eq!(assistants.len(), 2);
    assert_eq!(assistants[0].role_label(), "Lawyer");
    assert_eq!(urls.len(), 2);
    assert!(urls[0].contains("per_page=100"));
    assert!(urls[1].contains("cursor=page-2"));
}

#[test]
fn the_tools_list_is_a_json_rpc_call_to_mcp() {
    let mock = Mock::start(|_| {
        json(
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"ask_lynxby"},{"name":"search_knowledge"}]}}"#,
        )
    });

    let tools = ApiClient::new(&mock.base_url, "t").tools().unwrap();
    let request = mock.last();
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();

    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "ask_lynxby");
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "/mcp");
    assert_eq!(body["method"], "tools/list");
    assert_eq!(request.header("authorization"), Some("Bearer t"));
}

#[test]
fn a_streamed_answer_arrives_in_order_and_finishes_with_the_turn() {
    let mock = Mock::start(|_| {
        (
            200,
            "text/event-stream",
            concat!(
                "event: start\ndata: {\"model\":\"m\",\"session_id\":\"s\"}\n\n",
                "event: thinking\ndata: {\"delta\":\"hmm\"}\n\n",
                "event: tool\ndata: {\"tool\":{\"name\":\"search_knowledge\"}}\n\n",
                "event: delta\ndata: {\"delta\":\"Hel\"}\n\n",
                "event: delta\ndata: {\"delta\":\"lo\"}\n\n",
                "event: done\ndata: {\"id\":9,\"session_id\":\"s\",\"content\":\"Hello\",\"model\":\"m\",\"cost\":0.02,\"currency\":\"EUR\"}\n\n",
            )
            .to_string(),
        )
    });

    let mut events = Vec::new();
    let reply = ApiClient::new(&mock.base_url, "t")
        .stream_reply(4, "Hi", "s", None, &mut |event| events.push(event))
        .unwrap();

    let request = mock.last();
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();

    assert_eq!(reply.content, "Hello");
    assert_eq!(reply.cost, 0.02);
    assert_eq!(request.url, "/api/v1/assistants/4/messages/stream");
    assert_eq!(body["message"], "Hi");
    assert_eq!(body["session_id"], "s");
    assert!(request.header("idempotency-key").is_some());
    assert_eq!(request.header("accept"), Some("text/event-stream"));

    let summary: Vec<String> = events
        .iter()
        .map(|event| match event {
            StreamEvent::Start { .. } => "start".to_string(),
            StreamEvent::Thinking { .. } => "thinking".to_string(),
            StreamEvent::Tool(call) => format!("tool:{}", call.name),
            StreamEvent::Delta { delta } => format!("delta:{delta}"),
            StreamEvent::Done(_) => "done".to_string(),
            other => format!("{other:?}"),
        })
        .collect();

    assert_eq!(
        summary,
        vec![
            "start",
            "thinking",
            "tool:search_knowledge",
            "delta:Hel",
            "delta:lo",
            "done"
        ]
    );
}

#[test]
fn the_form_answers_go_with_the_first_message_only_when_given() {
    let mock = Mock::start(|_| {
        (
            200,
            "text/event-stream",
            "event: done\ndata: {\"content\":\"ok\"}\n\n".to_string(),
        )
    });
    let api = ApiClient::new(&mock.base_url, "t");

    api.stream_reply(
        1,
        "Hi",
        "s",
        Some(&serde_json::json!({"topic": "Tax"})),
        &mut |_| {},
    )
    .unwrap();
    let first: serde_json::Value = serde_json::from_str(&mock.last().body).unwrap();

    api.stream_reply(1, "Again", "s", None, &mut |_| {})
        .unwrap();
    let second: serde_json::Value = serde_json::from_str(&mock.last().body).unwrap();

    assert_eq!(first["intake"]["topic"], "Tax");
    assert!(second.get("intake").is_none());
}

#[test]
fn sharing_an_answer_asks_for_a_draft_once() {
    let mock = Mock::start(|_| {
        (
            201,
            "application/json",
            r#"{"id":7,"title":"Notice","state":"draft","confirm_url":"https://x.test/shared/drafts/7"}"#.to_string(),
        )
    });

    let draft = ApiClient::new(&mock.base_url, "t").share(44).unwrap();
    let request = mock.last();

    assert_eq!(draft.confirm_url, "https://x.test/shared/drafts/7");
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "/api/v1/messages/44/share");
    assert!(request.header("idempotency-key").is_some());
}

#[test]
fn a_refused_share_reads_as_forbidden() {
    let mock = Mock::start(|_| {
        (
            403,
            "application/json",
            r#"{"code":"sharing_disabled","message":"Sharing is turned off for this organisation."}"#.to_string(),
        )
    });

    let error = ApiClient::new(&mock.base_url, "t").share(44).unwrap_err();

    assert!(error.is_forbidden());
}

#[test]
fn a_model_error_ends_the_stream_as_an_error() {
    let mock = Mock::start(|_| {
        (
            200,
            "text/event-stream",
            "event: start\ndata: {}\n\nevent: error\ndata: {\"code\":\"model_error\",\"message\":\"upstream fell over\"}\n\n"
                .to_string(),
        )
    });

    let mut seen = Vec::new();
    let error = ApiClient::new(&mock.base_url, "t")
        .stream_reply(1, "Hi", "s", None, &mut |event| seen.push(event))
        .unwrap_err();

    assert_eq!(error.code(), Some("model_error"));
    assert!(error.to_string().contains("upstream fell over"));
    assert!(matches!(seen.first(), Some(StreamEvent::Start { .. })));
}

#[test]
fn a_stream_that_stops_early_is_a_protocol_error() {
    let mock = Mock::start(|_| {
        (
            200,
            "text/event-stream",
            "event: start\ndata: {}\n\nevent: delta\ndata: {\"delta\":\"half\"}\n\n".to_string(),
        )
    });

    let error = ApiClient::new(&mock.base_url, "t")
        .stream_reply(1, "Hi", "s", None, &mut |_| {})
        .unwrap_err();

    assert!(error.to_string().contains("ended before it was finished"));
}

#[test]
fn recent_conversations_are_newest_first_and_follow_the_cursor() {
    let mock = Mock::start(|request| {
        if request.url.contains("cursor=page-2") {
            json(
                r#"{"data":[{"session_id":"new","assistant_id":1,"title":"Newest","messages":2,"last_activity":"2026-11-17T14:41:09+00:00"}],"has_more":false}"#,
            )
        } else {
            json(
                r#"{"data":[{"session_id":"old","assistant_id":1,"title":"Older","messages":4}],"next_cursor":"page-2","has_more":true}"#,
            )
        }
    });

    let conversations = ApiClient::new(&mock.base_url, "t")
        .conversations(5)
        .unwrap();
    let urls: Vec<String> = mock.requests().into_iter().map(|r| r.url).collect();

    assert_eq!(conversations.len(), 2);
    assert_eq!(conversations[0].session_id, "new");
    assert_eq!(conversations[1].session_id, "old");
    assert!(urls[0].starts_with("/api/v1/sessions?per_page=100"));
    assert!(urls[1].contains("cursor=page-2"));
}

#[test]
fn only_the_newest_conversations_are_kept() {
    let mock = Mock::start(|_| {
        json(
            r#"{"data":[
                {"session_id":"a","assistant_id":1,"title":"A"},
                {"session_id":"b","assistant_id":1,"title":"B"},
                {"session_id":"c","assistant_id":1,"title":"C"}
            ],"has_more":false}"#,
        )
    });

    let conversations = ApiClient::new(&mock.base_url, "t")
        .conversations(2)
        .unwrap();

    assert_eq!(
        conversations
            .iter()
            .map(|conversation| conversation.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "b"]
    );
}

#[test]
fn a_stored_conversation_is_read_turn_by_turn() {
    let mock = Mock::start(|_| {
        json(
            r#"{"data":[
                {"role":"user","content":"Notice period?"},
                {"role":"assistant","content":"60 days.","model":"m","citations":[{"document":"Agreement","quote":"60 days"}]}
            ],"has_more":false}"#,
        )
    });

    let messages = ApiClient::new(&mock.base_url, "t")
        .conversation("s-1")
        .unwrap();

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].citations[0].document, "Agreement");
    assert_eq!(
        mock.last().url,
        "/api/v1/sessions/s-1/messages?per_page=100"
    );
}

/// A real HTTP request to the loopback listener, without a browser.
fn hit(port: u16, query: &str) {
    use std::io::Write;

    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let request =
        format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).expect("write");

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
}

#[test]
fn the_loopback_listener_hands_back_the_code_for_this_attempt() {
    let loopback = Loopback::bind(0).unwrap();
    let port = loopback.port();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        hit(port, "code=abc&state=right");
    });

    let code = loopback
        .wait_for_code("right", &AtomicBool::new(false))
        .unwrap();

    assert_eq!(code, "abc");
}

#[test]
fn the_loopback_listener_refuses_another_attempts_state() {
    let loopback = Loopback::bind(0).unwrap();
    let port = loopback.port();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        hit(port, "code=abc&state=wrong");
    });

    let error = loopback
        .wait_for_code("right", &AtomicBool::new(false))
        .unwrap_err();

    assert!(error.to_string().contains("did not match"));
}

#[test]
fn the_loopback_listener_reports_a_declined_consent() {
    let loopback = Loopback::bind(0).unwrap();
    let port = loopback.port();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        hit(
            port,
            "error=access_denied&error_description=Declined&state=right",
        );
    });

    let error = loopback
        .wait_for_code("right", &AtomicBool::new(false))
        .unwrap_err();

    assert!(error.to_string().contains("Declined"));
}

#[test]
fn registering_a_client_asks_for_a_native_loopback_client() {
    let mock = Mock::start(|_| {
        (
            201,
            "application/json",
            r#"{"client_id":"cid"}"#.to_string(),
        )
    });

    let client_id =
        oauth::register_client(&mock.base_url, "http://127.0.0.1:47833/callback").unwrap();
    let request = mock.last();
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();

    assert_eq!(client_id, "cid");
    assert_eq!(request.url, "/oauth/register");
    assert_eq!(body["application_type"], "native");
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["redirect_uris"][0], "http://127.0.0.1:47833/callback");
}

#[test]
fn a_deployment_that_refuses_registration_says_why() {
    let mock = Mock::start(|_| {
        (
            403,
            "application/json",
            r#"{"error":"registration_not_supported","error_description":"This server only accepts pre-registered clients."}"#
                .to_string(),
        )
    });

    let error = oauth::register_client(&mock.base_url, "http://127.0.0.1:1/callback").unwrap_err();

    assert_eq!(error.code(), Some("registration_not_supported"));
}

#[test]
fn the_code_is_exchanged_as_a_form_with_the_verifier() {
    let mock = Mock::start(|_| {
        json(
            r#"{"access_token":"a","refresh_token":"r","expires_in":900,"scope":"assistant.read"}"#,
        )
    });

    let tokens = oauth::exchange_code(
        &mock.base_url,
        "cid",
        "the-code",
        "the-verifier",
        "http://127.0.0.1:47833/callback",
    )
    .unwrap();
    let request = mock.last();

    assert_eq!(tokens.access_token, "a");
    assert_eq!(tokens.expires_in, Some(900));
    assert_eq!(request.url, "/oauth/token");
    assert!(request.body.contains("grant_type=authorization_code"));
    assert!(request.body.contains("code=the-code"));
    assert!(request.body.contains("code_verifier=the-verifier"));
    assert!(request.body.contains("client_id=cid"));
}

#[test]
fn refreshing_sends_the_refresh_grant() {
    let mock = Mock::start(|_| json(r#"{"access_token":"a2"}"#));

    let tokens = oauth::refresh(&mock.base_url, "cid", "the-refresh").unwrap();
    let request = mock.last();

    assert_eq!(tokens.access_token, "a2");
    assert!(request.body.contains("grant_type=refresh_token"));
    assert!(request.body.contains("refresh_token=the-refresh"));
}

/// A body that arrives in pieces, the way a model writes.
struct Chunks {
    parts: std::vec::IntoIter<String>,
    current: Option<std::io::Cursor<String>>,
}

impl Read for Chunks {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if let Some(cursor) = self.current.as_mut() {
                let read = cursor.read(buffer)?;

                if read > 0 {
                    return Ok(read);
                }

                self.current = None;
            }

            match self.parts.next() {
                Some(part) => self.current = Some(std::io::Cursor::new(part)),
                None => return Ok(0),
            }
        }
    }
}

#[test]
fn a_stream_split_across_packets_is_still_assembled() {
    let parts = vec![
        "event: delta\ndata: {\"del".to_string(),
        "ta\":\"café 🚀\"}\n\nevent: done\ndata: {\"content\":\"café 🚀\"}\n\n".to_string(),
    ];

    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();

    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let body = Chunks {
                parts: parts.clone().into_iter(),
                current: None,
            };

            let length = parts.iter().map(String::len).sum();
            let response = tiny_http::Response::new(
                200.into(),
                vec!["Content-Type: text/event-stream"
                    .parse::<tiny_http::Header>()
                    .unwrap()],
                body,
                Some(length),
                None,
            );

            let _ = request.respond(response);
        }
    });

    let mut deltas = String::new();
    let reply = ApiClient::new(format!("http://127.0.0.1:{port}"), "t")
        .stream_reply(1, "Hi", "s", None, &mut |event| {
            if let StreamEvent::Delta { delta } = event {
                deltas.push_str(&delta);
            }
        })
        .unwrap();

    assert_eq!(deltas, "café 🚀");
    assert_eq!(reply.content, "café 🚀");
}
