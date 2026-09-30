//! deckr — a bidirectional PowerPoint engine.
//!
//! Every conversion in deckr passes through one semantic representation, the
//! [Deck IR](ir). Read a `.pptx` into it, emit Markdown (or JSON) out of it,
//! and build a `.pptx` back from it. Keeping the middle representation
//! semantic rather than geometric is what makes round-tripping possible — and
//! what makes it possible to count what did not survive the trip.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let deck = deckr::read_pptx(Path::new("deck.pptx"))?;
//! println!("{}", deckr::markdown::to_markdown(&deck));
//!
//! // The same IR goes back out. Nothing here says where anything sits: each
//! // block carries a `Role` and binds to the matching master placeholder.
//! let report = deckr::write_pptx_file(&deck, Path::new("rebuilt.pptx"))?;
//! println!("wrote {} block(s), lost {}", report.blocks_written, report.skipped_count());
//! # Ok::<(), deckr::Error>(())
//! ```

pub mod build;
pub mod error;
pub mod ir;
pub mod markdown;
pub mod md;
pub mod ooxml;
pub mod parts;
pub mod render;
pub mod template;

pub use build::{
    BuildReport, write_pptx, write_pptx_file, write_pptx_file_template, write_pptx_template,
};
pub use error::{Error, Result};
pub use ir::{Block, BlockContent, Deck, Paragraph, Role, Run, Slide, TextContent};
pub use md::parse_markdown;
pub use ooxml::read_file;
pub use render::{
    RasterError, rasterise_svg, render_deck_pngs, render_deck_svgs, render_slide, render_slide_png,
};
pub use template::Template;

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
