//! Deck IR — the semantic intermediate representation every conversion passes through.
//!
//! A `Deck` is a list of `Slide`s; each slide is a list of `Block`s. Every block
//! carries a `Role`, which is the key design decision in deckr: instead of
//! exposing raw shapes and letting callers bypass the master, we force content
//! to declare *what it is* (title, body, chart, table…) so that writing it back
//! can re-bind it to the correct placeholder and preserve the theme.

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

    /// Roles that are chrome rather than content — usually skipped on export.
    pub fn is_chrome(self) -> bool {
        matches!(self, Role::Footer | Role::DateTime | Role::SlideNumber)
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockContent {
    Text(TextContent),
    Picture {
        alt: Option<String>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    /// Phase 0 records that a chart exists; numeric extraction lands in 0.4.
    Chart {
        caption: Option<String>,
    },
    Diagram {
        caption: Option<String>,
    },
    Empty,
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
        self.paragraphs.iter().all(|p| p.text.trim().is_empty())
    }
}

/// One bullet / line. `level` is the DrawingML indent level (0 = top level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub level: u8,
    pub text: String,
}
