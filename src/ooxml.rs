//! Reading `.pptx` (OOXML / PresentationML) into [`Deck`].
//!
//! # Why read order matters
//!
//! A `.pptx` is a ZIP of XML parts. The naive approach — sort `ppt/slides/slideN.xml`
//! by N — is wrong: after reordering slides in PowerPoint, `slide7.xml` can be the
//! first slide shown. The authoritative order lives in `ppt/presentation.xml`
//! (`<p:sldIdLst>`), whose `r:id`s resolve through `ppt/_rels/presentation.xml.rels`.
//! We read that chain first and only fall back to filename scanning when the
//! presentation part is missing or malformed.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader as XmlReader;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesRef, BytesStart, BytesText, Event};
use quick_xml::name::QName;
use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::ir::{
    Block, BlockContent, ChartBlob, ChartPart, Deck, DiagramBlob, DiagramPart, Paragraph, Role,
    Run, Slide, TextContent,
};

type Pkg = ZipArchive<File>;

const PRESENTATION: &str = "ppt/presentation.xml";
const PRESENTATION_RELS: &str = "ppt/_rels/presentation.xml.rels";

// ---------------------------------------------------------------------------
// small helpers that insulate us from the XML crate's churn
// ---------------------------------------------------------------------------

/// Local name of a qualified name — `p:sp` -> `"sp"`.
fn local_name(name: QName<'_>) -> String {
    let full: &str = name.0;
    match full.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => full.to_string(),
    }
}

/// Resolve the five entities XML actually permits, `&amp;` last so we don't
/// double-decode. quick-xml hands us the raw source text, so this is required.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&amp;", "&")
}

fn text_of(e: &BytesText<'_>) -> String {
    e.as_ref().to_string()
}

/// Resolve an XML entity reference (`&amp;`, `&#8212;`) to the character it
/// stands for. The reader hands these to us as their own event rather than
/// folding them into the surrounding text, so ignoring them silently loses
/// every ampersand in a deck.
fn ref_of(e: &BytesRef<'_>) -> String {
    let name: &str = e.as_ref();
    if e.is_char_ref() {
        return match e.resolve_char_ref() {
            Ok(Some(c)) => c.to_string(),
            _ => String::new(),
        };
    }
    match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
    .to_string()
}

/// `ppt/slides/slide2.xml` -> `ppt/slides/_rels/slide2.xml.rels`.
fn rels_path_for(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/_rels/{name}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// DrawingML spells booleans several ways across revisions: `b="1"`,
/// `b="on"`, `b="true"`. Anything else is not a value we understand, so we
/// leave it as *inherit* rather than guessing.
fn boolish(v: &str) -> Option<bool> {
    match v.trim() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

/// `<a:rPr strike="sngStrike">` rather than a boolean.
fn strike(v: &str) -> Option<bool> {
    match v.trim() {
        "sngStrike" | "dblStrike" => Some(true),
        "noStrike" | "none" => Some(false),
        other => boolish(other),
    }
}

/// `<a:rPr u="sng">` — any named underline style means underlined; only
/// `"none"` means not.
fn underline(v: &str) -> Option<bool> {
    match v.trim() {
        "" => None,
        "none" => Some(false),
        _ => Some(true),
    }
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        if local_name(a.key) == key {
            return Some(unescape(&a.value));
        }
    }
    None
}

fn attr_of(a: &Attribute<'_>) -> Option<(String, String)> {
    Some((local_name(a.key), unescape(&a.value)))
}

/// `<p:sldId id="257" r:id="rId2"/>` carries two attributes called `id`: the
/// unqualified one is the slide's own numeric id, and the namespaced one points
/// at the relationship that resolves to the actual part. Taking the first match
/// picks the wrong one on every real file, silently degrading us to filename
/// order — which is exactly the order we set out not to trust.
fn rel_id(e: &BytesStart<'_>) -> Option<String> {
    let mut unqualified = None;
    for a in e.attributes().flatten() {
        if local_name(a.key) != "id" {
            continue;
        }
        if a.key.0.contains(':') {
            return Some(unescape(&a.value));
        }
        unqualified = Some(unescape(&a.value));
    }
    unqualified
}

/// Join a relationship target onto `base_dir` and normalise `.` / `..`.
fn resolve_part(base_dir: &str, target: &str) -> String {
    let raw = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("{base_dir}{target}")
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(seg),
        }
    }
    out.join("/")
}

// ---------------------------------------------------------------------------
// package traversal
// ---------------------------------------------------------------------------

/// Read a whole slide deck from a `.pptx` file.
pub fn read_file(path: &Path) -> Result<Deck> {
    let file = File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut pkg = ZipArchive::new(file)?;

    let parts = slide_part_order(&mut pkg)?;
    if parts.is_empty() {
        return Err(Error::MissingPart("ppt/slides/slide1.xml".to_string()));
    }

    // The package content types tell us, for every captured chart part, what
    // content type to register on the way out — charts are preserved verbatim.
    let (ct_overrides, ct_defaults) = read_part(&mut pkg, "[Content_Types].xml")
        .map(|x| parse_content_types(&x))
        .unwrap_or_default();

    let mut deck = Deck::default();
    for (index, part) in parts.iter().enumerate() {
        let xml = read_part(&mut pkg, part).ok_or_else(|| Error::MissingPart(part.clone()))?;
        // Hyperlinks are stored as relationship ids, so the slide's own rels
        // part has to be available while its runs are being read.
        let hrefs = read_part(&mut pkg, &rels_path_for(part))
            .map(|x| parse_external_relationships(&x))
            .unwrap_or_default();
        // The *full* relationship set (internal too) resolves picture media and
        // chart parts to their files.
        let full = read_part(&mut pkg, &rels_path_for(part))
            .map(|x| parse_relationships(&x))
            .unwrap_or_default();

        let mut slide = parse_slide(&xml, index, part, &hrefs)?;
        resolve_media(
            &mut slide,
            part,
            &mut pkg,
            &full,
            &ct_overrides,
            &ct_defaults,
        );
        deck.slides.push(slide);
    }
    Ok(deck)
}

/// Turn the media and chart relationships a slide declares into real bytes.
///
/// The reader produces `Picture { embed }` and `Chart { rid, uri }` blocks with
/// no content; this walks the package to fill them in. A block whose
/// relationship is missing or dangling simply stays empty and is reported as
/// unplaceable later, rather than panicking here.
fn resolve_media(
    slide: &mut Slide,
    part: &str,
    pkg: &mut Pkg,
    rels: &HashMap<String, String>,
    ct_overrides: &HashMap<String, String>,
    ct_defaults: &HashMap<String, String>,
) {
    let dir = part
        .rsplit_once('/')
        .map(|(d, _)| format!("{d}/"))
        .unwrap_or_default();

    for block in &mut slide.blocks {
        let replacement = match &block.content {
            BlockContent::Picture {
                embed: Some(rid),
                alt,
                ..
            } => {
                let path = rels.get(rid).map(|t| resolve_part(&dir, t));
                let bytes = path.as_ref().and_then(|p| read_part_bytes(pkg, p));
                match (path, bytes) {
                    (Some(p), Some(b)) => Some(BlockContent::Picture {
                        alt: alt.clone(),
                        data: Some(b),
                        mime: Some(mime_for(&p)),
                        embed: None,
                    }),
                    _ => None,
                }
            }
            BlockContent::Chart {
                rid: Some(rid),
                uri: Some(uri),
                ..
            } => {
                let chart_path = rels.get(rid).map(|t| resolve_part(&dir, t));
                match chart_path {
                    Some(path) => capture_chart_subgraph(pkg, &path, ct_overrides, ct_defaults)
                        .map(|parts| {
                            let chart_xml = format!(
                                "<c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"{rid}\"/>"
                            );
                            let blob = ChartBlob {
                                uri: uri.clone(),
                                chart_xml,
                                parts,
                            };
                            // Numbers are a bonus view over the verbatim blob:
                            // decode what the chart part yields, and let the
                            // chart's own title serve as the caption when the
                            // slide did not provide one.
                            let data = crate::chart::decode_blob(&blob);
                            let caption = data.as_ref().and_then(|d| d.title.clone());
                            BlockContent::Chart {
                                caption,
                                blob: Some(blob),
                                data,
                                // Keep the original r:id so the writer can
                                // remap it onto a fresh relationship.
                                rid: Some(rid.clone()),
                                uri: Some(uri.clone()),
                            }
                        }),
                    None => None,
                }
            }
            BlockContent::Diagram { rel_ids, .. } if rel_ids.len() == 4 => {
                // The four relationships — data model, layout, quick style,
                // colours — are the diagram's entry points; everything else
                // (the pre-rendered drawing, its rels) hangs off the data
                // part's own relationships.
                let starts: Vec<Option<String>> = rel_ids
                    .iter()
                    .map(|rid| rels.get(rid).map(|t| resolve_part(&dir, t)))
                    .collect();
                if starts.iter().all(|p| p.is_some()) {
                    let starts: Vec<String> = starts.into_iter().flatten().collect();
                    capture_diagram_subgraph(pkg, &starts, ct_overrides, ct_defaults).map(|parts| {
                        // The data model yields the diagram's text points,
                        // in document order.
                        let texts = parts
                            .iter()
                            .find(|p| p.content_type.contains("diagramData"))
                            .and_then(|p| std::str::from_utf8(&p.bytes).ok())
                            .map(decode_diagram_texts)
                            .unwrap_or_default();
                        BlockContent::Diagram {
                            caption: None,
                            texts,
                            blob: Some(DiagramBlob { parts }),
                            rel_ids: rel_ids.clone(),
                        }
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(content) = replacement {
            block.content = content;
        }
    }
}

/// Read a part as raw bytes — media images and chart embeddings are not text.
fn read_part_bytes(pkg: &mut Pkg, name: &str) -> Option<Vec<u8>> {
    let mut entry = pkg.by_name(name).ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// BFS every part a chart's relationship graph reaches and collect them
/// verbatim, so the writer can re-emit the chart without understanding it.
///
/// The walk stops at an embedded workbook (its internals are opaque to a
/// `.pptx` reader and are copied as one blob) but continues through the chart's
/// own rels, theme and style parts.
fn capture_chart_subgraph(
    pkg: &mut Pkg,
    start: &str,
    overrides: &HashMap<String, String>,
    defaults: &HashMap<String, String>,
) -> Option<Vec<ChartPart>> {
    let mut out: Vec<ChartPart> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: Vec<String> = vec![start.to_string()];

    while let Some(part_path) = queue.pop() {
        if !seen.insert(part_path.clone()) {
            continue;
        }
        let bytes = read_part_bytes(pkg, &part_path)?;
        let content_type = content_type_for(&part_path, overrides, defaults);
        out.push(ChartPart {
            path: part_path.clone(),
            bytes,
            content_type,
        });

        // Only relationship-bearing XML parts continue the walk; a workbook is
        // a ZIP and must not be recursed into. A part with no `.rels` partner
        // (a chart that carries no embedded workbook, for instance) simply has
        // no further parts to pull in — that is normal, not an error, so we
        // move on to the next queued part rather than abandoning the whole
        // subgraph.
        if !part_path.ends_with(".rels") && !part_path.ends_with(".xlsx") {
            let rels_path = rels_path_for(&part_path);
            let Some(rels_xml) = read_part_bytes(pkg, &rels_path) else {
                continue;
            };
            // The relationship part is itself a part that must be re-emitted:
            // without it the rebuilt chart would reference an `r:id` (e.g. the
            // embedded workbook) through a `.rels` file that no longer exists,
            // which is exactly the dangling reference a consumer rejects.
            if seen.insert(rels_path.clone()) {
                out.push(ChartPart {
                    path: rels_path,
                    bytes: rels_xml.clone(),
                    content_type: "application/vnd.openxmlformats-package.relationships+xml"
                        .to_string(),
                });
            }
            let rels_text = String::from_utf8_lossy(&rels_xml);
            let base_dir = part_path
                .rsplit_once('/')
                .map(|(d, _)| format!("{d}/"))
                .unwrap_or_default();
            for (_, target) in relationships_of(&rels_text) {
                queue.push(resolve_part(&base_dir, &target));
            }
        }
    }

    Some(out)
}

/// Capture the diagram subgraph: BFS from each of the four entry parts (data
/// model, layout, quick style, colours), de-duplicated by part name. The data
/// part's relationships pull in the pre-rendered drawing, the same walk
/// [`capture_chart_subgraph`] makes for an embedded workbook.
fn capture_diagram_subgraph(
    pkg: &mut Pkg,
    starts: &[String],
    overrides: &HashMap<String, String>,
    defaults: &HashMap<String, String>,
) -> Option<Vec<DiagramPart>> {
    let mut out: Vec<DiagramPart> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for start in starts {
        let chart_parts = capture_chart_subgraph(pkg, start, overrides, defaults)?;
        for part in chart_parts {
            if seen.insert(part.path.clone()) {
                out.push(DiagramPart {
                    path: part.path,
                    bytes: part.bytes,
                    content_type: part.content_type,
                });
            }
        }
    }
    Some(out)
}

/// The text points of a diagram data model, in document order.
///
/// A `dgm:pt` carries its text inside `dgm:t`, whose paragraphs are ordinary
/// DrawingML (`a:p > a:r > a:t`). Runs within one point join; points stay
/// separate entries. Empty points (the `doc` root, layout stubs) yield nothing.
pub(crate) fn decode_diagram_texts(xml: &str) -> Vec<String> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = Vec::new();

    let mut pt_depth = 0usize;
    // Depth of the `t` element currently being captured (0 = not capturing).
    let mut t_depth = 0usize;
    let mut text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "pt" => pt_depth += 1,
                    "t" if pt_depth > 0 => {
                        t_depth += 1;
                        if t_depth == 1 {
                            text.clear();
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if t_depth > 0 {
                    text.push_str(&unescape(e.as_ref()));
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "pt" => {
                        pt_depth -= 1;
                        if t_depth > 0 {
                            // A point whose `dgm:t` was still open (no text).
                            t_depth = 0;
                        }
                    }
                    "t" if t_depth > 0 => {
                        t_depth -= 1;
                        if t_depth == 0 {
                            let t = text.trim().to_string();
                            if !t.is_empty() {
                                out.push(t);
                            }
                            text.clear();
                        }
                    }
                    _ => {}
                }
            }
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// All (id, target) pairs in a relationships part, internal and external.
fn relationships_of(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut id = None;
                let mut target = None;
                for a in e.attributes().flatten() {
                    if let Some((k, v)) = attr_of(&a) {
                        match k.as_str() {
                            "Id" => id = Some(v),
                            "Target" => target = Some(v),
                            _ => {}
                        }
                    }
                }
                if let (Some(i), Some(t)) = (id, target) {
                    out.push((i, t));
                }
            }
            Ok(_) => {}
        }
    }
    out
}

/// Parse `[Content_Types].xml` into Override part-name -> content type and
/// Default extension -> content type.
fn parse_content_types(xml: &str) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut overrides = HashMap::new();
    let mut defaults = HashMap::new();
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local = local_name(e.name());
                if local == "Override" {
                    let part = attr(&e, "PartName");
                    let ct = attr(&e, "ContentType");
                    if let (Some(p), Some(c)) = (part, ct) {
                        overrides.insert(p.trim_start_matches('/').to_string(), c);
                    }
                } else if local == "Default" {
                    let ext = attr(&e, "Extension");
                    let ct = attr(&e, "ContentType");
                    if let (Some(e), Some(c)) = (ext, ct) {
                        defaults.insert(e.to_lowercase(), c);
                    }
                }
            }
            Ok(_) => {}
        }
    }
    (overrides, defaults)
}

/// Resolve a content type for a captured part.
fn content_type_for(
    path: &str,
    overrides: &HashMap<String, String>,
    defaults: &HashMap<String, String>,
) -> String {
    if path.ends_with(".rels") {
        return "application/vnd.openxmlformats-package.relationships+xml".to_string();
    }
    if let Some(ct) = overrides.get(path.trim_start_matches('/')) {
        return ct.clone();
    }
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    defaults
        .get(&ext)
        .cloned()
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// Derive a media part's content type from its file extension.
fn mime_for(path: &str) -> String {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn read_part(pkg: &mut Pkg, name: &str) -> Option<String> {
    let mut entry = pkg.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

/// Slide parts in *presentation* order (not filename order).
fn slide_part_order(pkg: &mut Pkg) -> Result<Vec<String>> {
    let rels_xml = read_part(pkg, PRESENTATION_RELS).unwrap_or_default();
    let rels = parse_relationships(&rels_xml);

    let mut order: Vec<String> = Vec::new();
    if let Some(pres_xml) = read_part(pkg, PRESENTATION) {
        let mut reader = XmlReader::from_str(&pres_xml);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if local_name(e.name()) == "sldId" {
                        if let Some(id) = rel_id(&e) {
                            if let Some(t) = rels.get(&id) {
                                let part = resolve_part("ppt/", t);
                                if !order.contains(&part) {
                                    order.push(part);
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(source) => {
                    return Err(Error::Xml {
                        part: PRESENTATION.to_string(),
                        source,
                    });
                }
            }
        }
    }

    if order.is_empty() {
        order = scan_slide_parts(pkg);
    }
    Ok(order)
}

fn parse_relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut id = None;
                let mut target = None;
                for a in e.attributes().flatten() {
                    if let Some((k, v)) = attr_of(&a) {
                        match k.as_str() {
                            "Id" => id = Some(v),
                            "Target" => target = Some(v),
                            _ => {}
                        }
                    }
                }
                if let (Some(i), Some(t)) = (id, target) {
                    map.insert(i, t);
                }
            }
            Ok(_) => {}
        }
    }
    map
}

/// Only the relationships that point *outside* the package — hyperlink targets.
///
/// `TargetMode` defaults to `Internal`, where the target is another part name
/// rather than a URL, so mixing the two would put part paths in links.
fn parse_external_relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut id = None;
                let mut target = None;
                let mut external = false;
                for a in e.attributes().flatten() {
                    if let Some((k, v)) = attr_of(&a) {
                        match k.as_str() {
                            "Id" => id = Some(v),
                            "Target" => target = Some(v),
                            "TargetMode" => external = v.eq_ignore_ascii_case("external"),
                            _ => {}
                        }
                    }
                }
                if external {
                    if let (Some(i), Some(t)) = (id, target) {
                        map.insert(i, t);
                    }
                }
            }
            Ok(_) => {}
        }
    }
    map
}

/// Fallback when `presentation.xml` is unreadable: numeric filename order.
fn scan_slide_parts(pkg: &mut Pkg) -> Vec<String> {
    let mut found: Vec<(u32, String)> = Vec::new();
    for name in pkg.file_names() {
        if let Some(rest) = name.strip_prefix("ppt/slides/") {
            if let Some(num) = rest
                .strip_prefix("slide")
                .and_then(|r| r.strip_suffix(".xml"))
                .and_then(|n| n.parse::<u32>().ok())
            {
                found.push((num, name.to_string()));
            }
        }
    }
    found.sort_by_key(|(n, _)| *n);
    found.into_iter().map(|(_, s)| s).collect()
}

// ---------------------------------------------------------------------------
// slide parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapeKind {
    /// `p:sp` — a text box (possibly a placeholder).
    Shape,
    /// `p:pic` — an embedded or linked picture.
    Picture,
    /// `p:graphicFrame` — a table, chart, diagram or OLE object.
    GraphicFrame,
}

/// Accumulates everything we learn while streaming over one shape subtree.
///
/// Using a builder rather than a tree keeps the parser single-pass and O(1) in
/// memory per shape, which matters once you point it at a 400-slide deck.
#[derive(Debug)]
struct ShapeBuilder<'a> {
    kind: ShapeKind,
    /// Role declared by `p:ph/@type`, if any.
    ph_role: Option<Role>,
    /// True once a `p:ph` element is seen — an untyped placeholder means `body`.
    has_ph: bool,
    alt: Option<String>,

    txbody_depth: usize,
    para_open: bool,
    level: u8,
    buf: String,
    /// True while inside an `<a:t>` run. Everything else — indentation between
    /// elements, `&nbsp;`-free whitespace — is layout noise we must not absorb.
    in_run: bool,
    paragraphs: Vec<Paragraph>,

    /// The text run being assembled right now, if any.
    pending: Option<Run>,
    /// Runs accumulated for the paragraph currently open.
    runs: Vec<Run>,
    /// Inside `<a:rPr><a:solidFill>`, so `<a:srgbClr val="…"/>` is a colour.
    in_solid_fill: bool,
    /// Hyperlinks arrive as relationship ids; this resolves them to URLs.
    rels: &'a HashMap<String, String>,

    table: bool,
    rows: Vec<Vec<String>>,
    row: Option<Vec<String>>,
    cell: Option<String>,

    chart: bool,
    diagram: bool,
    /// `p:blip/@r:embed` for a picture — the relationship id whose target is
    /// the media file. Resolved to bytes by `read_file`.
    pic_embed: Option<String>,
    /// `c:chart/@r:id` and its containing graphicData's `uri`.
    chart_rid: Option<String>,
    chart_uri: Option<String>,
    /// `dgm:relIds/@r:dm,r:lo,r:qs,r:cs` — the four relationships a diagram
    /// frame references (data model, layout, quick style, colours), in that
    /// order.
    diagram_rels: Vec<String>,
}

impl<'a> ShapeBuilder<'a> {
    fn new(kind: ShapeKind, rels: &'a HashMap<String, String>) -> Self {
        Self {
            kind,
            ph_role: None,
            has_ph: false,
            alt: None,
            txbody_depth: 0,
            para_open: false,
            level: 0,
            buf: String::new(),
            in_run: false,
            paragraphs: Vec::new(),
            pending: None,
            runs: Vec::new(),
            in_solid_fill: false,
            rels,
            table: false,
            rows: Vec::new(),
            row: None,
            cell: None,
            chart: false,
            diagram: false,
            pic_embed: None,
            chart_rid: None,
            chart_uri: None,
            diagram_rels: Vec::new(),
        }
    }

    fn on_start(&mut self, local: &str, e: &BytesStart<'_>) {
        match local {
            "ph" => {
                self.has_ph = true;
                self.ph_role = attr(e, "type").and_then(|t| Role::from_ph_type(&t));
            }
            "cNvPr" => {
                if self.alt.is_none() {
                    self.alt = attr(e, "descr");
                }
            }
            "graphicData" => {
                if let Some(uri) = attr(e, "uri") {
                    if uri.contains("drawingml/2006/diagram") {
                        self.diagram = true;
                    } else if uri.contains("drawingml/2006/chart") {
                        // A chart graphic frame: remember the URI so the writer
                        // can rebuild the frame, and flag it as a chart.
                        self.chart = true;
                        self.chart_uri = Some(uri);
                    }
                }
            }
            "relIds" if self.diagram => {
                // <dgm:relIds r:dm=".." r:lo=".." r:qs=".." r:cs=".."/> — the
                // four relationships that make up the diagram. `attr` matches
                // local names, so the `r:` prefixes are already gone.
                for key in ["dm", "lo", "qs", "cs"] {
                    if let Some(rid) = attr(e, key) {
                        self.diagram_rels.push(rid);
                    }
                }
            }
            "blip" => {
                // Only pictures carry a blip; the referenced media lives in
                // `ppt/media/` and is fetched by `read_file`.
                if let Some(embed) = attr(e, "embed") {
                    self.pic_embed = Some(embed);
                }
            }
            "chart" => {
                self.chart = true;
                // The relationship reference is `r:id`; `attr` matches on the
                // local name, so the namespace prefix is already stripped and
                // we ask for `id`. (Matching `r:id` literally never hits, which
                // is exactly how a chart silently lost its r:id before.)
                if let Some(rid) = attr(e, "id").or_else(|| attr(e, "Id")) {
                    self.chart_rid = Some(rid);
                }
            }
            "tbl" => self.table = true,
            "tr" if self.table => self.row = Some(Vec::new()),
            "tc" if self.table => self.cell = Some(String::new()),
            "txBody" => self.txbody_depth += 1,
            "p" if self.txbody_depth > 0 && !self.table => {
                self.para_open = true;
                self.level = 0;
                self.buf.clear();
            }
            "pPr" => {
                if let Some(lvl) = attr(e, "lvl") {
                    self.level = lvl.trim().parse::<u8>().unwrap_or(0);
                }
            }
            // `<a:r>` and `<a:fld>` both begin a text run; the difference is
            // merely that a field's characters are generated rather than typed.
            "r" | "fld" if self.txbody_depth > 0 => {
                self.flush_run();
                self.pending = Some(Run::default());
            }
            "rPr" => {
                if let Some(r) = self.pending.as_mut() {
                    for a in e.attributes().flatten() {
                        let Some((k, v)) = attr_of(&a) else { continue };
                        match k.as_str() {
                            "b" => r.bold = boolish(&v),
                            "i" => r.italic = boolish(&v),
                            "u" => r.underline = underline(&v),
                            "strike" => r.strike = strike(&v),
                            // DrawingML measures in hundredths of a point.
                            "sz" => r.size = v.trim().parse::<u32>().ok(),
                            _ => {}
                        }
                    }
                }
            }
            // Colour is nested rather than an attribute: rPr -> solidFill ->
            // srgbClr. Theme references (`schemeClr`) deliberately do not become
            // colours — they are indirection, and resolving them is the job of
            // whoever owns the theme.
            "solidFill" => self.in_solid_fill = true,
            "srgbClr" if self.in_solid_fill => {
                if let (Some(r), Some(v)) = (self.pending.as_mut(), attr(e, "val")) {
                    r.color = Some(v.to_uppercase());
                }
            }
            "hlinkClick" => {
                if let Some(r) = self.pending.as_mut() {
                    if let Some(id) = attr(e, "id").or_else(|| attr(e, "Id")) {
                        r.link = self.rels.get(&id).cloned();
                    }
                }
            }
            "br" if self.txbody_depth > 0 => self.push_text("\n"),
            // `a:t` is the only element whose character data is text.
            "t" => self.in_run = true,
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        // Whitespace between child elements is not content; only `<a:t>` is.
        if !self.in_run {
            return;
        }
        if let Some(cell) = self.cell.as_mut() {
            cell.push_str(text);
        } else if self.para_open && self.txbody_depth > 0 {
            match self.pending.as_mut() {
                Some(r) => r.text.push_str(text),
                None => self.buf.push_str(text),
            }
        }
    }

    fn on_text(&mut self, text: &str) {
        self.push_text(text);
    }

    /// Hand the run being assembled to the paragraph now open.
    fn flush_run(&mut self) {
        if let Some(r) = self.pending.take() {
            // A run that carries properties but no characters is invisible; it
            // would otherwise show up as a spurious empty span on export.
            if !r.text.is_empty() {
                self.runs.push(r);
            }
        }
    }

    fn on_end(&mut self, local: &str) {
        match local {
            "txBody" => {
                if self.txbody_depth > 0 {
                    self.txbody_depth -= 1;
                }
            }
            "solidFill" | "rPr" => self.in_solid_fill = false,
            "r" | "fld" => {
                if self.txbody_depth > 0 {
                    self.flush_run();
                }
            }
            "p" if self.para_open => {
                self.flush_run();
                let mut runs = std::mem::take(&mut self.runs);
                let text = if runs.is_empty() {
                    collapse_spaces(&self.buf)
                } else {
                    // Collapse each run's own whitespace so element
                    // indentation vanishes, then trim the assembly.
                    for r in runs.iter_mut() {
                        r.text = collapse_spaces(&r.text);
                    }
                    runs.iter().map(|r| r.text.as_str()).collect::<String>()
                };
                self.paragraphs.push(Paragraph {
                    level: self.level,
                    text: text.trim().to_string(),
                    runs,
                });
                self.para_open = false;
                self.buf.clear();
            }
            "tc" => {
                if let (Some(cell), Some(row)) = (self.cell.take(), self.row.as_mut()) {
                    row.push(cell.trim().to_string());
                }
            }
            "tr" => {
                if let Some(row) = self.row.take() {
                    self.rows.push(row);
                }
            }
            "t" => self.in_run = false,
            _ => {}
        }
    }

    fn finish(self) -> Option<Block> {
        let content = if self.table {
            BlockContent::Table { rows: self.rows }
        } else if self.chart {
            BlockContent::Chart {
                caption: None,
                blob: None,
                data: None,
                rid: self.chart_rid,
                uri: self.chart_uri,
            }
        } else if self.diagram {
            BlockContent::Diagram {
                caption: None,
                texts: Vec::new(),
                blob: None,
                rel_ids: self.diagram_rels,
            }
        } else if self.kind == ShapeKind::Picture {
            BlockContent::Picture {
                alt: self.alt,
                data: None,
                mime: None,
                embed: self.pic_embed,
            }
        } else if !self.paragraphs.is_empty() {
            BlockContent::Text(TextContent {
                paragraphs: self.paragraphs,
            })
        } else {
            BlockContent::Empty
        };

        // A `<p:ph/>` with no `type` defaults to `body` per ECMA-376 §19.3.1.25.
        let role = match self.ph_role {
            Some(r) => r,
            None if self.has_ph => Role::Body,
            None => match &content {
                BlockContent::Table { .. } => Role::Table,
                BlockContent::Chart { .. } => Role::Chart,
                BlockContent::Diagram { .. } => Role::Diagram,
                BlockContent::Picture { .. } => Role::Picture,
                _ => Role::Freeform,
            },
        };

        Some(Block { role, content })
    }
}

/// Runs of whitespace inside a line collapse to one space; newlines from
/// `<a:br/>` are kept so a soft break survives the round trip.
///
/// Leading whitespace is deliberately preserved: a second run often opens with
/// the space that separates it from the first (`"Hello "` + `" world"`), and
/// collapsing that away silently glues words together. Callers trim the
/// finished paragraph instead.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.chars() {
        match ch {
            '\n' => {
                out.push('\n');
                pending_space = false;
            }
            c if c.is_whitespace() => pending_space = true,
            c => {
                if pending_space && !out.ends_with('\n') {
                    out.push(' ');
                }
                pending_space = false;
                out.push(c);
            }
        }
    }
    out
}

/// Parse one `ppt/slides/slideN.xml` part into a [`Slide`].
///
/// `rels` maps relationship ids to external targets, which is how hyperlinks are
/// stored; without it a link is only ever the opaque string `rId2`.
pub fn parse_slide(
    xml: &str,
    index: usize,
    part: &str,
    rels: &HashMap<String, String>,
) -> Result<Slide> {
    // Deliberately NOT trimming whitespace globally: quick-xml emits an entity
    // reference as its own event and trims the fragments either side of it, so
    // "Hello &amp; welcome" would otherwise come back as "Hellowelcome". We gate
    // capture on `<a:t>` and collapse runs of whitespace ourselves.
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut blocks: Vec<Block> = Vec::new();
    // Nesting depth inside the shape we are currently collecting, so that a
    // nested group still terminates on the shape's own closing tag.
    let mut depth: usize = 0;
    let mut shape: Option<ShapeBuilder<'_>> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(e.name());
                if let Some(s) = shape.as_mut() {
                    depth += 1;
                    s.on_start(&local, &e);
                } else if let Some(kind) = shape_kind(&local) {
                    shape = Some(ShapeBuilder::new(kind, rels));
                    depth = 1;
                }
            }
            Ok(Event::Empty(e)) => {
                if let Some(s) = shape.as_mut() {
                    let local = local_name(e.name());
                    // Self-closing elements can never contain text, so treating
                    // them as "start" is enough (e.g. `<p:ph/>`, `<a:br/>`).
                    s.on_start(&local, &e);
                }
            }
            Ok(Event::End(e)) => {
                if shape.is_some() {
                    let local = local_name(e.name());
                    if let Some(s) = shape.as_mut() {
                        s.on_end(&local);
                    }
                    depth -= 1;
                    if depth == 0 {
                        if let Some(block) = shape.take().and_then(|s| s.finish()) {
                            blocks.push(block);
                        }
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(s) = shape.as_mut() {
                    s.on_text(&text_of(&e));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(s) = shape.as_mut() {
                    s.on_text(&ref_of(&e));
                }
            }
            Ok(_) => {}
            Err(source) => {
                return Err(Error::Xml {
                    part: part.to_string(),
                    source,
                });
            }
        }
    }

    // Keep every block, including empty placeholders: knowing that slide 7 has
    // an unused body placeholder is exactly the kind of thing a linter wants.
    Ok(Slide { index, blocks })
}

fn shape_kind(local: &str) -> Option<ShapeKind> {
    match local {
        "sp" => Some(ShapeKind::Shape),
        "pic" => Some(ShapeKind::Picture),
        "graphicFrame" => Some(ShapeKind::GraphicFrame),
        _ => None,
    }
}

/// Re-author a captured diagram data model from edited [`texts`]
/// (`crate::ir::Diagram.texts`).
///
/// Surgical, like the chart rewrite: everything outside a text point's
/// `dgm:t` subtree passes through byte-for-byte (connections, layout hints,
/// the drawing reference), and only the text subtrees of the points that
/// carry text are replaced. Text points are paired with `texts` by document
/// order — the same order [`decode_diagram_texts`] reads them back in. Points
/// beyond the provided texts keep their original words; texts beyond the
/// model's points are ignored (creating points would mean inventing model ids
/// and connections).
pub(crate) fn rewrite_diagram_data(xml: &str, texts: &[String]) -> String {
    #[inline]
    fn raw(bytes: &[u8]) -> &str {
        std::str::from_utf8(bytes).unwrap_or("")
    }
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::with_capacity(xml.len() + 64);

    let mut pt_depth = 0usize;
    let mut t_depth = 0usize;
    let mut text_pts = 0usize;
    // While suppressing a `dgm:t`, the index of the point it belongs to.
    let mut suppress: Option<usize> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "pt" => pt_depth += 1,
                    "t" if pt_depth > 0 => {
                        t_depth += 1;
                        if t_depth == 1 {
                            // The point's own dgm:t opened: this is a
                            // text-bearing point.
                            let idx = text_pts;
                            text_pts += 1;
                            if idx < texts.len() {
                                suppress = Some(idx);
                                out.push_str(&format!(
                                    "<dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></dgm:t>",
                                    crate::parts::esc(&texts[idx])
                                ));
                                continue;
                            }
                        }
                        // Nested a:t (or an unrewritten point): passthrough.
                    }
                    _ => {}
                }
                if suppress.is_none() {
                    out.push('<');
                    out.push_str(raw(e.as_ref().as_bytes()));
                    out.push('>');
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name());
                match local.as_str() {
                    "pt" => {
                        pt_depth -= 1;
                        if t_depth > 0 {
                            // The point closed with its dgm:t still open.
                            t_depth = 0;
                            suppress = None;
                        }
                    }
                    "t" if t_depth > 0 => {
                        t_depth -= 1;
                        if t_depth == 0 {
                            // The point's own dgm:t closed. When it was
                            // rewritten, the replacement above already carries
                            // the closing tag — swallow this End too.
                            let was_suppressed = suppress.take().is_some();
                            if !was_suppressed {
                                out.push_str("</");
                                out.push_str(raw(e.as_ref().as_bytes()));
                                out.push('>');
                            }
                            continue;
                        }
                    }
                    _ => {}
                }
                if suppress.is_none() {
                    out.push_str("</");
                    out.push_str(raw(e.as_ref().as_bytes()));
                    out.push('>');
                }
            }
            Ok(Event::Empty(e)) => {
                if suppress.is_none() {
                    out.push('<');
                    out.push_str(raw(e.as_ref().as_bytes()));
                    out.push_str("/>");
                }
            }
            Ok(Event::Text(e)) => {
                if suppress.is_none() {
                    out.push_str(raw(e.as_ref().as_bytes()));
                }
            }
            Ok(Event::Comment(e)) => {
                out.push_str("<!--");
                out.push_str(raw(e.as_ref().as_bytes()));
                out.push_str("-->");
            }
            Ok(Event::Decl(e)) => {
                out.push_str("<?");
                out.push_str(raw(e.as_ref().as_bytes()));
                out.push_str("?>");
            }
            Ok(Event::PI(e)) => {
                out.push_str("<?");
                out.push_str(raw(e.as_ref().as_bytes()));
                out.push_str("?>");
            }
            Err(_) => break,
            // CData, DocType, GeneralRef and the rest never appear in a
            // diagram data model; anything unexpected is dropped.
            _ => {}
        }
        buf.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = concat!(
        r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
        r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#,
        r#" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
        r#"<p:cSld><p:spTree>"#,
    );

    fn slide(body: &str) -> Slide {
        slide_with_rels(body, &HashMap::new())
    }

    fn slide_with_rels(body: &str, rels: &HashMap<String, String>) -> Slide {
        let xml = format!("{NS}{body}</p:spTree></p:cSld></p:sld>");
        parse_slide(&xml, 0, "ppt/slides/slide1.xml", rels).expect("slide parses")
    }

    const TITLE: &str = r#"
        <p:sp>
          <p:nvSpPr>
            <p:cNvPr id="2" name="Title 1"/>
            <p:cNvSpPr txBox="1"/>
            <p:nvPr><p:ph type="title"/></p:nvPr>
          </p:nvSpPr>
          <p:txBody>
            <a:p>
              <a:r><a:rPr lang="en-US"/><a:t>Hello &amp; welcome</a:t></a:r>
              <a:r><a:t> to deckr</a:t></a:r>
            </a:p>
          </p:txBody>
        </p:sp>"#;

    /// `<p:ph/>` without `type` means `body` per ECMA-376, not `freeform`.
    const UNTYPED_BODY: &str = r#"
        <p:sp>
          <p:nvSpPr><p:cNvPr id="3" name="Content 2"/><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr>
          <p:txBody>
            <a:p><a:pPr lvl="0"/><a:r><a:t>alpha</a:t></a:r></a:p>
            <a:p><a:pPr lvl="1"/><a:r><a:t>beta</a:t></a:r></a:p>
            <a:p><a:pPr lvl="1"/><a:r><a:t>gamma<br/>delta</a:t></a:r></a:p>
          </p:txBody>
        </p:sp>"#;

    const TABLE: &str = r#"
        <p:graphicFrame>
          <p:nvGraphicFramePr>
            <p:cNvPr id="5" name="Table 4"/>
            <p:nvPr/>
          </p:nvGraphicFramePr>
          <a:graphic>
            <a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">
              <a:tbl>
                <a:tr>
                  <a:tc><a:txBody><a:p><a:t>Region</a:t></a:p></a:txBody></a:tc>
                  <a:tc><a:txBody><a:p><a:t>Q3</a:t></a:p></a:txBody></a:tc>
                </a:tr>
                <a:tr>
                  <a:tc><a:txBody><a:p><a:t>APAC</a:t></a:p></a:txBody></a:tc>
                  <a:tc><a:txBody><a:p><a:t>1.2</a:t></a:p></a:txBody></a:tc>
                </a:tr>
              </a:tbl>
            </a:graphicData>
          </a:graphic>
        </p:graphicFrame>"#;

    const PICTURE: &str = r#"
        <p:pic>
          <p:nvPicPr>
            <p:cNvPr id="4" name="Picture 3" descr="revenue trend"/>
            <p:nvPr/>
          </p:nvPicPr>
          <p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>
        </p:pic>"#;

    const CHART: &str = r#"
        <p:graphicFrame>
          <p:nvGraphicFramePr><p:cNvPr id="6" name="Chart 5"/><p:nvPr/></p:nvGraphicFramePr>
          <a:graphic>
            <a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
              <c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId4"/>
            </a:graphicData>
          </a:graphic>
        </p:graphicFrame>"#;

    #[test]
    fn title_is_read_from_the_title_placeholder() {
        let s = slide(TITLE);
        assert_eq!(s.title().as_deref(), Some("Hello & welcome to deckr"));
        assert!(s.blocks[0].as_text().is_some());
        assert_eq!(s.blocks[0].role, Role::Title);
    }

    #[test]
    fn untyped_placeholder_defaults_to_body() {
        let s = slide(UNTYPED_BODY);
        let text = s.blocks[0].as_text().expect("body text");
        assert_eq!(s.blocks[0].role, Role::Body);
        let levels: Vec<u8> = text.paragraphs.iter().map(|p| p.level).collect();
        assert_eq!(levels, vec![0, 1, 1]);
        assert_eq!(text.paragraphs[0].text, "alpha");
        // A soft break stays a line feed rather than vanishing.
        assert_eq!(text.paragraphs[2].text, "gamma\ndelta");
    }

    #[test]
    fn untyped_ph_and_nested_groups_keep_their_own_shape() {
        let grouped = format!(
            "{NS}<p:grpSp><p:grpSpPr/><p:sp>{TITLE_INNER}</p:sp></p:grpSp></p:spTree></p:cSld></p:sld>",
            NS = NS,
            TITLE_INNER = "<p:nvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:t>Nested</a:t></a:p></p:txBody>"
        );
        let s = parse_slide(&grouped, 0, "ppt/slides/slide1.xml", &HashMap::new()).unwrap();
        assert_eq!(
            s.blocks.len(),
            1,
            "the group itself must not become a block"
        );
        assert_eq!(s.title().as_deref(), Some("Nested"));
    }

    #[test]
    fn tables_are_reconstructed_from_cells() {
        let s = slide(TABLE);
        match &s.blocks[0].content {
            BlockContent::Table { rows } => assert_eq!(
                rows,
                &vec![
                    vec!["Region".to_string(), "Q3".to_string()],
                    vec!["APAC".to_string(), "1.2".to_string()],
                ]
            ),
            other => panic!("expected a table, got {other:?}"),
        }
        assert_eq!(s.blocks[0].role, Role::Table);
    }

    #[test]
    fn pictures_keep_alt_text() {
        let s = slide(PICTURE);
        assert_eq!(s.blocks[0].role, Role::Picture);
        match &s.blocks[0].content {
            BlockContent::Picture { alt, .. } => {
                assert_eq!(alt.as_deref(), Some("revenue trend"))
            }
            other => panic!("expected a picture, got {other:?}"),
        }
    }

    #[test]
    fn charts_are_recorded_even_without_numbers() {
        let s = slide(CHART);
        assert!(matches!(s.blocks[0].content, BlockContent::Chart { .. }));
        assert_eq!(s.blocks[0].role, Role::Chart);
    }

    #[test]
    fn every_block_survives_one_slide() {
        let body = format!("{TITLE}{UNTYPED_BODY}{TABLE}{PICTURE}{CHART}");
        let s = slide(&body);
        assert_eq!(s.blocks.len(), 5);
        let roles: Vec<Role> = s.blocks.iter().map(|b| b.role).collect();
        assert_eq!(
            roles,
            vec![
                Role::Title,
                Role::Body,
                Role::Table,
                Role::Picture,
                Role::Chart
            ]
        );
    }

    #[test]
    fn rels_targets_resolve_against_the_ppt_directory() {
        assert_eq!(
            resolve_part("ppt/", "slides/slide3.xml"),
            "ppt/slides/slide3.xml"
        );
        assert_eq!(
            resolve_part("ppt/", "slides/../slides/slide3.xml"),
            "ppt/slides/slide3.xml"
        );
    }

    #[test]
    fn entities_are_decoded_once() {
        assert_eq!(unescape("a &amp;amp; b"), "a &amp; b");
        assert_eq!(unescape("&lt;p&gt;"), "<p>");
        assert_eq!(unescape("plain"), "plain");
    }

    #[test]
    fn relationship_id_wins_over_the_bare_slide_id() {
        let xml = concat!(
            r#"<p:sldIdLst xmlns:p="http://x" xmlns:r="http://y">"#,
            r#"<p:sldId id="257" r:id="rId2"/>"#,
            r#"<p:sldId r:id="rId1" id="256"/>"#,
            r#"</p:sldIdLst>"#
        );
        let mut reader = XmlReader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut got = Vec::new();
        loop {
            match reader.read_event() {
                Ok(Event::Eof) | Err(_) => break,
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if local_name(e.name()) == "sldId" {
                        got.push(rel_id(&e).unwrap_or_default());
                    }
                }
                Ok(_) => {}
            }
        }
        assert_eq!(got, vec!["rId2".to_string(), "rId1".to_string()]);
    }

    const STYLED: &str = r#"
        <p:sp>
          <p:nvSpPr><p:cNvPr id="3" name="Content 2"/><p:nvPr><p:ph/></p:nvPr></p:nvSpPr>
          <p:txBody>
            <a:p>
              <a:r><a:rPr lang="en-US" b="1" sz="2400" dirty="0"/><a:t>Bold headline</a:t></a:r>
              <a:r><a:rPr lang="en-US" i="1" strike="sngStrike" dirty="0"/><a:t>quiet note</a:t></a:r>
              <a:r>
                <a:rPr lang="en-US" u="sng" dirty="0">
                  <a:solidFill><a:srgbClr val="ff0000"/></a:solidFill>
                  <a:hlinkClick r:id="rId2"/>
                </a:rPr>
                <a:t>read the report</a:t>
              </a:r>
            </a:p>
          </p:txBody>
        </p:sp>"#;

    #[test]
    fn run_formatting_survives_the_read() {
        let rels = HashMap::from([("rId2".to_string(), "https://example.com/report".to_string())]);
        let s = slide_with_rels(STYLED, &rels);
        let text = s.blocks[0].as_text().expect("body holds text");
        let runs = &text.paragraphs[0].runs;

        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "Bold headline");
        assert_eq!(runs[0].bold, Some(true));
        assert_eq!(runs[0].size, Some(2400));
        // Absent is not "false" — it means inherit from the placeholder.
        assert_eq!(runs[0].underline, None);

        assert_eq!(runs[1].italic, Some(true));
        assert_eq!(runs[1].strike, Some(true));

        assert_eq!(runs[2].color.as_deref(), Some("FF0000"));
        assert_eq!(runs[2].underline, Some(true));
        assert_eq!(runs[2].link.as_deref(), Some("https://example.com/report"));

        // The flat text fast path still sees the whole paragraph.
        assert_eq!(
            text.paragraphs[0].text,
            "Bold headlinequiet noteread the report"
        );
        assert!(text.has_formatting());
    }

    #[test]
    fn an_unresolvable_hyperlink_is_dropped_not_guessed() {
        let s = slide(STYLED);
        let runs = &s.blocks[0].as_text().unwrap().paragraphs[0].runs;
        assert_eq!(runs[2].link, None, "no rels part means no URL");
        assert_eq!(runs[2].text, "read the report");
    }

    #[test]
    fn slide_rels_live_next_door() {
        assert_eq!(
            rels_path_for("ppt/slides/slide2.xml"),
            "ppt/slides/_rels/slide2.xml.rels"
        );
    }
}
