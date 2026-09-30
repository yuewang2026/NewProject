//! Deck IR — the semantic intermediate representation every conversion passes through.
//!
//! A `Deck` is a list of `Slide`s; each slide is a list of `Block`s. Every block
//! carries a `Role`, which is the key design decision in deckr: instead of
//! exposing raw shapes and letting callers bypass the master, we force content
//! to declare *what it is* (title, body, chart, table…) so that writing it back
//! can re-bind it to the correct placeholder and preserve the theme.
//!
//! Nothing here may mention PresentationML. This module is the contract, and it
//! has to stay comprehensible to a caller who has never seen a `.pptx`.

use serde::{Deserialize, Serialize};

/// A whole presentation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deck {
    pub slides: Vec<Slide>,
}

impl Deck {
    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slides.is_empty()
    }

    /// Every slide title, in order (missing titles become `None`).
    pub fn outline(&self) -> Vec<Option<String>> {
        self.slides.iter().map(|s| s.title()).collect()
    }

    /// Total number of paragraphs across the deck — the unit of a loss report.
    pub fn paragraph_count(&self) -> usize {
        self.slides
            .iter()
            .map(|s| s.blocks.iter().map(Block::paragraph_count).sum::<usize>())
            .sum()
    }
}

/// One slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    /// Zero-based slide index.
    pub index: usize,
    pub blocks: Vec<Block>,
}

impl Slide {
    /// The title text, if this slide declares a title placeholder.
    pub fn title(&self) -> Option<String> {
        self.blocks
            .iter()
            .find(|b| matches!(b.role, Role::Title | Role::CenteredTitle))
            .and_then(|b| match &b.content {
                BlockContent::Text(t) => Some(t.plain_inline()),
                _ => None,
            })
            .filter(|s| !s.trim().is_empty())
    }

    pub fn text_blocks(&self) -> impl Iterator<Item = &TextContent> {
        self.blocks.iter().filter_map(|b| match &b.content {
            BlockContent::Text(t) => Some(t),
            _ => None,
        })
    }
}

/// What a block *means* on the slide.
///
/// Mirrors `p:ph/@type` from PresentationML, plus `Freeform` for shapes that are
/// not bound to any placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Title,
    CenteredTitle,
    Subtitle,
    Body,
    Object,
    Picture,
    Table,
    Chart,
    Diagram,
    Footer,
    DateTime,
    SlideNumber,
    /// A shape with no placeholder binding — authored freely on the canvas.
    Freeform,
}

impl Role {
    /// Map a `p:ph type="…"` attribute value to a role.
    pub fn from_ph_type(t: &str) -> Option<Role> {
        Some(match t {
            "title" => Role::Title,
            "ctrTitle" => Role::CenteredTitle,
            "subTitle" => Role::Subtitle,
            "body" => Role::Body,
            "obj" => Role::Object,
            "pic" => Role::Picture,
            "tbl" => Role::Table,
            "chart" => Role::Chart,
            "dgm" => Role::Diagram,
            "ftr" => Role::Footer,
            "dt" => Role::DateTime,
            "sldNum" => Role::SlideNumber,
            _ => return None,
        })
    }

    /// The PresentationML placeholder type this role writes back to.
    ///
    /// `Freeform` has none: it is authored on the canvas and must carry its own
    /// geometry, so the writer emits it without a `<p:ph/>` binding.
    pub fn to_ph_type(self) -> Option<&'static str> {
        Some(match self {
            Role::Title => "title",
            Role::CenteredTitle => "ctrTitle",
            Role::Subtitle => "subTitle",
            Role::Body => "body",
            Role::Object => "obj",
            Role::Picture => "pic",
            Role::Table => "tbl",
            Role::Chart => "chart",
            Role::Diagram => "dgm",
            Role::Footer => "ftr",
            Role::DateTime => "dt",
            Role::SlideNumber => "sldNum",
            Role::Freeform => return None,
        })
    }

    /// Roles that are chrome rather than content — usually skipped on export.
    pub fn is_chrome(self) -> bool {
        matches!(self, Role::Footer | Role::DateTime | Role::SlideNumber)
    }

    /// Roles the Phase 0.2 writer can actually materialise as text.
    pub fn is_text_role(self) -> bool {
        !matches!(
            self,
            Role::Picture | Role::Chart | Role::Diagram | Role::Table
        )
    }

    /// Stable snake_case name, used by the JSON IR and CLI output.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Title => "title",
            Role::CenteredTitle => "centered_title",
            Role::Subtitle => "subtitle",
            Role::Body => "body",
            Role::Object => "object",
            Role::Picture => "picture",
            Role::Table => "table",
            Role::Chart => "chart",
            Role::Diagram => "diagram",
            Role::Footer => "footer",
            Role::DateTime => "datetime",
            Role::SlideNumber => "slide_number",
            Role::Freeform => "freeform",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A positioned piece of content on a slide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub role: Role,
    pub content: BlockContent,
}

impl Block {
    pub fn as_text(&self) -> Option<&TextContent> {
        match &self.content {
            BlockContent::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.content {
            BlockContent::Text(t) => t.is_empty(),
            BlockContent::Table { rows } => rows.is_empty(),
            BlockContent::Empty => true,
            _ => false,
        }
    }

    pub fn paragraph_count(&self) -> usize {
        match &self.content {
            BlockContent::Text(t) => t.paragraph_count(),
            BlockContent::Table { rows } => rows.len(),
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockContent {
    Text(TextContent),
    /// An embedded picture. `data` is the raw bytes; when present the writer
    /// drops the file into `ppt/media/` and binds a `<p:pic>`. `embed` is a
    /// read-side scratch field — the relationship id the source used — and is
    /// resolved to `data` before the deck is returned, so callers never see it.
    Picture {
        alt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "base64_opt")]
        data: Option<Vec<u8>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(skip)]
        embed: Option<String>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    /// A chart, preserved verbatim rather than understood. `blob` holds the
    /// original `<c:chart>` element and every part its relationship graph
    /// reaches (the chart XML, its rels, the embedded workbook, any theme it
    /// references). It is serialised through the JSON IR so charts survive a
    /// `pptx -> json -> pptx` round trip unchanged; `rid`/`uri` are read-side
    /// scratch and are not carried in JSON, because `blob` already owns `uri`
    /// and the writer re-derives the id from `blob.chart_xml`. The writer
    /// ignores a chart whose `blob` is `None`.
    Chart {
        caption: Option<String>,
        #[serde(default)]
        blob: Option<ChartBlob>,
        #[serde(skip)]
        rid: Option<String>,
        #[serde(skip)]
        uri: Option<String>,
    },
    Diagram {
        caption: Option<String>,
    },
    Empty,
}

/// Everything a chart needs to be written back exactly as it came in.
///
/// Charts are not parsed into a data model (that is numeric extraction, a
/// later phase); instead the whole subgraph is carried as opaque bytes and
/// re-emitted untouched. Deriving `Serialize`/`Deserialize` keeps charts
/// portable through the JSON IR so they survive a `pptx -> json -> pptx`
/// round trip rather than collapsing to `{"caption": null}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChartBlob {
    /// The `a:graphicData/@uri` — `…/drawingml/2006/chart` distinguishes it
    /// from a diagram or OLE object.
    pub uri: String,
    /// The `<c:chart …/>` element exactly as read, including its original
    /// `r:id`; the writer remaps that id to a fresh one before emitting.
    pub chart_xml: String,
    /// Every part reached by walking the chart's relationships: the chart XML,
    /// its `.rels`, the embedded workbook, and any theme/style it references.
    pub parts: Vec<ChartPart>,
}

/// One part of a chart's subgraph, verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPart {
    /// Part name within the package, e.g. `ppt/charts/chart1.xml`.
    pub path: String,
    /// The part's bytes, base64-encoded in JSON so a chart subgraph (an
    /// embedded workbook included) travels as one self-contained document.
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
    /// Its content type, copied from the source `[Content_Types].xml` so the
    /// writer can register an Override for it.
    pub content_type: String,
}

/// A run of paragraphs belonging to one placeholder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextContent {
    pub paragraphs: Vec<Paragraph>,
}

impl TextContent {
    /// Paragraphs joined by newlines, preserving bullet structure.
    pub fn plain(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// All text collapsed onto one line — used for titles.
    pub fn plain_inline(&self) -> String {
        self.paragraphs
            .iter()
            .flat_map(|p| p.text.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty() || self.paragraphs.iter().all(|p| p.text.trim().is_empty())
    }

    pub fn paragraph_count(&self) -> usize {
        self.paragraphs
            .iter()
            .filter(|p| !p.text.trim().is_empty())
            .count()
    }

    /// Any run anywhere in here carries explicit formatting?
    pub fn has_formatting(&self) -> bool {
        self.paragraphs.iter().any(|p| !p.is_plain())
    }
}

/// One bullet / line. `level` is the DrawingML indent level (0 = top level).
///
/// `text` is always present as a flat concatenation of `runs`, so callers that
/// only want characters never have to walk the runs. `runs` is what preserves
/// bold, italic, colour, size and hyperlinks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub level: u8,
    pub text: String,
    /// `Vec<Run>` when the source had multiple text runs; empty for synthetic
    /// paragraphs built by callers that do not care about formatting.
    #[serde(default)]
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// A paragraph of unformatted text — the common case, and what nearly every
    /// test builds.
    pub fn new(level: u8, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            level,
            runs: vec![Run::new(&text)],
            text,
        }
    }

    /// Build from runs, deriving `text` by concatenation.
    pub fn from_runs(level: u8, runs: Vec<Run>) -> Self {
        let text = runs.iter().map(|r| r.text.as_str()).collect::<String>();
        Self { level, text, runs }
    }

    /// No run carries explicit formatting, so `text` fully describes it.
    pub fn is_plain(&self) -> bool {
        self.runs.len() <= 1 && self.runs.iter().all(Run::is_plain)
    }

    /// The runs to render. Falls back to a synthetic run so callers need not
    /// special-case the empty case.
    pub fn display_runs(&self) -> Vec<Run> {
        if self.runs.is_empty() {
            vec![Run::new(&self.text)]
        } else {
            self.runs.clone()
        }
    }
}

/// A span of characters sharing one set of character properties.
///
/// Every field is `None` by default meaning *inherit* rather than *off* —
/// matching DrawingML, where the absence of `b` means "take it from the
/// placeholder or master", not "not bold".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    /// Font size in hundredths of a point (DrawingML's unit: `sz="1800"` = 18pt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    /// Six hex digits without a leading `#`, as DrawingML spells them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Target of a hyperlink, already resolved out of the relationship part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl Run {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn is_plain(&self) -> bool {
        self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.strike.is_none()
            && self.size.is_none()
            && self.color.is_none()
            && self.link.is_none()
    }
}

/// Transparent base64 (de)serialisation for `Option<Vec<u8>>`.
///
/// `None` stays `null`; `Some(bytes)` becomes a base64 string. This keeps
/// pictures portable through the JSON IR without a separate sidecar file.
mod base64_opt {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => {
                s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
            }
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let text = Option::<String>::deserialize(d)?;
        match text {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Transparent base64 (de)serialisation for `Vec<u8>` — no `Option` layer.
///
/// Used by `ChartPart::bytes`, where a part always has bytes. Serialising as a
/// base64 string keeps a chart's embedded workbook portable through the JSON
/// IR alongside the picture bytes handled by `base64_opt`.
mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_derived_from_runs() {
        let p = Paragraph::from_runs(
            0,
            vec![
                Run::new("plain "),
                Run {
                    text: "loud".into(),
                    bold: Some(true),
                    ..Default::default()
                },
            ],
        );
        assert_eq!(p.text, "plain loud");
        assert!(!p.is_plain());
    }

    #[test]
    fn a_single_unstyled_run_counts_as_plain() {
        let p = Paragraph::new(0, "just words");
        assert!(p.is_plain());
        assert_eq!(p.display_runs(), vec![Run::new("just words")]);
    }

    #[test]
    fn roles_round_trip_through_placeholder_types() {
        for role in [
            Role::Title,
            Role::CenteredTitle,
            Role::Subtitle,
            Role::Body,
            Role::Object,
            Role::Picture,
            Role::Table,
            Role::Chart,
            Role::Diagram,
            Role::Footer,
            Role::DateTime,
            Role::SlideNumber,
        ] {
            let t = role.to_ph_type().expect("every named role has a type");
            assert_eq!(Role::from_ph_type(t), Some(role), "{t}");
        }
        // Freeform has no placeholder binding by definition.
        assert_eq!(Role::Freeform.to_ph_type(), None);
    }

    #[test]
    fn old_json_without_runs_still_loads() {
        let json = r#"{"index":0,"blocks":[{"role":"title","content":{"text":{"paragraphs":[{"level":0,"text":"Hello"}]}}}]}"#;
        let slide: Slide = serde_json::from_str(json).expect("deserialises");
        assert_eq!(slide.title().as_deref(), Some("Hello"));
        assert!(slide.blocks[0].as_text().unwrap().paragraphs[0]
            .runs
            .is_empty());
        assert_eq!(
            slide.blocks[0].as_text().unwrap().paragraphs[0].display_runs(),
            vec![Run::new("Hello")]
        );
    }

    #[test]
    fn paragraph_count_skips_empties() {
        let tc = TextContent {
            paragraphs: vec![
                Paragraph::new(0, "a"),
                Paragraph::new(0, "   "),
                Paragraph::new(0, "b"),
            ],
        };
        assert_eq!(tc.paragraph_count(), 2);
    }
}
