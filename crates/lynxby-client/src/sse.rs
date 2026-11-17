//! A server-sent-events reader, fed bytes as they arrive.
//!
//! The deployment writes `event: <name>` and one `data:` line per event, but
//! the reader follows the spec rather than that habit: any number of data
//! lines, comments, CRLF, and an event split across reads.

/// Decodes a byte stream into text without splitting a character in half.
///
/// TCP hands over whatever arrived, so a two-byte `é` can land one byte per
/// read. Decoding each read on its own would put a replacement character in the
/// middle of a word.
#[derive(Debug, Default)]
pub struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();

        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();

                    if valid > 0 {
                        out.push_str(
                            std::str::from_utf8(&self.pending[..valid])
                                .expect("the prefix is valid by definition"),
                        );
                        self.pending.drain(..valid);
                    }

                    match error.error_len() {
                        Some(invalid) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..invalid);
                        }
                        // An incomplete character at the end: keep waiting.
                        None => break,
                    }
                }
            }
        }

        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SseEvent {
    pub name: String,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    pending: String,
    event_name: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a chunk and returns every event it completed.
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.pending.push_str(chunk);

        let mut events = Vec::new();

        while let Some(newline) = self.pending.find('\n') {
            let mut line = self.pending[..newline].to_string();
            self.pending.drain(..=newline);

            if line.ends_with('\r') {
                line.pop();
            }

            if let Some(event) = self.line(&line) {
                events.push(event);
            }
        }

        events
    }

    /// Flushes an event a server wrote without its closing blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            let line = line.strip_suffix('\r').unwrap_or(&line).to_string();
            let _ = self.line(&line);
        }

        self.dispatch()
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }

        if line.starts_with(':') {
            return None;
        }

        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };

        match field {
            "event" => self.event_name = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            _ => {}
        }

        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let name = self.event_name.take();

        if self.data.is_empty() {
            return None;
        }

        Some(SseEvent {
            name: name.unwrap_or_else(|| "message".to_string()),
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, data: &str) -> SseEvent {
        SseEvent {
            name: name.to_string(),
            data: data.to_string(),
        }
    }

    #[test]
    fn reads_one_event() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.push("event: delta\ndata: {\"delta\":\"Hi\"}\n\n"),
            vec![event("delta", "{\"delta\":\"Hi\"}")]
        );
    }

    #[test]
    fn reads_several_events_from_one_chunk() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.push("event: start\ndata: {}\n\nevent: delta\ndata: {\"delta\":\"a\"}\n\n"),
            vec![event("start", "{}"), event("delta", "{\"delta\":\"a\"}")]
        );
    }

    #[test]
    fn waits_for_the_rest_of_a_split_event() {
        let mut parser = SseParser::new();

        assert!(parser.push("event: del").is_empty());
        assert!(parser.push("ta\ndata: {\"del").is_empty());
        assert_eq!(
            parser.push("ta\":\"ok\"}\n\n"),
            vec![event("delta", "{\"delta\":\"ok\"}")]
        );
    }

    #[test]
    fn joins_multiple_data_lines() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.push("event: delta\ndata: one\ndata: two\n\n"),
            vec![event("delta", "one\ntwo")]
        );
    }

    #[test]
    fn ignores_comments_and_unknown_fields() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.push(": keep-alive\nid: 7\nretry: 500\nevent: delta\ndata: x\n\n"),
            vec![event("delta", "x")]
        );
    }

    #[test]
    fn handles_crlf() {
        let mut parser = SseParser::new();

        assert_eq!(
            parser.push("event: delta\r\ndata: x\r\n\r\n"),
            vec![event("delta", "x")]
        );
    }

    #[test]
    fn a_blank_line_without_data_dispatches_nothing() {
        let mut parser = SseParser::new();

        assert!(parser.push("event: delta\n\n").is_empty());
        assert!(parser.push("event: delta\n").is_empty());
    }

    #[test]
    fn defaults_the_name_to_message() {
        let mut parser = SseParser::new();

        assert_eq!(parser.push("data: x\n\n"), vec![event("message", "x")]);
    }

    #[test]
    fn finish_flushes_an_unterminated_event() {
        let mut parser = SseParser::new();

        assert!(parser.push("event: done\ndata: {}").is_empty());
        assert_eq!(parser.finish(), Some(event("done", "{}")));
        assert_eq!(parser.finish(), None);
    }

    #[test]
    fn decodes_a_character_split_across_reads() {
        let mut decoder = Utf8Decoder::default();
        let text = "café";

        assert_eq!(decoder.push(text.as_bytes()), "café");
    }

    #[test]
    fn a_split_multibyte_character_is_not_mangled() {
        let mut decoder = Utf8Decoder::default();
        let bytes = "café".as_bytes();

        assert_eq!(decoder.push(&bytes[..4]), "caf");
        assert_eq!(decoder.push(&bytes[4..]), "é");
    }

    #[test]
    fn a_split_emoji_survives() {
        let mut decoder = Utf8Decoder::default();
        let bytes = "ok 🚀".as_bytes();
        let split = bytes.len() - 2;

        let mut text = decoder.push(&bytes[..split]);
        text.push_str(&decoder.push(&bytes[split..]));

        assert_eq!(text, "ok 🚀");
    }

    #[test]
    fn invalid_bytes_become_one_replacement_character() {
        let mut decoder = Utf8Decoder::default();

        assert_eq!(decoder.push(&[0xff, b'a']), "\u{FFFD}a");
    }
}
