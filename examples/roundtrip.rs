//! The example printed in the README, kept here so that CI compiles it.
//!
//! Documentation examples rot silently; this one fails the build.
//!
//! Note the return type: deckr keeps the path attached to its own I/O errors
//! (`Error::Io { path, .. }`), which a blanket `From<std::io::Error>` would
//! throw away, so callers that also touch the filesystem want `Box<dyn Error>`.

use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let deck = deckr::read_pptx(Path::new("deck.pptx"))?;

    println!("{} slides", deck.len());
    for (i, title) in deck.outline().iter().enumerate() {
        println!("{}. {}", i + 1, title.as_deref().unwrap_or("(untitled)"));
    }

    std::fs::write("deck.md", deckr::markdown::to_markdown(&deck))?;

    let report = deckr::write_pptx_file(&deck, Path::new("rebuilt.pptx"))?;
    println!(
        "{} block(s) written, {} lost, {} placed loose",
        report.blocks_written,
        report.skipped_count(),
        report.relocated.len()
    );
    Ok(())
}
