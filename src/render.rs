//! Render the Deck IR to SVG — a deterministic vector preview — and, with
//! `resvg`, to PNG bitmaps.
//!
//! deckr does not own pixel-accurate layout: the master owns geometry on the
//! write side, so the renderer is a *preview* that places each block by role the
//! way PowerPoint's outline view does. The SVG path needs no external crate, so
//! it builds wherever deckr builds and stays cheap to test. The PNG path uses
//! `resvg` (pure Rust, no Cairo/HarfBuzz/fontconfig), the only renderer
//! dependency, and is what `deckr render --png` calls.
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
            BlockContent::Chart { data: Some(d), .. } if d.has_numbers() => {
                y = render_chart(&mut body, y, d);
            }
            BlockContent::Chart { .. } => {
                y = render_media_box(&mut body, y, "chart", None);
            }
            BlockContent::Diagram { caption, .. } => {
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

/// The series colours, in assignment order. Office's own palette, simplified:
/// the preview is a reading aid, not a style source — the master owns styling.
const CHART_COLORS: [&str; 6] = [
    "#4472C4", "#C00000", "#70AD47", "#FFC000", "#7C64A8", "#2E9599",
];

/// Draw a decoded chart as a real plot — grouped bars, lines, or a pie from
/// the first numeric series. Layout is fixed and deterministic: same data in,
/// same SVG out.
///
/// The plot replaces the dashed media placeholder only when the data actually
/// carries numbers; otherwise the caller falls back to the placeholder box.
fn render_chart(body: &mut String, y: i32, d: &crate::chart::ChartData) -> i32 {
    let chart_w = (W - 2 * MARGIN) as i32;
    let chart_h = 240i32;
    let x0 = MARGIN as i32;
    let top = y + 8;
    let bottom = top + chart_h;

    // The plot's own geometry: a left gutter for y-axis labels, a bottom
    // gutter for category labels.
    let gutter_l = x0 + 56;
    let plot_w = chart_w - (gutter_l - x0) - 8;
    let plot_top = top + if d.title.is_some() { 26 } else { 6 };
    let plot_h = bottom - 26 - plot_top;

    let max = d
        .series
        .iter()
        .flat_map(|s| s.values.iter())
        .flatten()
        .copied()
        .fold(0.0f64, f64::max);
    let cat_count = d
        .series
        .iter()
        .map(|s| s.categories.len().max(s.values.len()))
        .max()
        .unwrap_or(0);

    if max <= 0.0 || cat_count == 0 {
        // Nothing plottable — keep the honest placeholder instead.
        return render_media_box(body, y, "chart", d.title.as_deref());
    }

    let scale = |v: f64| plot_h as f64 * (v / max);

    // Frame: y-axis with three gridlines and labels (0, mid, max).
    for (i, frac) in [0.0f64, 0.5, 1.0].iter().enumerate() {
        let gy = bottom - 26 - (plot_h as f64 * frac) as i32;
        body.push_str(&format!(
            "  <line x1=\"{gutter_l}\" y1=\"{gy}\" x2=\"{}\" y2=\"{gy}\" stroke=\"{GRID}\" stroke-width=\"{}\"/>\n",
            gutter_l + plot_w,
            if i == 0 { 2 } else { 1 }
        ));
        let label = if *frac == 1.0 {
            crate::chart::format_number(max)
        } else if *frac == 0.5 {
            crate::chart::format_number(max * 0.5)
        } else {
            "0".to_string()
        };
        body.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"12\" fill=\"{INK}\" text-anchor=\"end\">{}</text>\n",
            gutter_l - 6,
            gy + 4,
            label
        ));
    }

    // Title, top-left above the plot.
    if let Some(title) = &d.title {
        body.push_str(&format!(
            "  <text x=\"{gutter_l}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"15\" font-weight=\"700\" fill=\"{TITLE_INK}\">{}</text>\n",
            plot_top - 8,
            escape(&truncate(title, 60))
        ));
    }

    let category_of = |i: usize| -> String {
        d.series
            .iter()
            .filter_map(|s| s.categories.get(i))
            .find(|c| !c.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", i + 1))
    };

    match d.kind {
        crate::chart::ChartKind::Pie => {
            render_pie(body, d, gutter_l + plot_w / 2, plot_top + plot_h / 2);
        }
        crate::chart::ChartKind::Line => {
            render_lines(body, d, gutter_l, plot_w, bottom, &scale);
        }
        _ => {
            render_bars(body, d, gutter_l, plot_w, bottom, &scale);
        }
    }

    // Category labels under the axis.
    let slot = plot_w as f64 / cat_count as f64;
    for i in 0..cat_count {
        let cx = gutter_l as f64 + slot * (i as f64 + 0.5);
        body.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"12\" fill=\"{INK}\" text-anchor=\"middle\">{}</text>\n",
            cx as i32,
            bottom - 8,
            escape(&truncate(&category_of(i), 10))
        ));
    }

    // Legend under the categories (pie legend lists categories instead).
    if d.kind != crate::chart::ChartKind::Pie {
        let mut lx = gutter_l;
        let ly = bottom + 14;
        for i in 0..d.series.len() {
            body.push_str(&format!(
                "  <rect x=\"{lx}\" y=\"{}\" width=\"10\" height=\"10\" fill=\"{}\"/>\n",
                ly - 9,
                CHART_COLORS[i % CHART_COLORS.len()]
            ));
            let label = d.series_label(i);
            body.push_str(&format!(
                "  <text x=\"{}\" y=\"{ly}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"12\" fill=\"{INK}\">{}</text>\n",
                lx + 14,
                escape(&truncate(&label, 16))
            ));
            lx += 24 + (label.chars().count() as i32 * 7).min(120);
            if lx > gutter_l + plot_w - 60 {
                break;
            }
        }
    } else {
        for (i, cat) in (0..cat_count).enumerate() {
            let lx = gutter_l + (i as i32 % 6) * 120;
            let ly = bottom + 14 + (i as i32 / 6) * 16;
            body.push_str(&format!(
                "  <rect x=\"{lx}\" y=\"{}\" width=\"10\" height=\"10\" fill=\"{}\"/>\n",
                ly - 9,
                CHART_COLORS[i % CHART_COLORS.len()]
            ));
            body.push_str(&format!(
                "  <text x=\"{}\" y=\"{ly}\" font-family=\"Segoe UI, Arial, sans-serif\" font-size=\"12\" fill=\"{INK}\">{}</text>\n",
                lx + 14,
                escape(&truncate(&category_of(cat), 14))
            ));
        }
    }

    y + chart_h + 44
}

/// Grouped vertical bars: one bar per (category, series) pair.
fn render_bars(
    body: &mut String,
    d: &crate::chart::ChartData,
    gutter_l: i32,
    plot_w: i32,
    bottom: i32,
    scale: &dyn Fn(f64) -> f64,
) {
    let cats = d
        .series
        .iter()
        .map(|s| s.categories.len().max(s.values.len()))
        .max()
        .unwrap_or(0);
    let slot = plot_w as f64 / cats as f64;
    let group = slot * 0.7;
    let bar_w = (group / d.series.len() as f64).floor().max(2.0);
    let axis = bottom - 26;
    for (si, s) in d.series.iter().enumerate() {
        for (ci, v) in s.values.iter().enumerate() {
            let Some(v) = v else { continue };
            let h = scale(*v).round().max(1.0) as i32;
            let x = (gutter_l as f64 + slot * ci as f64 + (slot - group) / 2.0 + si as f64 * bar_w)
                as i32;
            let y = axis - h;
            body.push_str(&format!(
                "  <rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{h}\" fill=\"{}\"/>\n",
                bar_w as i32 - 1,
                CHART_COLORS[si % CHART_COLORS.len()]
            ));
        }
    }
}

/// One polyline per series, point at each category slot.
fn render_lines(
    body: &mut String,
    d: &crate::chart::ChartData,
    gutter_l: i32,
    plot_w: i32,
    bottom: i32,
    scale: &dyn Fn(f64) -> f64,
) {
    let axis = bottom - 26;
    for (si, s) in d.series.iter().enumerate() {
        let color = CHART_COLORS[si % CHART_COLORS.len()];
        let n = s.values.len();
        if n == 0 {
            continue;
        }
        let slot = if n > 1 {
            plot_w as f64 / (n - 1) as f64
        } else {
            0.0
        };
        let pts: Vec<String> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(ci, v)| {
                v.map(|v| {
                    let x = gutter_l as f64 + slot * ci as f64;
                    let y = axis - scale(v).round() as i32;
                    format!("{:.0},{:.0}", x, y)
                })
            })
            .collect();
        if pts.len() > 1 {
            body.push_str(&format!(
                "  <polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.5\"/>\n",
                pts.join(" ")
            ));
        }
        for p in &pts {
            let (x, y) = p.split_once(',').unwrap_or(("0", "0"));
            body.push_str(&format!(
                "  <circle cx=\"{x}\" cy=\"{y}\" r=\"3\" fill=\"{color}\"/>\n"
            ));
        }
    }
}

/// A pie of the first series that carries numbers; slices start at 12 o'clock
/// and run clockwise, in series order of the categories.
fn render_pie(body: &mut String, d: &crate::chart::ChartData, cx: i32, cy: i32) {
    let Some(series) = d
        .series
        .iter()
        .find(|s| s.values.iter().any(|v| v.is_some()))
    else {
        return;
    };
    let total: f64 = series.values.iter().flatten().copied().sum();
    if total <= 0.0 {
        return;
    }
    let r = 90.0f64;
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (ci, v) in series.values.iter().enumerate() {
        let Some(v) = v else { continue };
        if *v <= 0.0 {
            continue;
        }
        let sweep = v / total * std::f64::consts::TAU;
        let x1 = cx as f64 + r * angle.cos();
        let y1 = cy as f64 + r * angle.sin();
        angle += sweep;
        let x2 = cx as f64 + r * angle.cos();
        let y2 = cy as f64 + r * angle.sin();
        let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
        body.push_str(&format!(
            "  <path d=\"M {cx},{cy} L {x1:.1},{y1:.1} A {r},{r} 0 {large} 1 {x2:.1},{y2:.1} Z\" fill=\"{}\" stroke=\"#FFFFFF\" stroke-width=\"1.5\"/>\n",
            CHART_COLORS[ci % CHART_COLORS.len()]
        ));
    }
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

// --- PNG rasterisation ---------------------------------------------------
//
// The SVG above is the vector source of truth. `deckr render --png` rasterises
// it with `resvg` (pure Rust) so you get a real bitmap you can drop into a
// document, email or slide. Rasterising is deterministic given the same fonts,
// and resvg is the only renderer dependency.

use resvg::usvg;

/// Something went wrong turning an SVG string into a PNG buffer.
#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    /// The SVG could not be parsed into a render tree.
    #[error("SVG parse failed: {0}")]
    Parse(String),
    /// `resvg` refused to rasterise the tree (e.g. zero-sized surface).
    #[error("rasterisation failed: {0}")]
    Render(String),
    /// The output pixmap could not be allocated.
    #[error("could not allocate an output pixmap")]
    Alloc,
}

/// Render one slide straight to PNG bytes (encoded, not written).
pub fn render_slide_png(slide: &Slide) -> Result<Vec<u8>, RasterError> {
    rasterise_svg(&render_slide(slide))
}

/// Render every slide to PNG bytes, returning only the slides that rasterised
/// successfully as `(1-based slide number, png bytes)`.
pub fn render_deck_pngs(deck: &Deck) -> Vec<(usize, Vec<u8>)> {
    deck.slides
        .iter()
        .map(|slide| (slide.index + 1, render_slide_png(slide)))
        .filter_map(|(n, r)| r.ok().map(|bytes| (n, bytes)))
        .collect()
}

/// Rasterise a standalone SVG string into PNG bytes via `resvg`.
pub fn rasterise_svg(svg: &str) -> Result<Vec<u8>, RasterError> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opts).map_err(|e| RasterError::Parse(e.to_string()))?;
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or(RasterError::Alloc)?;
    let mut pixmap_mut = pixmap.as_mut();
    // resvg 0.44 takes a root transform instead of an auto-fit; the pixmap is
    // sized to the SVG's own dimensions, so the identity transform is exact.
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap_mut,
    );
    pixmap
        .encode_png()
        .map_err(|e| RasterError::Render(e.to_string()))
}

/// The PNG magic number — used by tests and any caller that wants to sanity
/// check the rasteriser output before writing it to disk.
pub const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, Slide, TextContent};
    use quick_xml::Reader as XmlReader;
    use quick_xml::events::Event;

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
                        data: None,
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

    #[test]
    fn slide_rasterises_to_a_valid_png() {
        let png = render_slide_png(&chart_slide()).expect("rasterisation should succeed");
        assert!(png.len() > 8, "png buffer was empty");
        assert_eq!(&png[..8], PNG_MAGIC, "output was not a PNG");
        // A real raster has substantial payload beyond the 8-byte header.
        assert!(
            png.len() > 1024,
            "png suspiciously small: {} bytes",
            png.len()
        );
    }

    #[test]
    fn deck_rasterises_one_png_per_slide() {
        let mut a = chart_slide();
        a.index = 0;
        let mut b = chart_slide();
        b.index = 1;
        let deck = Deck { slides: vec![a, b] };
        let pngs = render_deck_pngs(&deck);
        assert_eq!(pngs.len(), 2);
        assert!(pngs.iter().all(|(_, b)| &b[..8] == PNG_MAGIC));
    }
}
