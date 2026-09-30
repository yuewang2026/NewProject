//! Render the Deck IR to SVG — a deterministic, dependency-free vector preview.
//!
//! deckr does not own pixel-accurate layout: the master owns geometry on the
//! write side, so the renderer is a *preview* that places each block by role the
//! way PowerPoint's outline view does. It needs no external crate (no resvg, no
//! cosmic-text, no Typst), so it builds wherever deckr builds and stays cheap to
//! test. Output is pure SVG; rasterising to PNG/PDF is a separate, later step.
//!
//! Rendering is fully deterministic — no timestamps, no random ids — which is
//! what keeps a `diff` between two renders meaningful.

use crate::ir::{BlockContent, Deck, Paragraph, Role, Slide};

const W: u32 = 960;
const H: u32 = 540;
const MARGIN: u32 = 48;
const LINE: u32 = 28;
const BODY_SIZE: u32 = 20;
const TITLE_SIZE: u32 = 30;

const INK: &str = "#222222";
const TITLE_INK: &str = "#1F3864";
const ACCENT: &str = "#1F3864";
const GRID: &str = "#BFBFBF";
const HEADER_FILL: &str = "#D9E1F2";
const MEDIA_FILL: &str = "#F2F2F2";
const MEDIA_STROKE: &str = "#7F7F7F";

/// Render every slide to its own standalone SVG document.
///
/// Returns `(1-based slide number, svg)`. The caller decides how to persist
/// them — the CLI writes `slide_{n}.svg` and a gallery `index.html`.
pub fn render_deck_svgs(deck: &Deck) -> Vec<(usize, String)> {
    deck.slides
        .iter()
        .map(|slide| (slide.index + 1, render_slide(slide)))
        .collect()
}

/// Render a single slide as a 16:9 SVG string.
pub fn render_slide(slide: &Slide) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {W} {H}\" width=\"{W}\" height=\"{H}\">\n"
    ));
    body.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{W}\" height=\"{H}\" fill=\"#FFFFFF\"/>\n"
    ));

    let mut y = MARGIN as i32;

    if let Some(title) = slide.title() {
        for line in wrap(&title, TITLE_SIZE) {
            body.push_str(&format!(
                "  <text x=\"{MARGIN}\" y=\"{y}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"{TITLE_SIZE}\" font-weight=\"700\" fill=\"{TITLE_INK}\">{text}</text>\n",
                text = escape(&line)
            ));
            y += TITLE_SIZE as i32 + 12;
        }
        // A rule under the title ties it to the slide, the way a master would.
        body.push_str(&format!(
            "  <line x1=\"{MARGIN}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"{ACCENT}\" stroke-width=\"2\"/>\n",
            W - MARGIN
        ));
        y += 20;
    }

    for block in &slide.blocks {
        if block.role.is_chrome() || matches!(block.role, Role::Title | Role::CenteredTitle) {
            continue;
        }
        match &block.content {
            BlockContent::Text(t) => {
                let paras: Vec<&Paragraph> = t
                    .paragraphs
                    .iter()
                    .filter(|p| !p.text.trim().is_empty())
                    .collect();
                if paras.is_empty() {
                    continue;
                }
                let bulleted = matches!(block.role, Role::Body | Role::Object)
                    || paras.iter().any(|p| p.level > 0);
                for p in paras {
                    let indent = (p.level as u32) * 22;
                    for (i, raw) in p.text.split('\n').enumerate() {
                        let lead = if bulleted {
                            if i == 0 {
                                "• ".to_string()
                            } else {
                                "  ".to_string()
                            }
                        } else {
                            String::new()
                        };
                        for line in wrap(raw, BODY_SIZE) {
                            let text = format!("{lead}{line}");
                            let x = MARGIN + indent;
                            body.push_str(&format!(
                                "  <text x=\"{x}\" y=\"{y}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"{BODY_SIZE}\" fill=\"{INK}\">{text}</text>\n",
                                text = escape(&text)
                            ));
                            y += LINE as i32;
                        }
                    }
                }
                y += 8;
            }
            BlockContent::Table { rows } => {
                y = render_table(&mut body, rows, y);
            }
            BlockContent::Picture { alt, .. } => {
                y = render_media_box(&mut body, y, "image", alt.as_deref());
            }
            BlockContent::Chart { .. } => {
                y = render_media_box(&mut body, y, "chart", None);
            }
            BlockContent::Diagram { caption } => {
                y = render_media_box(&mut body, y, "diagram", caption.as_deref());
            }
            BlockContent::Empty => {}
        }
    }

    body.push_str("</svg>\n");
    body
}

/// Draw a table as a grid of bordered cells with a tinted header row.
fn render_table(body: &mut String, rows: &[Vec<String>], mut y: i32) -> i32 {
    let Some(cols) = rows.iter().map(|r| r.len()).max().filter(|c| *c > 0) else {
        return y;
    };
    let table_w = W - 2 * MARGIN;
    let cell_w = table_w / cols as u32;
    let row_h: u32 = 30;
    for (ri, row) in rows.iter().enumerate() {
        let fill = if ri == 0 { HEADER_FILL } else { "#FFFFFF" };
        for ci in 0..cols {
            let x = MARGIN + ci as u32 * cell_w;
            body.push_str(&format!(
                "  <rect x=\"{x}\" y=\"{y}\" width=\"{cell_w}\" height=\"{row_h}\" fill=\"{fill}\" stroke=\"{GRID}\" stroke-width=\"1\"/>\n"
            ));
            let val = row.get(ci).map(|s| s.trim()).unwrap_or("");
            let weight = if ri == 0 { 700 } else { 400 };
            body.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"14\" font-weight=\"{weight}\" fill=\"{INK}\">{}</text>\n",
                x + 6,
                y + 20,
                escape(&truncate(val, (cell_w as usize / 8).max(4)))
            ));
        }
        y += row_h as i32;
    }
    y += 14;
    y
}

/// Draw a dashed placeholder box for a media block (picture, chart, diagram)
/// with its kind and caption centred inside.
fn render_media_box(body: &mut String, mut y: i32, kind: &str, label: Option<&str>) -> i32 {
    let box_w = 360u32;
    let box_h = 200u32;
    body.push_str(&format!(
        "  <rect x=\"{MARGIN}\" y=\"{y}\" width=\"{box_w}\" height=\"{box_h}\" fill=\"{MEDIA_FILL}\" stroke=\"{MEDIA_STROKE}\" stroke-width=\"1.5\" stroke-dasharray=\"6 4\"/>\n"
    ));
    let caption = label.unwrap_or(kind);
    body.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"{ACCENT}\" text-anchor=\"middle\">{text}</text>\n",
        MARGIN + box_w / 2,
        y + box_h as i32 / 2 - 6,
        text = escape(kind)
    ));
    body.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"13\" fill=\"{INK}\" text-anchor=\"middle\">{text}</text>\n",
        MARGIN + box_w / 2,
        y + box_h as i32 / 2 + 18,
        text = escape(&truncate(caption, 40))
    ));
    y += box_h as i32 + 16;
    y
}

/// Escape the five XML-significant characters.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Greedy word-wrap to a rough character budget derived from the canvas width.
///
/// Long unbreakable tokens are kept intact (they will simply be wide), because a
/// preview must never mangle a word to fit the line. Newlines in the source are
/// preserved as paragraph breaks.
fn wrap(text: &str, font_size: u32) -> Vec<String> {
    let max_chars =
        ((W.saturating_sub(2 * MARGIN)) as f32 / (font_size as f32 * 0.55)).max(8.0) as usize;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word.chars().count() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if current.is_empty() && paragraph.trim().is_empty() {
            lines.push(String::new());
        } else if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Trim to `max` characters, appending an ellipsis if anything was dropped.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let taken: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{taken}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, Slide, TextContent};
    use quick_xml::events::Event;
    use quick_xml::Reader as XmlReader;

    /// An SVG is only useful if it parses; assert well-formedness via the same
    /// reader the rest of deckr uses.
    fn is_well_formed(svg: &str) -> bool {
        let mut r = XmlReader::from_str(svg);
        r.config_mut().trim_text(false);
        let mut depth = 0usize;
        loop {
            match r.read_event() {
                Ok(Event::Eof) => break,
                // Self-closing elements (`<line/>`, `<rect/>`) open and close in
                // one token and must not move the balance.
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(_)) => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Err(_) => return false,
                Ok(_) => {}
            }
        }
        depth == 0
    }

    fn chart_slide() -> Slide {
        Slide {
            index: 1,
            blocks: vec![
                Block {
                    role: Role::Title,
                    content: BlockContent::Text(TextContent {
                        paragraphs: vec![Paragraph::new(0, "Quarterly Review")],
                    }),
                },
                Block {
                    role: Role::Body,
                    content: BlockContent::Text(TextContent {
                        paragraphs: vec![Paragraph::new(0, "revenue"), Paragraph::new(1, "up 12%")],
                    }),
                },
                Block {
                    role: Role::Chart,
                    content: BlockContent::Chart {
                        caption: None,
                        blob: None,
                        rid: None,
                        uri: None,
                    },
                },
                Block {
                    role: Role::Picture,
                    content: BlockContent::Picture {
                        alt: Some("architecture".into()),
                        data: None,
                        mime: None,
                        embed: None,
                    },
                },
            ],
        }
    }

    #[test]
    fn slide_renders_to_well_formed_svg() {
        let svg = render_slide(&chart_slide());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(is_well_formed(&svg), "svg was not well-formed:\n{svg}");
    }

    #[test]
    fn title_and_bullets_appear_in_the_output() {
        let svg = render_slide(&chart_slide());
        assert!(svg.contains("Quarterly Review"), "{svg}");
        // Body bullets render with a bullet marker.
        assert!(svg.contains("• revenue"), "{svg}");
        assert!(svg.contains("up 12%"), "{svg}");
    }

    #[test]
    fn media_blocks_become_labelled_boxes() {
        let svg = render_slide(&chart_slide());
        assert!(svg.contains(">chart<"), "{svg}");
        assert!(svg.contains(">image<"), "{svg}");
        assert!(svg.contains("architecture"), "{svg}");
    }

    #[test]
    fn tables_draw_one_rect_per_cell() {
        let rows = vec![
            vec!["Region".to_string(), "Q3".to_string()],
            vec!["APAC".to_string(), "1.2".to_string()],
        ];
        let mut svg = String::new();
        render_table(&mut svg, &rows, 100);
        // 2 rows x 2 cols = 4 cells, plus 4 text labels.
        assert_eq!(svg.matches("<rect").count(), 4, "{svg}");
        assert_eq!(svg.matches("<text").count(), 4, "{svg}");
        assert!(svg.contains("APAC"));
    }

    #[test]
    fn wrap_respects_the_budget_and_keeps_words() {
        let long = "the quick brown fox jumps over the lazy dog";
        let lines = wrap(long, BODY_SIZE);
        assert!(lines.iter().all(|l| l.chars().count() <= 80));
        // The whole sentence is preserved across the wrapped lines.
        assert_eq!(lines.join(" "), long);
    }

    #[test]
    fn deck_render_produces_one_svg_per_slide() {
        let mut a = chart_slide();
        a.index = 0;
        let mut b = chart_slide();
        b.index = 1;
        let deck = Deck { slides: vec![a, b] };
        let pages = render_deck_svgs(&deck);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].0, 1);
        assert_eq!(pages[1].0, 2);
        assert!(pages.iter().all(|(_, s)| is_well_formed(s)));
    }
}
