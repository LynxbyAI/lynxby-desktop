//! Enough Markdown to read an answer: headings, paragraphs, lists, quotes,
//! code blocks, and bold, italic, inline code and links inside a line.
//!
//! shortcut: no tables, nested lists or images; they render as plain lines. Add
//! them when answers start relying on them.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Code,
    Link,
}

/// One line of text with the ranges (byte offsets) that carry a style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Style)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, Inline),
    Paragraph(Inline),
    /// Each item with its marker: "•" for bullets, "3." for numbers.
    List(Vec<(String, Inline)>),
    Quote(Inline),
    Code(String),
    Rule,
}

pub fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Vec<(String, String)> = Vec::new();
    let mut code: Option<Vec<&str>> = None;

    let flush =
        |blocks: &mut Vec<Block>, paragraph: &mut Vec<&str>, list: &mut Vec<(String, String)>| {
            if !paragraph.is_empty() {
                blocks.push(Block::Paragraph(inline(&paragraph.join(" "))));
                paragraph.clear();
            }
            if !list.is_empty() {
                blocks.push(items(std::mem::take(list)));
            }
        };

    for line in source.lines() {
        let trimmed = line.trim();

        if let Some(lines) = code.as_mut() {
            if trimmed.starts_with("```") {
                blocks.push(Block::Code(lines.join("\n")));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }

        if trimmed.starts_with("```") {
            flush(&mut blocks, &mut paragraph, &mut list);
            code = Some(Vec::new());
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut blocks, &mut paragraph, &mut list);
            continue;
        }

        if let Some((level, text)) = heading(trimmed) {
            flush(&mut blocks, &mut paragraph, &mut list);
            blocks.push(Block::Heading(level, inline(text)));
            continue;
        }

        if ["---", "***", "___"].contains(&trimmed) {
            flush(&mut blocks, &mut paragraph, &mut list);
            blocks.push(Block::Rule);
            continue;
        }

        if let Some(text) = trimmed.strip_prefix('>').map(str::trim_start) {
            flush(&mut blocks, &mut paragraph, &mut list);
            blocks.push(Block::Quote(inline(text)));
            continue;
        }

        if let Some((marker, text)) = list_item(trimmed) {
            if !paragraph.is_empty() {
                blocks.push(Block::Paragraph(inline(&paragraph.join(" "))));
                paragraph.clear();
            }
            list.push((marker, text.to_string()));
            continue;
        }

        if !list.is_empty() && line.starts_with([' ', '\t']) {
            // A wrapped list item continues the item above it.
            let (_, item) = list.last_mut().expect("checked non-empty");
            item.push(' ');
            item.push_str(trimmed);
            continue;
        }

        if !list.is_empty() {
            blocks.push(items(std::mem::take(&mut list)));
        }
        paragraph.push(trimmed);
    }

    if let Some(lines) = code {
        blocks.push(Block::Code(lines.join("\n")));
    }
    flush(&mut blocks, &mut paragraph, &mut list);

    blocks
}

fn items(list: Vec<(String, String)>) -> Block {
    Block::List(
        list.into_iter()
            .map(|(marker, source)| (marker, inline(&source)))
            .collect(),
    )
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();

    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        Some((hashes as u8, line[hashes..].trim()))
    } else {
        None
    }
}

fn list_item(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "+ ", "• "] {
        if let Some(text) = line.strip_prefix(bullet) {
            return Some(("•".to_string(), text));
        }
    }

    let digits = line.chars().take_while(char::is_ascii_digit).count();

    if digits > 0 && digits < 4 {
        let rest = &line[digits..];

        if let Some(text) = rest.strip_prefix(". ").or(rest.strip_prefix(") ")) {
            return Some((format!("{}.", &line[..digits]), text));
        }
    }

    None
}

/// Strips the inline markers and records where each style applies.
pub fn inline(source: &str) -> Inline {
    let mut out = Inline::default();
    let mut rest = source;

    while !rest.is_empty() {
        if let Some((inner, after)) = between(rest, "`", "`") {
            push(&mut out, inner, Some(Style::Code));
            rest = after;
        } else if let Some((inner, after)) = between(rest, "**", "**").or(between(rest, "__", "__"))
        {
            push_styled(&mut out, inner, Style::Bold);
            rest = after;
        } else if let Some((inner, after)) = emphasis(rest) {
            push_styled(&mut out, inner, Style::Italic);
            rest = after;
        } else if let Some((label, after)) = link(rest) {
            push(&mut out, label, Some(Style::Link));
            rest = after;
        } else {
            let character = rest.chars().next().expect("rest is not empty");
            out.text.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }

    out
}

/// Styled text that may itself hold inline code or another style.
fn push_styled(out: &mut Inline, source: &str, style: Style) {
    let start = out.text.len();
    let nested = inline(source);

    for (range, inner) in nested.spans {
        out.spans
            .push((range.start + start..range.end + start, inner));
    }
    out.text.push_str(&nested.text);
    out.spans.push((start..out.text.len(), style));
}

fn push(out: &mut Inline, text: &str, style: Option<Style>) {
    let start = out.text.len();
    out.text.push_str(text);

    if let Some(style) = style {
        out.spans.push((start..out.text.len(), style));
    }
}

fn between<'a>(source: &'a str, open: &str, close: &str) -> Option<(&'a str, &'a str)> {
    let body = source.strip_prefix(open)?;
    let end = body.find(close)?;

    (end > 0).then(|| (&body[..end], &body[end + close.len()..]))
}

/// `*x*` or `_x_`, but not a lone asterisk in arithmetic or a snake_case word.
fn emphasis(source: &str) -> Option<(&str, &str)> {
    for marker in ["*", "_"] {
        if let Some((inner, after)) = between(source, marker, marker) {
            let tight = !inner.starts_with(' ') && !inner.ends_with(' ');
            let word_after = after.chars().next().is_some_and(char::is_alphanumeric);

            if tight && !(marker == "_" && word_after) {
                return Some((inner, after));
            }
        }
    }

    None
}

fn link(source: &str) -> Option<(&str, &str)> {
    let (label, after) = between(source, "[", "]")?;
    let (_url, after) = between(after, "(", ")")?;

    Some((label, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_code_and_links_lose_their_markers_and_keep_their_style() {
        let line = inline("Pay **60 days** via `notice()` or see [the contract](https://x.test).");

        assert_eq!(line.text, "Pay 60 days via notice() or see the contract.");
        assert!(line.spans.contains(&(4..11, Style::Bold)));
        assert!(line.spans.contains(&(16..24, Style::Code)));
        assert!(line.spans.contains(&(32..44, Style::Link)));
    }

    #[test]
    fn a_lone_asterisk_and_snake_case_stay_as_written() {
        assert_eq!(inline("2 * 3 = 6").text, "2 * 3 = 6");
        assert_eq!(
            inline("call search_knowledge_base now").text,
            "call search_knowledge_base now"
        );
        assert_eq!(
            inline("an *important* word").spans,
            vec![(3..12, Style::Italic)]
        );
    }

    #[test]
    fn multibyte_text_keeps_valid_ranges() {
        let line = inline("café **één** ✓");

        assert_eq!(line.text, "café één ✓");
        let (range, _) = &line.spans[0];
        assert_eq!(&line.text[range.clone()], "één");
    }

    #[test]
    fn blocks_are_split_the_way_an_answer_reads() {
        let blocks = parse(
            "## Notice\nThe contract says\n60 days.\n\n- first\n- second\n  continued\n1. one\n\n> quoted\n\n```\nlet x = 1;\n```\n---",
        );

        assert_eq!(blocks[0], Block::Heading(2, inline("Notice")));
        assert_eq!(
            blocks[1],
            Block::Paragraph(inline("The contract says 60 days."))
        );
        assert_eq!(
            blocks[2],
            Block::List(vec![
                ("•".into(), inline("first")),
                ("•".into(), inline("second continued")),
                ("1.".into(), inline("one")),
            ])
        );
        assert_eq!(blocks[3], Block::Quote(inline("quoted")));
        assert_eq!(blocks[4], Block::Code("let x = 1;".into()));
        assert_eq!(blocks[5], Block::Rule);
    }

    #[test]
    fn an_unclosed_code_block_still_shows_its_lines() {
        assert_eq!(parse("```\nhalf"), vec![Block::Code("half".into())]);
    }
}
