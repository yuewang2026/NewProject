//! Deck IR -> `.pptx`.
//!
//! The writing half, and the reason the IR exists at all. Nothing here accepts a
//! coordinate: each [`Block`](crate::ir::Block) declares a [`Role`], this module
//! finds the placeholder of that role on the slide's layout, and drops the content into
//! it. Position, fonts, colour and bullet glyphs stay the layout's business, so
//! the generated deck looks like it was made in PowerPoint — structurally, it
//! was.
//!
//! The one thing this does *not* do yet is reuse an existing corporate template;
//! `parts.rs` ships a small default theme. Swapping in a template is a change
//! confined to that module.

use std::io::{Seek, Write};
use std::path::Path;

use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::error::{Error, Result};
use crate::ir::{BlockContent, Deck, Paragraph, Role, Run, Slide, TextContent};
use crate::parts;

/// Height of one table row: 0.405in, Office's own default.
const ROW_HEIGHT: i64 = 370_332;
/// Top of the content area on every layout we ship.
const CONTENT_TOP: i64 = 2_019_300;
const CONTENT_WIDTH: i64 = 10_515_600;
const CONTENT_LEFT: i64 = 838_200;
/// Roughly half the content area: where unbound content starts once body text
/// has claimed the top of it.
const CONTENT_SLOT_HALF: i64 = 2_100_000;
/// Line box used to size text that has no placeholder to inherit a height from.
/// The shape carries `normAutofit`, so this only has to be roughly right.
const LINE_HEIGHT: i64 = 320_000;

/// What came out of a build, including everything that could not be placed.
#[derive(Debug, Default)]
pub struct BuildReport {
    pub slides: usize,
    pub blocks_written: usize,
    /// Blocks with no placeholder on the chosen layout. Reported rather than
    /// silently dropped — the whole promise of an IR is that loss is visible.
    pub skipped: Vec<String>,
    /// Blocks whose content survived but not their binding: text that had no
    /// placeholder available and was placed as free-standing shape instead.
    pub relocated: Vec<String>,
}

impl BuildReport {
    /// Zero indicates a perfect write; anything above it is damage.
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

/// Serialise `deck` into a `.pptx` at `path`.
pub fn write_pptx_file(deck: &Deck, path: &Path) -> Result<BuildReport> {
    let file = std::fs::File::create(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    write_pptx(deck, file)
}

/// Serialise `deck` into a `.pptx`.
pub fn write_pptx<W: Write + Seek>(deck: &Deck, sink: W) -> Result<BuildReport> {
    let layouts = parts::layouts();
    let slide_count = deck.len();
    if slide_count == 0 {
        return Err(Error::Other(
            "refusing to build a deck with no slides".into(),
        ));
    }

    let mut report = BuildReport {
        slides: slide_count,
        ..Default::default()
    };

    // Every part of the package shares one timestamp, so two builds of the same
    // deck are byte-identical and therefore diffable.
    let stamped = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip_datetime());

    let mut zip = ZipWriter::new(sink);

    put(
        &mut zip,
        stamped,
        "[Content_Types].xml",
        &parts::content_types_xml(slide_count, layouts.len()),
    )?;
    put(&mut zip, stamped, "_rels/.rels", &parts::root_rels_xml())?;
    put(
        &mut zip,
        stamped,
        "docProps/core.xml",
        &parts::core_props_xml(deck.slides[0].title().as_deref().unwrap_or("Deck")),
    )?;
    put(
        &mut zip,
        stamped,
        "docProps/app.xml",
        &parts::app_props_xml(slide_count),
    )?;

    put(
        &mut zip,
        stamped,
        "ppt/presentation.xml",
        &parts::presentation_xml(slide_count),
    )?;
    put(
        &mut zip,
        stamped,
        "ppt/_rels/presentation.xml.rels",
        &parts::presentation_rels_xml(slide_count),
    )?;
    put(&mut zip, stamped, "ppt/theme/theme1.xml", parts::THEME_XML)?;

    put(
        &mut zip,
        stamped,
        "ppt/slideMasters/slideMaster1.xml",
        &parts::slide_master_xml(layouts.len()),
    )?;
    put(
        &mut zip,
        stamped,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        &parts::master_rels_xml(layouts.len()),
    )?;

    for layout in &layouts {
        put(
            &mut zip,
            stamped,
            &format!("ppt/slideLayouts/{}", layout.file),
            &parts::slide_layout_xml(layout),
        )?;
        put(
            &mut zip,
            stamped,
            &format!("ppt/slideLayouts/_rels/{}.rels", layout.file),
            &parts::layout_rels_xml(),
        )?;
    }

    for (i, slide) in deck.slides.iter().enumerate() {
        let layout_index = layout_for(slide);
        let mut table = LinkTable::default();
        let body = slide_xml(slide, layout_index, &mut table, &mut report);
        put(
            &mut zip,
            stamped,
            &format!("ppt/slides/slide{}.xml", i + 1),
            &body,
        )?;
        put(
            &mut zip,
            stamped,
            &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
            &parts::slide_rels_xml(layout_index, &table.links),
        )?;
    }

    zip.finish().map_err(|e| Error::Io {
        path: "package".into(),
        source: std::io::Error::other(e),
    })?;
    Ok(report)
}

fn zip_datetime() -> zip::DateTime {
    zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).expect("a valid ZIP timestamp")
}

#[allow(clippy::needless_pass_by_value)]
fn put<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    opts: SimpleFileOptions,
    name: &str,
    body: &str,
) -> Result<()> {
    zip.start_file(name, opts).map_err(|e| Error::Io {
        path: name.into(),
        source: std::io::Error::other(e),
    })?;
    zip.write_all(body.as_bytes()).map_err(|e| Error::Io {
        path: name.into(),
        source: e,
    })
}

/// Which layout a slide deserves, judged only by what it contains.
///
/// A slide that is nothing but a title (and maybe a subtitle) wants the title
/// layout; anything with content wants "Title and Content". This is what a human
/// chooses in PowerPoint's layout gallery, derived rather than configured.
fn layout_for(slide: &Slide) -> usize {
    const TITLE_SLIDE: usize = 0;
    const TITLE_AND_CONTENT: usize = 1;

    // Judge by the roles that actually need a slot. A slide carrying nothing but
    // a title and subtitle is a section divider even if a chart rides along —
    // charts and pictures do not occupy the content placeholder.
    let needs_content_slot = slide
        .blocks
        .iter()
        .filter(|b| !b.is_empty() && !b.role.is_chrome())
        .any(|b| matches!(b.role, Role::Body | Role::Object | Role::Table));
    if needs_content_slot {
        TITLE_AND_CONTENT
    } else {
        TITLE_SLIDE
    }
}

/// The placeholder type a role binds to on a given layout, if any.
fn placeholder_for(layout_index: usize, role: Role) -> Option<&'static str> {
    // Index 0 is "Title Slide": ctrTitle + subTitle.
    // Index 1 is "Title and Content": title + body.
    match (layout_index, role) {
        (0, Role::Title | Role::CenteredTitle) => Some("ctrTitle"),
        (0, Role::Subtitle) => Some("subTitle"),
        (1, Role::Title | Role::CenteredTitle) => Some("title"),
        (1, Role::Body | Role::Object) => Some("body"),
        _ => None,
    }
}

fn slide_xml(
    slide: &Slide,
    layout_index: usize,
    links: &mut LinkTable,
    report: &mut BuildReport,
) -> String {
    let layouts = parts::layouts();
    let layout = &layouts[layout_index];

    let mut shapes = String::new();
    // Shape id 1 belongs to the shape tree itself.
    let mut next_id = 2u32;
    // Decide up front where unbound content starts. If some block is about to
    // claim the content placeholder, loose shapes begin below it rather than on
    // top of it; otherwise they begin at the top of the content area, advancing
    // as they go so they stack instead of overlapping.
    let claims_content_slot = slide
        .blocks
        .iter()
        .filter(|b| !b.is_empty() && !b.role.is_chrome())
        .any(|b| placeholder_for(layout_index, b.role) == Some("body"));
    let mut free_top = layout
        .placeholder("body")
        .filter(|_| claims_content_slot)
        .map_or(CONTENT_TOP, |ph| ph.y + CONTENT_SLOT_HALF);

    for block in &slide.blocks {
        if block.is_empty() || block.role.is_chrome() {
            continue;
        }
        match &block.content {
            BlockContent::Text(t) => {
                match placeholder_for(layout_index, block.role) {
                    Some(ph_type) => {
                        let Some(ph) = layout.placeholder(ph_type) else {
                            report.skipped.push(format!(
                                "slide {}: layout is missing its {} placeholder",
                                slide.index + 1,
                                ph_type
                            ));
                            continue;
                        };
                        shapes.push_str(&text_shape(
                            next_id,
                            &format!("{} Placeholder", block.role.as_str()),
                            ph_type,
                            ph,
                            t,
                            links,
                        ));
                        next_id += 1;
                        report.blocks_written += 1;
                    }
                    // No placeholder of this role exists on the chosen layout.
                    // Dropping the text would be the easy answer and it is the
                    // wrong one: the reader promised to account for every
                    // paragraph, so bind it loose. It loses its inheritance —
                    // which is exactly what the report line below says.
                    None => {
                        let height = free_height_of(t);
                        shapes.push_str(&free_shape(
                            next_id,
                            &format!("{} (unbound)", block.role.as_str()),
                            Rect {
                                x: CONTENT_LEFT,
                                y: free_top,
                                cx: CONTENT_WIDTH,
                                cy: height,
                            },
                            t,
                            links,
                        ));
                        free_top += height;
                        next_id += 1;
                        report.blocks_written += 1;
                        report.relocated.push(format!(
                            "slide {}: '{}' has no placeholder on layout '{}' — \
                             placed loose, so it no longer follows the template",
                            slide.index + 1,
                            block.role,
                            layout.name
                        ));
                    }
                }
            }
            BlockContent::Table { rows } => {
                let height = rows.len() as i64 * ROW_HEIGHT;
                shapes.push_str(&table_shape(next_id, rows, CONTENT_LEFT, free_top));
                free_top += height;
                next_id += 1;
                report.blocks_written += 1;
            }
            // Round-tripping pictures and charts means carrying their media
            // parts; until 0.4 does that, saying so beats emitting a shape that
            // renders as an empty frame.
            BlockContent::Picture { .. } => report.skipped.push(format!(
                "slide {}: pictures need media extraction (0.4)",
                slide.index + 1
            )),
            BlockContent::Chart { .. } | BlockContent::Diagram { .. } => {
                report.skipped.push(format!(
                    "slide {}: {}-blocks need chart XML (0.4)",
                    slide.index + 1,
                    block.role
                ))
            }
            BlockContent::Empty => {}
        }
    }

    if shapes.is_empty() {
        report.skipped.push(format!(
            "slide {}: nothing placeable survived",
            slide.index + 1
        ));
    }

    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"",
            " xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"",
            " xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">",
            "<p:cSld><p:spTree>",
            "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
            "<p:grpSpPr><a:xfrm/></p:grpSpPr>{shapes}</p:spTree></p:cSld>",
            "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>",
            "</p:sld>"
        ),
        shapes = shapes,
    )
}

fn text_shape(
    id: u32,
    name: &str,
    ph_type: &str,
    ph: &parts::Placeholder,
    text: &TextContent,
    links: &mut LinkTable,
) -> String {
    let body = paragraphs_xml(text, links);
    format!(
        concat!(
            "<p:sp>",
            "<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr txBox=\"1\"/>",
            "<p:nvPr><p:ph type=\"{ph_type}\" idx=\"{idx}\"/></p:nvPr></p:nvSpPr>",
            // The coordinates come from the layout's own placeholder, not from
            // anything a caller supplied: they exist so that a consumer which
            // ignores placeholder inheritance still shows the shape in place.
            "<p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/>",
            "<a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>",
            "<p:txBody><a:bodyPr anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr>{body}</p:txBody>",
            "</p:sp>"
        ),
        id = id,
        name = parts::esc_attr(name),
        ph_type = ph_type,
        idx = ph.idx,
        x = ph.x,
        y = ph.y,
        cx = ph.cx,
        cy = ph.cy,
        anchor = ph.anchor,
        body = body,
    )
}

/// Height for text that has no layout placeholder to inherit a box from: one
/// line per paragraph plus slack. `normAutofit` means this only has to be in
/// the right ballpark for the first render.
fn free_height_of(text: &TextContent) -> i64 {
    let lines = text
        .paragraphs
        .iter()
        .filter(|p| !p.text.trim().is_empty())
        .count()
        .max(1) as i64;
    lines * LINE_HEIGHT + 100_000
}

/// A text shape carrying no `<p:ph>` at all.
///
/// Omitting `<p:ph/>` rather than writing an empty one is deliberate: ECMA-376
/// §19.3.1.25 says a `<p:ph/>` with no `type` defaults to `body`, so declaring
/// "some placeholder" here would re-bind the shape to the layout's body and
/// quietly restore the inheritance we just told the user was lost.
/// A rectangle in EMU. Exists mostly so the shape helpers take one geometry
/// argument rather than four indistinguishable integers.
#[derive(Clone, Copy)]
struct Rect {
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
}

fn free_shape(id: u32, name: &str, at: Rect, text: &TextContent, links: &mut LinkTable) -> String {
    format!(
        concat!(
            "<p:sp>",
            "<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/>",
            "<p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>",
            "<p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/>",
            "<a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr>",
            "<p:txBody><a:bodyPr wrap=\"square\"><a:normAutofit/></a:bodyPr>",
            // Nothing above it defines bullets, so declare an empty list style
            // rather than leaving readers to wonder which rule applies.
            "<a:lstStyle/>{body}</p:txBody>",
            "</p:sp>"
        ),
        id = id,
        name = parts::esc_attr(name),
        x = at.x,
        y = at.y,
        cx = at.cx,
        cy = at.cy,
        body = paragraphs_xml(text, links),
    )
}

fn paragraphs_xml(text: &TextContent, links: &mut LinkTable) -> String {
    let paragraphs: Vec<&Paragraph> = text
        .paragraphs
        .iter()
        .filter(|p| !p.text.trim().is_empty())
        .collect();
    if paragraphs.is_empty() {
        return "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>".to_string();
    }
    let mut out = String::new();
    for p in paragraphs {
        // Only the indent level appears here. The bullet glyph, hanging indent
        // and font come from the layout's list style, so slides stay dumb.
        out.push_str(&format!("<a:p><a:pPr lvl=\"{}\"/>", p.level));
        out.push_str(&runs_xml(p, links));
        out.push_str("</a:p>");
    }
    out
}

fn runs_xml(p: &Paragraph, links: &mut LinkTable) -> String {
    let mut out = String::new();
    for run in p.display_runs() {
        let mut first = true;
        for segment in run.text.split('\n') {
            if !first {
                // A soft break sits between runs rather than inside `<a:t>`,
                // because a text run may contain characters but not children.
                out.push_str("<a:br/>");
            }
            if !segment.is_empty() {
                out.push_str(&run_xml(segment, &run, links));
            }
            first = false;
        }
    }
    out
}

fn run_xml(text: &str, run: &Run, links: &mut LinkTable) -> String {
    format!(
        "<a:r>{pr}<a:t xml:space=\"preserve\">{text}</a:t></a:r>",
        pr = run_props_xml(run, links),
        text = parts::esc(text),
    )
}

fn run_props_xml(run: &Run, links: &mut LinkTable) -> String {
    let mut attrs = String::from(" lang=\"en-US\" dirty=\"0\"");
    let mut push = |name: &str, value: bool| attrs.push_str(&format!(" {name}=\"{}\"", bit(value)));
    if let Some(b) = run.bold {
        push("b", b);
    }
    if let Some(i) = run.italic {
        push("i", i);
    }
    if let Some(u) = run.underline {
        attrs.push_str(&format!(" u=\"{}\"", if u { "sng" } else { "none" }));
    }
    if let Some(s) = run.strike {
        attrs.push_str(&format!(
            " strike=\"{}\"",
            if s { "sngStrike" } else { "noStrike" }
        ));
    }
    if let Some(sz) = run.size {
        attrs.push_str(&format!(" sz=\"{sz}\""));
    }

    let mut inner = String::new();
    if let Some(color) = &run.color {
        inner.push_str(&format!(
            "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
            parts::esc_attr(color)
        ));
    }
    if let Some(link) = &run.link {
        inner.push_str(&format!(
            "<a:hlinkClick r:id=\"{}\"/>",
            parts::esc_attr(&links.intern(link))
        ));
    }

    if inner.is_empty() {
        format!("<a:rPr{attrs}/>")
    } else {
        format!("<a:rPr{attrs}>{inner}</a:rPr>")
    }
}

fn bit(b: bool) -> u8 {
    u8::from(b)
}

/// Keeps one relationship id per distinct URL, so the slide's rels part stays
/// free of duplicates.
#[derive(Debug, Default)]
struct LinkTable {
    links: Vec<String>,
}

impl LinkTable {
    fn intern(&mut self, target: &str) -> String {
        match self.links.iter().position(|l| l == target) {
            // Layout relationship occupies rId1, so links start at rId2.
            Some(i) => format!("rId{}", i + 2),
            None => {
                self.links.push(target.to_string());
                format!("rId{}", self.links.len() + 1)
            }
        }
    }
}

fn table_shape(id: u32, rows: &[Vec<String>], x: i64, y: i64) -> String {
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let col_w = CONTENT_WIDTH / width as i64;
    let mut grid = String::new();
    for _ in 0..width {
        grid.push_str(&format!("<a:gridCol w=\"{col_w}\"/>"));
    }

    let mut body_rows = String::new();
    for row in rows {
        body_rows.push_str(&format!("<a:tr h=\"{ROW_HEIGHT}\">"));
        for c in 0..width {
            let text = row.get(c).map(|s| s.trim()).unwrap_or_default();
            body_rows.push_str(&format!(
                concat!(
                    "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>",
                    "<a:p><a:r><a:rPr lang=\"en-US\" dirty=\"0\"/>",
                    "<a:t xml:space=\"preserve\">{text}</a:t></a:r></a:p>",
                    "</a:txBody><a:tcPr marL=\"68580\" marR=\"68580\" anchor=\"ctr\"/></a:tc>"
                ),
                text = parts::esc(text),
            ));
        }
        body_rows.push_str("</a:tr>");
    }

    let cy = rows.len() as i64 * ROW_HEIGHT;
    format!(
        concat!(
            "<p:graphicFrame>",
            "<p:nvGraphicFramePr>",
            "<p:cNvPr id=\"{id}\" name=\"Table {id}\"/>",
            "<p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr>",
            "<p:nvPr/>",
            "</p:nvGraphicFramePr>",
            "<p:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></p:xfrm>",
            "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">",
            "<a:tbl><a:tblPr firstRow=\"1\" bandRow=\"1\"/><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl>",
            "</a:graphicData></a:graphic>",
            "</p:graphicFrame>"
        ),
        id = id,
        x = x,
        y = y,
        cx = CONTENT_WIDTH,
        cy = cy,
        grid = grid,
        rows = body_rows,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, Deck, Slide};
    use std::collections::HashMap;

    fn two_slide_deck() -> Deck {
        Deck {
            slides: vec![
                Slide {
                    index: 0,
                    blocks: vec![
                        Block {
                            role: Role::CenteredTitle,
                            content: BlockContent::Text(TextContent {
                                paragraphs: vec![Paragraph::new(0, "Hello & welcome")],
                            }),
                        },
                        Block {
                            role: Role::Subtitle,
                            content: BlockContent::Text(TextContent {
                                paragraphs: vec![Paragraph::new(0, "Read it. Build it back.")],
                            }),
                        },
                    ],
                },
                Slide {
                    index: 1,
                    blocks: vec![
                        Block {
                            role: Role::Title,
                            content: BlockContent::Text(TextContent {
                                paragraphs: vec![Paragraph::new(0, "Roadmap")],
                            }),
                        },
                        Block {
                            role: Role::Body,
                            content: BlockContent::Text(TextContent {
                                paragraphs: vec![
                                    Paragraph::new(0, "ship the reader"),
                                    Paragraph::new(1, "keep it simple"),
                                ],
                            }),
                        },
                        Block {
                            role: Role::Table,
                            content: BlockContent::Table {
                                rows: vec![
                                    vec!["Phase".into(), "Target".into()],
                                    vec!["0.2 write".into(), "2026 Q4".into()],
                                ],
                            },
                        },
                    ],
                },
            ],
        }
    }

    /// Build into memory and hand back every part, decoded.
    ///
    /// Parts in the package are compressed, so assertions have to look at what
    /// came out rather than at the bytes.
    fn built_parts(deck: &Deck) -> (HashMap<String, String>, BuildReport) {
        use std::io::Read;
        let mut buf: Vec<u8> = Vec::new();
        let report = write_pptx(deck, std::io::Cursor::new(&mut buf)).expect("writes");
        let mut pkg = zip::ZipArchive::new(std::io::Cursor::new(&buf)).expect("a read zip");
        let mut out = HashMap::new();
        let names: Vec<String> = pkg.file_names().map(|s| s.to_string()).collect();
        for name in names {
            let mut s = String::new();
            pkg.by_name(&name)
                .expect("named part")
                .read_to_string(&mut s)
                .expect("utf8 part");
            out.insert(name, s);
        }
        (out, report)
    }

    #[test]
    fn the_package_carries_every_part_it_promises() {
        let (parts_map, _) = built_parts(&two_slide_deck());
        for required in [
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "ppt/theme/theme1.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            "ppt/slides/slide1.xml",
            "ppt/slides/_rels/slide2.xml.rels",
        ] {
            assert!(parts_map.contains_key(required), "missing {required}");
        }
        // Every part declared in content types has to exist.
        let types = &parts_map["[Content_Types].xml"];
        for part in types
            .split("<Override PartName=\"")
            .skip(1)
            .filter_map(|tail| tail.split('"').next())
        {
            let path = part.trim_start_matches('/');
            assert!(parts_map.contains_key(path), "declared but absent: {path}");
        }
    }

    #[test]
    fn a_title_only_slide_picks_the_title_layout() {
        let deck = two_slide_deck();
        assert_eq!(layout_for(&deck.slides[0]), 0);
        assert_eq!(layout_for(&deck.slides[1]), 1);
    }

    #[test]
    fn ampersands_are_escaped_not_swallowed() {
        let (parts_map, _) = built_parts(&two_slide_deck());
        assert!(
            parts_map["ppt/slides/slide1.xml"].contains("Hello &amp; welcome"),
            "{}",
            parts_map["ppt/slides/slide1.xml"]
        );
    }

    #[test]
    fn titles_bind_to_placeholders_and_bullets_carry_only_their_level() {
        let (parts_map, _) = built_parts(&two_slide_deck());
        let xml = &parts_map["ppt/slides/slide2.xml"];
        assert!(xml.contains("<p:ph type=\"title\""), "{xml}");
        assert!(xml.contains("<p:ph type=\"body\""), "{xml}");
        assert!(xml.contains("<a:pPr lvl=\"1\"/>"), "{xml}");
        // No list styling on the slide: the layout owns bullet glyphs.
        let body_from = xml.find("<p:ph type=\"body\"").expect("a body placeholder");
        let body_to = xml[body_from..].find("</p:sp>").expect("the body shape");
        let body_shape = &xml[body_from..body_from + body_to];
        assert!(!body_shape.contains("lstStyle"), "{body_shape}");
        // And the layout really does define them.
        assert!(
            parts_map["ppt/slideLayouts/slideLayout2.xml"].contains("<a:lstStyle>"),
            "layouts must carry bullet definitions"
        );
    }

    #[test]
    fn tables_become_graphic_frames() {
        let (parts_map, _) = built_parts(&two_slide_deck());
        let xml = &parts_map["ppt/slides/slide2.xml"];
        assert!(
            xml.contains(
                "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">"
            ),
            "{xml}"
        );
        assert!(
            xml.contains("<a:tblPr firstRow=\"1\" bandRow=\"1\"/>"),
            "{xml}"
        );
        // Cell properties come *after* the text body in DrawingML.
        let tc = xml.find("<a:tc>").expect("a cell");
        let body_pr = xml[tc..].find("</a:txBody>").expect("cell text");
        let tc_pr = xml[tc..].find("<a:tcPr").expect("cell props");
        assert!(body_pr < tc_pr, "txBody must precede tcPr");
    }

    #[test]
    fn generated_package_reads_back_as_a_deck() {
        let deck = two_slide_deck();
        let mut buf: Vec<u8> = Vec::new();
        write_pptx(&deck, std::io::Cursor::new(&mut buf)).expect("writes");
        let dir = std::env::temp_dir().join(format!("deckr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("roundtrip.pptx");
        std::fs::write(&path, &buf).expect("writes file");

        let back = crate::read_pptx(&path).expect("a file we wrote must parse");
        assert_eq!(back.len(), 2);
        assert_eq!(
            back.outline(),
            vec![Some("Hello & welcome".into()), Some("Roadmap".into())]
        );

        let roles: Vec<Role> = back.slides[1].blocks.iter().map(|b| b.role).collect();
        assert!(roles.contains(&Role::Body), "{roles:?}");
        assert!(roles.contains(&Role::Table), "{roles:?}");

        let levels: Vec<u8> = back.slides[1]
            .blocks
            .iter()
            .find(|b| b.role == Role::Body)
            .and_then(|b| b.as_text())
            .map(|t| t.paragraphs.iter().map(|p| p.level).collect())
            .expect("body block");
        assert_eq!(levels, vec![0, 1]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_deck_builds_to_the_same_bytes() {
        let deck = two_slide_deck();
        let mut a: Vec<u8> = Vec::new();
        write_pptx(&deck, std::io::Cursor::new(&mut a)).expect("writes");
        let mut b: Vec<u8> = Vec::new();
        write_pptx(&deck, std::io::Cursor::new(&mut b)).expect("writes");
        assert_eq!(
            a, b,
            "generated decks must be diffable, which requires stable bytes"
        );
    }

    #[test]
    fn unreportable_blocks_are_reported_not_silenced() {
        let deck = Deck {
            slides: vec![Slide {
                index: 0,
                blocks: vec![
                    Block {
                        role: Role::Title,
                        content: BlockContent::Text(TextContent {
                            paragraphs: vec![Paragraph::new(0, "Numbers")],
                        }),
                    },
                    Block {
                        role: Role::Chart,
                        content: BlockContent::Chart { caption: None },
                    },
                    Block {
                        role: Role::Picture,
                        content: BlockContent::Picture { alt: None },
                    },
                ],
            }],
        };
        let mut buf: Vec<u8> = Vec::new();
        let report = write_pptx(&deck, std::io::Cursor::new(&mut buf)).expect("writes");
        assert_eq!(report.skipped_count(), 2, "{:?}", report.skipped);
        assert!(report.skipped.iter().any(|s| s.contains("chart")));
        assert!(report.skipped.iter().any(|s| s.contains("pictures")));
    }

    #[test]
    fn links_get_one_relationship_each() {
        let mut t = LinkTable::default();
        assert_eq!(t.intern("https://a.example"), "rId2");
        assert_eq!(t.intern("https://b.example"), "rId3");
        assert_eq!(t.intern("https://a.example"), "rId2");
        assert_eq!(t.links.len(), 2);
    }

    #[test]
    fn styling_is_written_back_as_run_properties() {
        let deck = Deck {
            slides: vec![Slide {
                index: 0,
                blocks: vec![Block {
                    role: Role::Title,
                    content: BlockContent::Text(TextContent {
                        paragraphs: vec![Paragraph::from_runs(
                            0,
                            vec![
                                Run {
                                    text: "bold".into(),
                                    bold: Some(true),
                                    size: Some(2400),
                                    color: Some("FF0000".into()),
                                    link: Some("https://example.com".into()),
                                    ..Default::default()
                                },
                                Run::new(" plain"),
                            ],
                        )],
                    }),
                }],
            }],
        };
        let (parts_map, _) = built_parts(&deck);
        let xml = &parts_map["ppt/slides/slide1.xml"];
        assert!(xml.contains("b=\"1\""), "{xml}");
        assert!(xml.contains("sz=\"2400\""), "{xml}");
        assert!(xml.contains("<a:srgbClr val=\"FF0000\"/>"), "{xml}");
        assert!(xml.contains("<a:hlinkClick r:id=\"rId2\"/>"), "{xml}");
        // The link has to actually resolve through the slide's relationships.
        assert!(
            parts_map["ppt/slides/_rels/slide1.xml.rels"]
                .contains("Target=\"https://example.com\" TargetMode=\"External\""),
            "{}",
            parts_map["ppt/slides/_rels/slide1.xml.rels"]
        );
    }

    /// A subtitle alongside body text is the honest-shaped version of this bug:
    /// the content layout has no `subTitle` placeholder, so the naive writer
    /// drops the caption. Loss must not be the default.
    #[test]
    fn text_without_a_placeholder_is_placed_loose_not_dropped() {
        let deck = Deck {
            slides: vec![Slide {
                index: 0,
                blocks: vec![
                    Block {
                        role: Role::Title,
                        content: BlockContent::Text(TextContent {
                            paragraphs: vec![Paragraph::new(0, "Numbers")],
                        }),
                    },
                    Block {
                        role: Role::Subtitle,
                        content: BlockContent::Text(TextContent {
                            paragraphs: vec![Paragraph::new(0, "Source: internal deck")],
                        }),
                    },
                    Block {
                        role: Role::Body,
                        content: BlockContent::Text(TextContent {
                            paragraphs: vec![Paragraph::new(0, "one bullet")],
                        }),
                    },
                ],
            }],
        };
        let (parts_map, report) = built_parts(&deck);

        assert_eq!(report.skipped_count(), 0, "{:?}", report.skipped);
        assert_eq!(report.relocated.len(), 1, "{:?}", report.relocated);
        assert!(
            report.relocated[0].contains("subtitle"),
            "{}",
            report.relocated[0]
        );

        // The words are in the package, inside a shape that claims no
        // placeholder — which is precisely what "relocated" means.
        let xml = &parts_map["ppt/slides/slide1.xml"];
        assert!(xml.contains("Source: internal deck"), "{xml}");
        let loose = xml
            .split("<p:sp>")
            .find(|fragment| fragment.contains("Source: internal deck"))
            .expect("the loose shape");
        assert!(
            !loose.contains("<p:ph"),
            "a loose shape must not re-bind to the layout: {loose}"
        );
        // And it sits below the body placeholder rather than over it.
        assert!(xml.contains("<p:ph type=\"body\""), "{xml}");
    }

    #[test]
    fn an_empty_deck_is_refused_rather_than_written_broken() {
        let deck = Deck::default();
        let mut buf: Vec<u8> = Vec::new();
        let err = write_pptx(&deck, std::io::Cursor::new(&mut buf)).unwrap_err();
        assert!(err.to_string().contains("no slides"), "{err}");
    }
}
