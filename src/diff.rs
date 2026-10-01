//! Semantic diff between two decks — `deckr diff old.pptx new.pptx`.
//!
//! The comparison happens on the Deck IR, not on the zip. Two decks are
//! aligned slide by slide (both carry presentation order), each slide's blocks
//! are paired by [`Role`], and paired text is compared paragraph by paragraph,
//! tables cell by cell. What comes out is a report a human can act on —
//! "slide 3 title changed, one bullet added, table cell (3,2) is now 4.1%" —
//! rather than "binary files differ".
//!
//! What is deliberately *not* reported: page furniture (`footer`, `datetime`,
//! `slide_number`) regenerates from the master on rebuild, so carrying it in a
//! diff would report noise; and a chart is compared byte-for-byte because its
//! numbers are still opaque to deckr (numeric extraction is a later phase).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::chart::ChartData;
use crate::ir::{Block, BlockContent, ChartBlob, Deck, Paragraph, Role, Slide, TextContent};

/// One semantic difference between two decks.
///
/// `slide` is the one-based slide number the change happened on (for
/// `SlideAdded` it is where the slide landed in the new deck; for
/// `SlideRemoved` where it lived in the old one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A whole slide exists in the new deck but not the old.
    SlideAdded { slide: usize, title: Option<String> },
    /// A whole slide exists in the old deck but not the new.
    SlideRemoved { slide: usize, title: Option<String> },
    /// The slide's title placeholder says something different now.
    TitleChanged {
        slide: usize,
        from: String,
        to: String,
    },
    /// A block of this role appears on the slide for the first time.
    BlockAdded {
        slide: usize,
        role: Role,
        description: String,
    },
    /// A block of this role is gone from the slide.
    BlockRemoved {
        slide: usize,
        role: Role,
        description: String,
    },
    /// A paragraph was appended to a text block.
    ParagraphAdded {
        slide: usize,
        role: Role,
        level: u8,
        text: String,
    },
    /// A paragraph disappeared from a text block.
    ParagraphRemoved {
        slide: usize,
        role: Role,
        level: u8,
        text: String,
    },
    /// The same paragraph slot now says something different.
    ParagraphChanged {
        slide: usize,
        role: Role,
        level: u8,
        from: String,
        to: String,
    },
    /// The characters are identical but the character properties are not —
    /// bold appeared, a colour changed, a link was added.
    ParagraphFormattingChanged {
        slide: usize,
        role: Role,
        text: String,
    },
    /// One table cell says something different.
    TableCellChanged {
        slide: usize,
        row: usize,
        col: usize,
        from: String,
        to: String,
    },
    /// A table row exists now that did not before (or vice versa) — the row's
    /// cells travel with the change.
    TableRowAdded {
        slide: usize,
        row: usize,
        cells: Vec<String>,
    },
    TableRowRemoved {
        slide: usize,
        row: usize,
        cells: Vec<String>,
    },
    /// A table changed shape in a way per-cell reports cannot express (the
    /// two rows being compared have different column counts).
    TableReshaped { slide: usize, description: String },
    /// A chart gained a series (or the deck's chart data changed shape in a
    /// way the other chart kinds cannot express — see `MediaChanged`).
    ChartSeriesAdded { slide: usize, name: String },
    /// A chart lost a series.
    ChartSeriesRemoved { slide: usize, name: String },
    /// The same series slot has a different legend name now.
    ChartSeriesRenamed {
        slide: usize,
        from: String,
        to: String,
    },
    /// A category label on a series changed.
    ChartCategoryChanged {
        slide: usize,
        series: String,
        index: usize,
        from: String,
        to: String,
    },
    /// One plotted number says something different — the change that motivates
    /// the whole feature.
    ChartValueChanged {
        slide: usize,
        series: String,
        category: String,
        from: String,
        to: String,
    },
    /// A picture was replaced, or its alt text changed; a chart's content or
    /// caption changed.
    MediaChanged {
        slide: usize,
        role: Role,
        description: String,
    },
}

impl Change {
    /// The one-based slide this change belongs to.
    pub fn slide(&self) -> usize {
        match self {
            Change::SlideAdded { slide, .. }
            | Change::SlideRemoved { slide, .. }
            | Change::TitleChanged { slide, .. }
            | Change::BlockAdded { slide, .. }
            | Change::BlockRemoved { slide, .. }
            | Change::ParagraphAdded { slide, .. }
            | Change::ParagraphRemoved { slide, .. }
            | Change::ParagraphChanged { slide, .. }
            | Change::ParagraphFormattingChanged { slide, .. }
            | Change::TableCellChanged { slide, .. }
            | Change::TableRowAdded { slide, .. }
            | Change::TableRowRemoved { slide, .. }
            | Change::TableReshaped { slide, .. }
            | Change::ChartSeriesAdded { slide, .. }
            | Change::ChartSeriesRemoved { slide, .. }
            | Change::ChartSeriesRenamed { slide, .. }
            | Change::ChartCategoryChanged { slide, .. }
            | Change::ChartValueChanged { slide, .. }
            | Change::MediaChanged { slide, .. } => *slide,
        }
    }

    /// One human-readable line, without the slide prefix.
    fn describe(&self) -> String {
        match self {
            Change::SlideAdded { title, .. } => match title {
                Some(t) => format!("+ slide \"{t}\""),
                None => "+ slide (untitled)".to_string(),
            },
            Change::SlideRemoved { title, .. } => match title {
                Some(t) => format!("- slide \"{t}\""),
                None => "- slide (untitled)".to_string(),
            },
            Change::TitleChanged { from, to, .. } => {
                format!("~ title \"{from}\" -> \"{to}\"")
            }
            Change::BlockAdded {
                role, description, ..
            } => {
                format!("+ {role}: {description}")
            }
            Change::BlockRemoved {
                role, description, ..
            } => {
                format!("- {role}: {description}")
            }
            Change::ParagraphAdded {
                role, level, text, ..
            } => {
                format!("+ {role} bullet (level {level}) \"{text}\"")
            }
            Change::ParagraphRemoved {
                role, level, text, ..
            } => {
                format!("- {role} bullet (level {level}) \"{text}\"")
            }
            Change::ParagraphChanged {
                role,
                level,
                from,
                to,
                ..
            } => {
                format!("~ {role} bullet (level {level}) \"{from}\" -> \"{to}\"")
            }
            Change::ParagraphFormattingChanged { role, text, .. } => {
                format!("~ formatting on {role} \"{text}\"")
            }
            Change::TableCellChanged {
                row, col, from, to, ..
            } => {
                format!("~ table cell ({row},{col}) \"{from}\" -> \"{to}\"")
            }
            Change::TableRowAdded { row, cells, .. } => {
                format!("+ table row {row} {cells:?}")
            }
            Change::TableRowRemoved { row, cells, .. } => {
                format!("- table row {row} {cells:?}")
            }
            Change::TableReshaped { description, .. } => format!("~ table {description}"),
            Change::ChartSeriesAdded { name, .. } => format!("+ chart series \"{name}\""),
            Change::ChartSeriesRemoved { name, .. } => format!("- chart series \"{name}\""),
            Change::ChartSeriesRenamed { from, to, .. } => {
                format!("~ chart series \"{from}\" -> \"{to}\"")
            }
            Change::ChartCategoryChanged {
                series,
                index,
                from,
                to,
                ..
            } => {
                format!("~ chart category {index} of \"{series}\" \"{from}\" -> \"{to}\"")
            }
            Change::ChartValueChanged {
                series,
                category,
                from,
                to,
                ..
            } => {
                format!("~ chart value \"{series}\" / \"{category}\" {from} -> {to}")
            }
            Change::MediaChanged {
                role, description, ..
            } => {
                format!("~ {role}: {description}")
            }
        }
    }
}

/// The complete semantic difference between two decks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeckDiff {
    /// Slide count of the old deck.
    pub old_slides: usize,
    /// Slide count of the new deck.
    pub new_slides: usize,
    /// Every difference found, in slide order.
    pub changes: Vec<Change>,
}

impl DeckDiff {
    /// `true` when the two decks are semantically identical.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// A human-readable report, the shape `deckr diff` prints.
    pub fn to_text(&self, old_label: &str, new_label: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("{old_label} -> {new_label}\n"));
        out.push_str(&format!(
            "  slides: {} -> {}\n",
            self.old_slides, self.new_slides
        ));
        if self.is_empty() {
            out.push_str("  no semantic differences\n");
            return out;
        }
        out.push_str(&format!("  {} change(s):\n", self.changes.len()));
        for change in &self.changes {
            out.push_str(&format!(
                "    slide {}: {}\n",
                change.slide(),
                change.describe()
            ));
        }
        out
    }
}

/// Diff two `.pptx` files on disk.
pub fn diff_files(old: &Path, new: &Path) -> Result<DeckDiff> {
    Ok(diff_decks(&crate::read_pptx(old)?, &crate::read_pptx(new)?))
}

/// Diff two decks on the IR level.
///
/// Slides are paired by position (both decks carry presentation order), and
/// within a slide blocks are paired by role, preserving each role group's own
/// order. Everything unpaired is reported as added or removed.
pub fn diff_decks(old: &Deck, new: &Deck) -> DeckDiff {
    let mut diff = DeckDiff {
        old_slides: old.len(),
        new_slides: new.len(),
        changes: Vec::new(),
    };

    let shared = old.slides.len().min(new.slides.len());
    for i in 0..shared {
        diff_slide(
            old.slides[i].index + 1,
            &old.slides[i],
            &new.slides[i],
            &mut diff.changes,
        );
    }
    // Slides that exist on one side only.
    for slide in &old.slides[shared..] {
        diff.changes.push(Change::SlideRemoved {
            slide: slide.index + 1,
            title: slide.title(),
        });
    }
    for slide in &new.slides[shared..] {
        diff.changes.push(Change::SlideAdded {
            slide: slide.index + 1,
            title: slide.title(),
        });
    }
    diff
}

/// The slide's non-chrome, non-empty blocks — the content a diff cares about.
///
/// When `skip_titles` is set the title text blocks are left out as well: their
/// change is already carried by a `TitleChanged` headline, and diffing them
/// again would report the same edit twice.
fn content_blocks(s: &Slide, skip_titles: bool) -> Vec<&Block> {
    s.blocks
        .iter()
        .filter(|b| {
            !b.role.is_chrome()
                && !b.is_empty()
                && !(skip_titles
                    && matches!(b.role, Role::Title | Role::CenteredTitle)
                    && b.as_text().is_some())
        })
        .collect()
}

/// Diff one aligned pair of slides.
fn diff_slide(number: usize, old: &Slide, new: &Slide, out: &mut Vec<Change>) {
    // Titles are the slide's headline — compare them first so the report leads
    // with the change a human would name first.
    let titles_differ = matches!(
        (old.title(), new.title()),
        (Some(a), Some(b)) if a != b
    );
    if titles_differ {
        out.push(Change::TitleChanged {
            slide: number,
            from: old.title().unwrap_or_default(),
            to: new.title().unwrap_or_default(),
        });
    }

    // Content blocks by role, page furniture excluded: it is regenerated by
    // the master on rebuild, so it is noise in a semantic diff. When the
    // headline above already carries the title change, the title block itself
    // is excluded too — otherwise the same edit would be reported twice.
    let (olds, news) = (
        content_blocks(old, titles_differ),
        content_blocks(new, titles_differ),
    );

    // Pair by role, in order within each role. A linear scan that only ever
    // matches equal roles gives the same pairing as grouping would, without
    // allocating the groups.
    let mut oi = 0usize;
    let mut ni = 0usize;
    while oi < olds.len() && ni < news.len() {
        if olds[oi].role == news[ni].role {
            diff_block(number, olds[oi], news[ni], out);
            oi += 1;
            ni += 1;
        } else if role_rank(olds[oi].role) < role_rank(news[ni].role) {
            out.push(Change::BlockRemoved {
                slide: number,
                role: olds[oi].role,
                description: describe_block(olds[oi]),
            });
            oi += 1;
        } else {
            out.push(Change::BlockAdded {
                slide: number,
                role: news[ni].role,
                description: describe_block(news[ni]),
            });
            ni += 1;
        }
    }
    for b in &olds[oi..] {
        out.push(Change::BlockRemoved {
            slide: number,
            role: b.role,
            description: describe_block(b),
        });
    }
    for b in &news[ni..] {
        out.push(Change::BlockAdded {
            slide: number,
            role: b.role,
            description: describe_block(b),
        });
    }
}

/// Sort key so both sides' blocks meet in role order (the role's enum
/// discriminant is a stable order).
fn role_rank(role: Role) -> u8 {
    role as u8
}

/// Diff one aligned pair of same-role blocks.
fn diff_block(number: usize, old: &Block, new: &Block, out: &mut Vec<Change>) {
    match (&old.content, &new.content) {
        (BlockContent::Text(a), BlockContent::Text(b)) => diff_text(number, old.role, a, b, out),
        (BlockContent::Table { rows: a }, BlockContent::Table { rows: b }) => {
            diff_table(number, a, b, out)
        }
        (
            BlockContent::Picture {
                alt: aa, data: da, ..
            },
            BlockContent::Picture {
                alt: ab, data: db, ..
            },
        ) => {
            match (aa, ab) {
                (Some(x), Some(y)) if x != y => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: format!("alt text \"{x}\" -> \"{y}\""),
                }),
                (Some(x), None) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: format!("alt text \"{x}\" removed"),
                }),
                (None, Some(y)) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: format!("alt text \"{y}\" added"),
                }),
                _ => {}
            }
            match (da, db) {
                (Some(x), Some(y)) if x != y => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: "picture replaced".to_string(),
                }),
                (Some(_), None) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: "picture bytes dropped".to_string(),
                }),
                (None, Some(_)) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: "picture bytes added".to_string(),
                }),
                _ => {}
            }
        }
        (
            BlockContent::Chart {
                caption: ca,
                data: da,
                blob: ba,
                ..
            },
            BlockContent::Chart {
                caption: cb,
                data: db,
                blob: bb,
                ..
            },
        ) => {
            if ca != cb {
                out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: format!("chart caption {} -> {}", quote_opt(ca), quote_opt(cb)),
                });
            }
            match (da, db) {
                // Both sides decoded: the diff speaks in numbers.
                (Some(a), Some(b)) => diff_chart_data(number, a, b, out),
                (Some(_), None) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: "chart data dropped".to_string(),
                }),
                (None, Some(_)) => out.push(Change::MediaChanged {
                    slide: number,
                    role: old.role,
                    description: "chart data added".to_string(),
                }),
                // Neither side decoded: fall back to a byte comparison of the
                // real chart XML — still a reportable change even though it
                // cannot be named.
                (None, None) => {
                    let differ = match (blob_chart_part(ba), blob_chart_part(bb)) {
                        (Some(x), Some(y)) => x.bytes != y.bytes,
                        (Some(_), None) | (None, Some(_)) => true,
                        (None, None) => false,
                    };
                    if differ {
                        out.push(Change::MediaChanged {
                            slide: number,
                            role: old.role,
                            description: "chart content changed".to_string(),
                        });
                    }
                }
            }
        }
        (a, b) if a != b => out.push(Change::MediaChanged {
            slide: number,
            role: old.role,
            description: format!("content changed ({}, {})", kind_of(a), kind_of(b)),
        }),
        _ => {}
    }
}

/// Diff two aligned text blocks, paragraph by paragraph.
fn diff_text(number: usize, role: Role, a: &TextContent, b: &TextContent, out: &mut Vec<Change>) {
    let pa: Vec<&Paragraph> = a
        .paragraphs
        .iter()
        .filter(|p| !p.text.trim().is_empty())
        .collect();
    let pb: Vec<&Paragraph> = b
        .paragraphs
        .iter()
        .filter(|p| !p.text.trim().is_empty())
        .collect();

    let shared = pa.len().min(pb.len());
    for i in 0..shared {
        let (x, y) = (pa[i], pb[i]);
        let (xa, ya) = (x.text.trim(), y.text.trim());
        if xa != ya {
            out.push(Change::ParagraphChanged {
                slide: number,
                role,
                level: y.level,
                from: xa.to_string(),
                to: ya.to_string(),
            });
        } else if x.level != y.level {
            // An indent change is a structural edit; report it as a rewrite so
            // the outline stays honest.
            out.push(Change::ParagraphChanged {
                slide: number,
                role,
                level: y.level,
                from: format!("(level {}) {xa}", x.level),
                to: format!("(level {}) {ya}", y.level),
            });
        } else if x.display_runs() != y.display_runs() {
            out.push(Change::ParagraphFormattingChanged {
                slide: number,
                role,
                text: xa.to_string(),
            });
        }
    }
    for p in &pa[shared..] {
        out.push(Change::ParagraphRemoved {
            slide: number,
            role,
            level: p.level,
            text: p.text.trim().to_string(),
        });
    }
    for p in &pb[shared..] {
        out.push(Change::ParagraphAdded {
            slide: number,
            role,
            level: p.level,
            text: p.text.trim().to_string(),
        });
    }
}

/// Diff two aligned tables, row by row, cell by cell.
fn diff_table(number: usize, a: &[Vec<String>], b: &[Vec<String>], out: &mut Vec<Change>) {
    let shared = a.len().min(b.len());
    for r in 0..shared {
        let (ra, rb) = (&a[r], &b[r]);
        if ra.len() != rb.len() {
            out.push(Change::TableReshaped {
                slide: number,
                description: format!(
                    "row {} now has {} column(s), had {}",
                    r + 1,
                    rb.len(),
                    ra.len()
                ),
            });
            continue;
        }
        for c in 0..ra.len() {
            if ra[c] != rb[c] {
                out.push(Change::TableCellChanged {
                    slide: number,
                    row: r + 1,
                    col: c + 1,
                    from: ra[c].clone(),
                    to: rb[c].clone(),
                });
            }
        }
    }
    for (k, row) in a[shared..].iter().enumerate() {
        out.push(Change::TableRowRemoved {
            slide: number,
            row: shared + k + 1,
            cells: row.clone(),
        });
    }
    for (k, row) in b[shared..].iter().enumerate() {
        out.push(Change::TableRowAdded {
            slide: number,
            row: shared + k + 1,
            cells: row.clone(),
        });
    }
}

/// A short, human description of a block for added/removed lines.
fn describe_block(b: &Block) -> String {
    match &b.content {
        BlockContent::Text(t) => {
            let first = t
                .paragraphs
                .iter()
                .map(|p| p.text.trim())
                .find(|s| !s.is_empty())
                .unwrap_or("(empty)");
            format!("\"{}\"", truncate(first, 60))
        }
        BlockContent::Table { rows } => format!("table ({} row(s))", rows.len()),
        BlockContent::Picture { alt, .. } => match alt {
            Some(a) => format!("picture \"{a}\""),
            None => "picture".to_string(),
        },
        BlockContent::Chart { caption, .. } => match caption {
            Some(c) => format!("chart \"{c}\""),
            None => "chart".to_string(),
        },
        BlockContent::Diagram { caption } => match caption {
            Some(c) => format!("diagram \"{c}\""),
            None => "diagram".to_string(),
        },
        BlockContent::Empty => "(empty)".to_string(),
    }
}

fn kind_of(content: &BlockContent) -> &'static str {
    match content {
        BlockContent::Text(_) => "text",
        BlockContent::Picture { .. } => "picture",
        BlockContent::Table { .. } => "table",
        BlockContent::Chart { .. } => "chart",
        BlockContent::Diagram { .. } => "diagram",
        BlockContent::Empty => "empty",
    }
}

/// The real chart XML part of a captured blob, if the blob carries one.
fn blob_chart_part(b: &Option<ChartBlob>) -> Option<&crate::ir::ChartPart> {
    b.as_ref().and_then(crate::chart::chart_xml_part)
}

/// Diff two decoded charts, series by series and point by point.
///
/// Series are paired by index (a chart's series order is meaningful), so a
/// rename and a reorder look different — which is the honest reading. A
/// structural mismatch beyond that (categories appearing or vanishing) is one
/// `MediaChanged` per series rather than a storm of per-point reports.
fn diff_chart_data(number: usize, a: &ChartData, b: &ChartData, out: &mut Vec<Change>) {
    let shared = a.series.len().min(b.series.len());
    for i in shared..a.series.len() {
        out.push(Change::ChartSeriesRemoved {
            slide: number,
            name: a.series_label(i),
        });
    }
    for i in shared..b.series.len() {
        out.push(Change::ChartSeriesAdded {
            slide: number,
            name: b.series_label(i),
        });
    }

    for i in 0..shared {
        let (sa, sb) = (&a.series[i], &b.series[i]);
        let label = b.series_label(i);
        let name = |s: &Option<String>| s.clone().unwrap_or_else(|| label.clone());

        match (sa.name.clone(), sb.name.clone()) {
            (Some(x), Some(y)) if x != y => {
                out.push(Change::ChartSeriesRenamed {
                    slide: number,
                    from: x,
                    to: y,
                });
            }
            _ => {}
        }

        // Category labels, position by position.
        let cat_shared = sa.categories.len().min(sb.categories.len());
        for c in 0..cat_shared {
            if sa.categories[c] != sb.categories[c] {
                out.push(Change::ChartCategoryChanged {
                    slide: number,
                    series: name(&sa.name),
                    index: c + 1,
                    from: sa.categories[c].clone(),
                    to: sb.categories[c].clone(),
                });
            }
        }

        // Values, position by position. `(empty)` is the missing-cell mark —
        // a gap and a zero are different things and must not diff as equal.
        let val_shared = sa.values.len().min(sb.values.len());
        for c in 0..val_shared {
            if sa.values[c] != sb.values[c] {
                let category = sb
                    .categories
                    .get(c)
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("point {}", c + 1));
                out.push(Change::ChartValueChanged {
                    slide: number,
                    series: name(&sa.name),
                    category,
                    from: fmt_point(sa.values[c]),
                    to: fmt_point(sb.values[c]),
                });
            }
        }

        if sa.categories.len() != sb.categories.len() || sa.values.len() != sb.values.len() {
            out.push(Change::MediaChanged {
                slide: number,
                role: Role::Chart,
                description: format!(
                    "series \"{label}\" changed shape ({} -> {} categories)",
                    sa.categories.len().max(sa.values.len()),
                    sb.categories.len().max(sb.values.len()),
                ),
            });
        }
    }
}

/// One plotted point as text: `3`, `1.5`, or `(empty)` for a gap.
fn fmt_point(v: Option<f64>) -> String {
    match v {
        Some(v) => {
            if v.fract() == 0.0 && v.abs() < 1e15 {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            }
        }
        None => "(empty)".to_string(),
    }
}

fn quote_opt(s: &Option<String>) -> String {
    match s {
        Some(v) => format!("\"{v}\""),
        None => "(none)".to_string(),
    }
}

/// First `max` characters plus an ellipsis when longer.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BlockContent, Paragraph, Run, TextContent};

    fn text(role: Role, paragraphs: Vec<(&str, u8)>) -> Block {
        Block {
            role,
            content: BlockContent::Text(TextContent {
                paragraphs: paragraphs
                    .into_iter()
                    .map(|(t, l)| Paragraph::new(l, t))
                    .collect(),
            }),
        }
    }

    fn slide(index: usize, blocks: Vec<Block>) -> Slide {
        Slide { index, blocks }
    }

    fn deck(slides: Vec<Slide>) -> Deck {
        Deck { slides }
    }

    #[test]
    fn identical_decks_report_nothing() {
        let d = deck(vec![slide(
            0,
            vec![
                text(Role::Title, vec![("Agenda", 0)]),
                text(Role::Body, vec![("one", 0), ("two", 1)]),
            ],
        )]);
        let diff = diff_decks(&d, &d);
        assert!(diff.is_empty(), "{:?}", diff.changes);
    }

    #[test]
    fn a_title_change_is_the_headline() {
        let old = deck(vec![slide(
            0,
            vec![text(Role::Title, vec![("Q2 Results", 0)])],
        )]);
        let new = deck(vec![slide(
            0,
            vec![text(Role::Title, vec![("Q3 Results", 0)])],
        )]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![Change::TitleChanged {
                slide: 1,
                from: "Q2 Results".into(),
                to: "Q3 Results".into()
            }]
        );
    }

    #[test]
    fn bullets_are_reported_per_paragraph() {
        let old = deck(vec![slide(
            0,
            vec![
                text(Role::Title, vec![("Roadmap", 0)]),
                text(Role::Body, vec![("keep it simple", 0), ("drop this", 1)]),
            ],
        )]);
        let new = deck(vec![slide(
            0,
            vec![
                text(Role::Title, vec![("Roadmap", 0)]),
                text(
                    Role::Body,
                    vec![
                        ("keep it simple", 0),
                        ("ship the reader", 1),
                        ("new one", 1),
                    ],
                ),
            ],
        )]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![
                Change::ParagraphChanged {
                    slide: 1,
                    role: Role::Body,
                    level: 1,
                    from: "drop this".into(),
                    to: "ship the reader".into(),
                },
                Change::ParagraphAdded {
                    slide: 1,
                    role: Role::Body,
                    level: 1,
                    text: "new one".into(),
                },
            ]
        );
    }

    #[test]
    fn a_new_slide_is_added_with_its_title() {
        let old = deck(vec![slide(0, vec![text(Role::Title, vec![("First", 0)])])]);
        let new = deck(vec![
            slide(0, vec![text(Role::Title, vec![("First", 0)])]),
            slide(1, vec![text(Role::Title, vec![("Fresh page", 0)])]),
        ]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![Change::SlideAdded {
                slide: 2,
                title: Some("Fresh page".into())
            }]
        );
    }

    #[test]
    fn table_cells_are_compared_individually() {
        let table = |rows: Vec<Vec<&str>>| Block {
            role: Role::Table,
            content: BlockContent::Table {
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(String::from).collect())
                    .collect(),
            },
        };
        let old = deck(vec![slide(
            0,
            vec![table(vec![
                vec!["Phase", "Target"],
                vec!["0.1 read", "Q3"],
                vec!["0.2 write", "Q4"],
            ])],
        )]);
        let new = deck(vec![slide(
            0,
            vec![table(vec![
                vec!["Phase", "Target"],
                vec!["0.1 read", "Q3"],
                vec!["0.2 write", "4.1%"],
            ])],
        )]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![Change::TableCellChanged {
                slide: 1,
                row: 3,
                col: 2,
                from: "Q4".into(),
                to: "4.1%".into(),
            }]
        );
    }

    #[test]
    fn table_rows_added_and_removed_are_whole_rows() {
        let table = |rows: Vec<Vec<&str>>| Block {
            role: Role::Table,
            content: BlockContent::Table {
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(String::from).collect())
                    .collect(),
            },
        };
        let old = deck(vec![slide(
            0,
            vec![table(vec![vec!["a", "b"], vec!["c", "d"], vec!["e", "f"]])],
        )]);
        let new = deck(vec![slide(
            0,
            vec![table(vec![vec!["a", "b"], vec!["g", "h"]])],
        )]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![
                Change::TableCellChanged {
                    slide: 1,
                    row: 2,
                    col: 1,
                    from: "c".into(),
                    to: "g".into(),
                },
                Change::TableCellChanged {
                    slide: 1,
                    row: 2,
                    col: 2,
                    from: "d".into(),
                    to: "h".into(),
                },
                Change::TableRowRemoved {
                    slide: 1,
                    row: 3,
                    cells: vec!["e".into(), "f".into()],
                },
            ]
        );
    }

    #[test]
    fn formatting_only_changes_are_their_own_kind() {
        let plain = Block {
            role: Role::Body,
            content: BlockContent::Text(TextContent {
                paragraphs: vec![Paragraph::new(0, "emphasis")],
            }),
        };
        let bold = Block {
            role: Role::Body,
            content: BlockContent::Text(TextContent {
                paragraphs: vec![Paragraph::from_runs(
                    0,
                    vec![Run {
                        bold: Some(true),
                        ..Run::new("emphasis")
                    }],
                )],
            }),
        };
        let old = deck(vec![slide(0, vec![plain])]);
        let new = deck(vec![slide(0, vec![bold])]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![Change::ParagraphFormattingChanged {
                slide: 1,
                role: Role::Body,
                text: "emphasis".into(),
            }]
        );
    }

    #[test]
    fn page_furniture_is_not_noise() {
        let with_furniture = |n: &str| {
            slide(
                0,
                vec![
                    text(Role::Title, vec![(n, 0)]),
                    text(Role::SlideNumber, vec![("7", 0)]),
                    text(Role::Footer, vec![("Confidential", 0)]),
                ],
            )
        };
        let old = deck(vec![with_furniture("Same")]);
        let new = deck(vec![with_furniture("Same")]);
        let diff = diff_decks(&old, &new);
        assert!(diff.is_empty(), "{:?}", diff.changes);
    }

    #[test]
    fn a_removed_block_is_named() {
        let old = deck(vec![slide(
            0,
            vec![
                text(Role::Title, vec![("T", 0)]),
                text(Role::Body, vec![("gone", 0)]),
            ],
        )]);
        let new = deck(vec![slide(0, vec![text(Role::Title, vec![("T", 0)])])]);
        let diff = diff_decks(&old, &new);
        assert_eq!(
            diff.changes,
            vec![Change::BlockRemoved {
                slide: 1,
                role: Role::Body,
                description: "\"gone\"".into(),
            }]
        );
    }

    #[test]
    fn the_report_reads_like_prose() {
        let old = deck(vec![slide(0, vec![text(Role::Title, vec![("Q2", 0)])])]);
        let new = deck(vec![slide(0, vec![text(Role::Title, vec![("Q3", 0)])])]);
        let text = diff_decks(&old, &new).to_text("old.pptx", "new.pptx");
        assert!(text.contains("old.pptx -> new.pptx"));
        assert!(text.contains("slides: 1 -> 1"));
        assert!(text.contains("slide 1: ~ title \"Q2\" -> \"Q3\""));
    }
}
