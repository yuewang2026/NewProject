//! Markdown -> Deck IR.
//!
//! The reciprocal of [crate::markdown]: whatever `deckr convert` prints, this
//! module reads back. That contract is the whole point — `deckr build` takes the
//! Markdown a human (or an LLM) edited, re-binds each block to the matching
//! master placeholder, and writes a real `.pptx`.
//!
//! The dialect is deliberately small and line-oriented:
//!
//! ```text
//! ---
//!
//! ## Slide 1 — Quarterly Review
//!
//! Subtitle text, which binds to the subtitle placeholder.
//!
//! - top level bullet
//!   - nested bullet
//!
//! | Region | Q3 |
//! | --- | --- |
//! | APAC | 1.2 |
//!
//! `![alt text](path)`
//! `[chart]`
//! ```
//!
//! Anything unfamiliar is treated as body text rather than rejected, because the
//! common failure mode for a converter is being too clever about someone else's
//! Markdown.

use crate::ir::{Block, BlockContent, Deck, Paragraph, Role, Run, Slide, TextContent};

const SLIDE_SEPARATOR: &str = "---";
const EM_DASH: char = '\u{2014}';

/// Parse a Markdown deck into Deck IR. Never fails: unrecognised lines become
/// body text.
pub fn parse_markdown(s: &str) -> Deck {
    let mut slides: Vec<Slide> = Vec::new();
    let mut lines = s.lines().peekable();

    while lines.peek().is_some() {
        let mut buffer: Vec<&str> = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim() == SLIDE_SEPARATOR {
                lines.next();
                break;
            }
            buffer.push(lines.next().unwrap());
        }
        if buffer.iter().all(|l| l.trim().is_empty()) {
            continue;
        }
        slides.push(parse_slide(slides.len(), &buffer));
    }

    Deck { slides }
}

fn parse_slide(index: usize, lines: &[&str]) -> Slide {
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || is_placeholder_note(trimmed) {
            i += 1;
            continue;
        }

        if let Some(heading) = heading_text(trimmed) {
            if blocks.is_empty() {
                blocks.push(Block {
                    role: Role::Title,
                    content: BlockContent::Text(TextContent {
                        paragraphs: vec![Paragraph::from_runs(0, parse_inline(heading))],
                    }),
                });
            } else {
                // A `##` after content is a section break, not a second title.
                push_paragraph(&mut blocks, Role::Body, 0, parse_inline(heading));
            }
            i += 1;
            continue;
        }

        if trimmed.starts_with('|') {
            let rows = read_table(lines, &mut i);
            if !rows.is_empty() {
                blocks.push(Block {
                    role: Role::Table,
                    content: BlockContent::Table { rows },
                });
            }
            continue;
        }

        if let Some(alt) = image_alt(trimmed) {
            blocks.push(Block {
                role: Role::Picture,
                content: BlockContent::Picture {
                    alt: Some(alt.to_string()),
                    data: None,
                    mime: None,
                    embed: None,
                },
            });
            i += 1;
            continue;
        }

        if trimmed == "`[chart]`" {
            blocks.push(Block {
                role: Role::Chart,
                content: BlockContent::Chart {
                    caption: None,
                    blob: None,
                    data: None,
                    rid: None,
                    uri: None,
                },
            });
            i += 1;
            continue;
        }

        if trimmed == "`[diagram]`" {
            blocks.push(Block {
                role: Role::Diagram,
                content: BlockContent::Diagram {
                    caption: None,
                    texts: Vec::new(),
                    blob: None,
                    rel_ids: Vec::new(),
                },
            });
            i += 1;
            continue;
        }

        // Note `line`, not `trimmed`: the indent *is* the bullet level.
        if let Some((level, text)) = bullet(line) {
            push_paragraph(&mut blocks, Role::Body, level, parse_inline(text));
            i += 1;
            continue;
        }

        // Plain prose. Directly under the title it is the subtitle; anywhere
        // else it joins the body, which is how the exporter arranged things.
        let role = if blocks.len() == 1 {
            Role::Subtitle
        } else {
            Role::Body
        };
        push_paragraph(&mut blocks, role, 0, parse_inline(trimmed));
        i += 1;
    }

    Slide { index, blocks }
}

fn is_placeholder_note(line: &str) -> bool {
    line == "_(no content)_"
}

/// `## Slide 3 — Revenue` -> `Revenue`. The only heading format we emit, but we
/// tolerate any number of hashes and its absence.
fn heading_text(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    if rest == line {
        return None;
    }
    let mut text = rest.trim();
    // Drop the `Slide N —` prefix: it is generated, not authored.
    if let Some(after) = text.strip_prefix("Slide ") {
        if let Some((_, tail)) = after.split_once(EM_DASH) {
            text = tail.trim();
        }
    }
    Some(text)
}

fn bullet(line: &str) -> Option<(u8, &str)> {
    let indent = line.len() - line.trim_start().len();
    let text = line.trim();
    let body = text.strip_prefix("- ").or_else(|| text.strip_prefix("-"))?;
    // Two spaces per level, matching DrawingML's eight-level indent ladder.
    Some(((indent / 2) as u8, body.trim_end()))
}

fn image_alt(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("!")?;
    let alt = rest.strip_prefix('[')?;
    let (alt, _) = alt.split_once(']')?;
    Some(alt)
}

fn read_table(lines: &[&str], i: &mut usize) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    while *i < lines.len() {
        let line = lines[*i].trim();
        if !line.starts_with('|') {
            break;
        }
        *i += 1;
        if is_table_separator(line) {
            continue;
        }
        let cells = split_row(line)
            .iter()
            .map(|c| c.trim().replace("\\|", "|"))
            .collect::<Vec<_>>();
        if cells.iter().any(|c| !c.is_empty()) {
            rows.push(cells);
        }
    }
    rows
}

fn is_table_separator(line: &str) -> bool {
    line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) && line.contains('-')
}

fn split_row(line: &str) -> Vec<String> {
    let inner = line.trim().trim_matches('|');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '|' if !escaped => cells.push(std::mem::take(&mut cur)),
            _ => {
                cur.push(c);
                escaped = false;
            }
        }
    }
    cells.push(cur);
    cells
}

/// Append a paragraph to the last block if it is already the right kind of text
/// holder, otherwise start one.
fn push_paragraph(blocks: &mut Vec<Block>, role: Role, level: u8, runs: Vec<Run>) {
    let p = Paragraph::from_runs(level, runs);
    if let Some(Block {
        content: BlockContent::Text(t),
        role: r,
    }) = blocks.last_mut()
    {
        if *r == role {
            t.paragraphs.push(p);
            return;
        }
    }
    blocks.push(Block {
        role,
        content: BlockContent::Text(TextContent {
            paragraphs: vec![p],
        }),
    });
}

/// One line of Markdown inline markup -> runs.
///
/// Nesting is one level deep, which is exactly what [`crate::markdown`] emits
/// (`**[label](url)**`). Deeper nesting is flattened rather than rejected.
pub fn parse_inline(s: &str) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut buf = String::new();

    let mut pos = 0;
    while pos < s.len() {
        if let Some((inner, end)) = match_delimited(&s[pos..], "**", "**") {
            flush(&mut buf, &mut runs);
            for r in parse_flat(inner) {
                runs.push(Run {
                    bold: Some(true),
                    ..r
                });
            }
            pos += end;
            continue;
        }
        if let Some((inner, end)) = match_delimited(&s[pos..], "<u>", "</u>") {
            flush(&mut buf, &mut runs);
            for r in parse_flat(inner) {
                runs.push(Run {
                    underline: Some(true),
                    ..r
                });
            }
            pos += end;
            continue;
        }
        if let Some((inner, end)) = match_delimited(&s[pos..], "~~", "~~") {
            flush(&mut buf, &mut runs);
            for r in parse_flat(inner) {
                runs.push(Run {
                    strike: Some(true),
                    ..r
                });
            }
            pos += end;
            continue;
        }
        if let Some((link, label, end)) = match_link(&s[pos..]) {
            flush(&mut buf, &mut runs);
            for r in parse_flat(label) {
                runs.push(Run {
                    link: Some(link.to_string()),
                    ..r
                });
            }
            pos += end;
            continue;
        }
        if let Some((inner, end)) = match_delimited(&s[pos..], "*", "*") {
            flush(&mut buf, &mut runs);
            for r in parse_flat(inner) {
                runs.push(Run {
                    italic: Some(true),
                    ..r
                });
            }
            pos += end;
            continue;
        }

        let ch = s[pos..].chars().next().expect("pos is a char boundary");
        buf.push(ch);
        pos += ch.len_utf8();
    }

    flush(&mut buf, &mut runs);
    runs
}

/// Inside an emphasis span we accept no further emphasis — one level is all we
/// round-trip, and a stray `*` in real prose should not swallow a sentence.
fn parse_flat(s: &str) -> Vec<Run> {
    // Links are still meaningful at this depth: `**[label](url)**` is one level
    // of bold around a link, not two levels of emphasis.
    let mut runs: Vec<Run> = Vec::new();
    let mut buf = String::new();
    let mut pos = 0;
    while pos < s.len() {
        if let Some((link, label, end)) = match_link(&s[pos..]) {
            flush(&mut buf, &mut runs);
            runs.push(Run {
                text: label.to_string(),
                link: Some(link.to_string()),
                ..Default::default()
            });
            pos += end;
            continue;
        }
        let ch = s[pos..].chars().next().expect("pos is a char boundary");
        buf.push(ch);
        pos += ch.len_utf8();
    }
    flush(&mut buf, &mut runs);
    runs
}

fn flush(buf: &mut String, runs: &mut Vec<Run>) {
    if !buf.is_empty() {
        runs.push(Run::new(std::mem::take(buf)));
    }
}

/// If `s` starts with `open`, find its matching `close` and return the inner
/// text plus how many bytes to advance.
fn match_delimited<'a>(s: &'a str, open: &str, close: &str) -> Option<(&'a str, usize)> {
    let rest = s.strip_prefix(open)?;
    let rel = rest.find(close)?;
    if rel == 0 {
        return None; // `****` is not an empty span.
    }
    Some((&rest[..rel], open.len() + rel + close.len()))
}

fn match_link(s: &str) -> Option<(&str, &str, usize)> {
    let rest = s.strip_prefix('[')?;
    let close = rest.find(']')?;
    let after = &rest[close + 1..];
    let target = after.strip_prefix('(')?;
    let end = target.find(')')?;
    let advance = 1 + close + 1 + 1 + end + 1;
    Some((&target[..end], &rest[..close], advance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_and_bullets_become_roles() {
        let deck = parse_markdown(
            "## Slide 1 — Quarterly Review\n\nRevenue grew.\n\n- revenue\n  - up 12%\n",
        );
        assert_eq!(deck.len(), 1);
        assert_eq!(deck.slides[0].title().as_deref(), Some("Quarterly Review"));
        let roles: Vec<Role> = deck.slides[0].blocks.iter().map(|b| b.role).collect();
        assert_eq!(roles, vec![Role::Title, Role::Subtitle, Role::Body]);
        let body = &deck.slides[0].blocks[2];
        let levels: Vec<u8> = body
            .as_text()
            .unwrap()
            .paragraphs
            .iter()
            .map(|p| p.level)
            .collect();
        assert_eq!(levels, vec![0, 1]);
    }

    #[test]
    fn markdown_round_trips_through_the_ir() {
        let source = concat!(
            "---\n\n",
            "## Slide 1 — Hello & welcome\n\n",
            "Read it. Diff it. Build it back.\n\n",
            "- **ship** the reader\n",
            "  - keep it simple\n\n",
            "| Phase | Target |\n| --- | --- |\n| 0.2 write | 2026 Q4 |\n\n",
            "![architecture diagram](media://slide1)\n\n",
            "`[chart]`\n\n",
            "---\n\n",
            "## Slide 2 — Roadmap\n\n",
            "- second deck\n",
        );
        let deck = parse_markdown(source);
        assert_eq!(deck.len(), 2);
        assert_eq!(
            deck.outline(),
            vec![Some("Hello & welcome".into()), Some("Roadmap".into()),]
        );

        let roles: Vec<&str> = deck.slides[0]
            .blocks
            .iter()
            .map(|b| b.role.as_str())
            .collect();
        assert_eq!(
            roles,
            vec!["title", "subtitle", "body", "table", "picture", "chart"]
        );

        let table = match &deck.slides[0].blocks[3].content {
            BlockContent::Table { rows } => rows.clone(),
            other => panic!("expected table, got {other:?}"),
        };
        assert_eq!(table[0], vec!["Phase", "Target"]);
        assert_eq!(table[1], vec!["0.2 write", "2026 Q4"]);

        // The bold span survived the trip instead of becoming literal asterisks.
        let body = deck.slides[0].blocks[2].as_text().unwrap();
        assert_eq!(body.paragraphs[0].text, "ship the reader");
        assert_eq!(body.paragraphs[0].runs[0].bold, Some(true));
    }

    #[test]
    fn inline_markers_become_runs() {
        let runs = parse_inline("plain **bold** *it* ~~gone~~ <u>under</u>");
        let kinds: Vec<&str> = runs
            .iter()
            .map(|r| {
                if r.bold == Some(true) {
                    "b"
                } else if r.italic == Some(true) {
                    "i"
                } else if r.strike == Some(true) {
                    "s"
                } else if r.underline == Some(true) {
                    "u"
                } else {
                    "-"
                }
            })
            .collect();
        assert_eq!(kinds, vec!["-", "b", "-", "i", "-", "s", "-", "u"]);
        assert_eq!(runs[1].text, "bold");
    }

    #[test]
    fn links_survive_and_compose_with_bold() {
        let runs = parse_inline("**[report](https://example.com/r)**");
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0].text, "report");
        assert_eq!(runs[0].link.as_deref(), Some("https://example.com/r"));
        assert_eq!(runs[0].bold, Some(true));
    }

    #[test]
    fn unmatched_markers_stay_literal() {
        let runs = parse_inline("a * unfinished thought");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "a * unfinished thought");
        assert!(runs[0].is_plain());
    }

    #[test]
    fn separator_rows_are_not_data() {
        let mut i = 0;
        let lines = ["| a | b |", "| --- | --- |", "| 1 | 2 |"];
        let rows = read_table(&lines, &mut i);
        assert_eq!(i, 3);
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["1".to_string(), "2".to_string()]
            ]
        );
    }

    #[test]
    fn empty_slides_are_dropped() {
        let deck = parse_markdown("---\n\n---\n");
        assert_eq!(deck.len(), 0);
    }
}
