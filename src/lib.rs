//! deckr — a bidirectional PowerPoint engine.
//!
//! Every conversion in deckr passes through one semantic representation, the
//! [Deck IR](ir). Read a `.pptx` into it, emit Markdown (or JSON) out of it,
//! and later rebuild a `.pptx` from it. Keeping the middle representation
//! semantic rather than geometric is what makes round-tripping possible.
//!
//! ```no_run
//! use std::path::Path;
//! let deck = deckr::read_pptx(Path::new("deck.pptx"))?;
//! println!("{}", deckr::markdown::to_markdown(&deck));
//! # Ok::<(), deckr::Error>(())
//! ```

pub mod error;
pub mod ir;
pub mod markdown;
pub mod ooxml;

pub use error::{Error, Result};
pub use ir::{Block, BlockContent, Deck, Paragraph, Role, Slide, TextContent};
pub use ooxml::read_file;

use std::path::Path;

/// Read a `.pptx` into Deck IR.
pub fn read_pptx(path: &Path) -> Result<Deck> {
    read_file(path)
}

/// Serialise Deck IR to JSON — the interchange format between deckr stages.
pub fn to_json(deck: &Deck) -> Result<String> {
    serde_json::to_string_pretty(deck).map_err(|e| Error::Other(e.to_string()))
}

/// Deserialise Deck IR from JSON.
pub fn from_json(s: &str) -> Result<Deck> {
    serde_json::from_str(s).map_err(|e| Error::Other(e.to_string()))
}
