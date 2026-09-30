//! `deckr` command line interface.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

/// Read, write and compare PowerPoint files without PowerPoint.
#[derive(Debug, Parser)]
#[command(
    name = "deckr",
    version,
    about = "Read, write and compare PowerPoint files without Office.",
    long_about = "deckr turns .pptx into a semantic Deck IR and back again.\n\n\
                  Reading and writing both work. Blocks are bound to the master's\n\
                  placeholders by role, never by coordinate, so what comes out\n\
                  inherits the template instead of fighting it.\n\n\
                  `check` round-trips a file and tells you exactly what did not\n\
                  survive — the number other converters cannot give you, because\n\
                  they have no intermediate representation to compare against."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show what is inside a deck.
    Inspect {
        /// The .pptx to read.
        file: PathBuf,
        /// Full Deck IR as JSON instead of a human summary.
        #[arg(short, long)]
        json: bool,
        /// Print the block inventory grouped by role.
        #[arg(short, long)]
        roles: bool,
    },

    /// Convert a deck to another representation.
    Convert {
        /// The .pptx to read.
        file: PathBuf,
        /// Output format: `md` (default) or `json`.
        #[arg(short, long, default_value = "md")]
        to: String,
        /// Write here instead of stdout.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Build a .pptx from Markdown or Deck IR JSON.
    ///
    /// Content is bound to the matching master placeholder by role; nothing is
    /// positioned by hand, so the result inherits whatever theme you point the
    /// layout at.
    Build {
        /// Input file: `.md` / `.markdown` otherwise `.json` (Deck IR).
        file: PathBuf,
        /// Write the .pptx here. Defaults to the input stem.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Round-trip a deck through the writer and report what came back.
    ///
    /// The loss number other converters cannot give you, because they have no
    /// intermediate representation to compare against.
    Check {
        /// The .pptx to measure.
        file: PathBuf,
    },

    /// Render a deck to per-slide SVG (and optionally PNG) previews.
    ///
    /// Vector output (one `slide_N.svg` per slide plus a gallery `index.html`)
    /// is built with no external dependencies; pass `--png` to additionally
    /// rasterise each slide to `slide_N.png` using `resvg` (pure Rust).
    Render {
        /// The .pptx to read.
        file: PathBuf,
        /// Directory to write `slide_N.svg` and `index.html` into.
        #[arg(short, long, default_value = "deckr_render")]
        out: PathBuf,
        /// Also rasterise each slide to `slide_N.png` with resvg.
        #[arg(long)]
        png: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("error: {err}");
        let mut first = err.source();
        while let Some(cause) = first {
            eprintln!("  caused by: {cause}");
            first = cause.source();
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Inspect { file, json, roles } => {
            let deck = deckr::read_pptx(&file)?;
            if json {
                println!("{}", deckr::to_json(&deck)?);
            } else if roles {
                for (role, count) in deckr::markdown::role_histogram(&deck) {
                    println!("{count:>6}  {role}");
                }
            } else {
                println!("{}", file.display());
                println!("  slides: {}", deck.len());
                for (i, slide) in deck.slides.iter().enumerate() {
                    let title = slide.title().unwrap_or_else(|| "(untitled)".to_string());
                    let filled = slide.blocks.iter().filter(|b| !b.is_empty()).count();
                    println!("  {}. {title}  [{filled} block(s)]", i + 1);
                }
            }
        }

        Command::Convert { file, to, output } => {
            let deck = deckr::read_pptx(&file)?;
            let rendered = match to.as_str() {
                "md" | "markdown" => deckr::markdown::to_markdown(&deck),
                "json" => deckr::to_json(&deck)?,
                other => {
                    return Err(format!("unknown --to format `{other}` (try md or json)").into());
                }
            };
            match output {
                Some(path) => {
                    let mut f = std::fs::File::create(&path)?;
                    f.write_all(rendered.as_bytes())?;
                    eprintln!("wrote {}", path.display());
                }
                None => print!("{rendered}"),
            }
        }
        Command::Build { file, output } => {
            let source = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let deck = match extension_of(&file).as_str() {
                "md" | "markdown" | "txt" => deckr::parse_markdown(&source),
                "json" => deckr::from_json(&source)?,
                other => {
                    return Err(format!(
                        "don't know how to build from `.{other}` (try .md or .json)"
                    )
                    .into());
                }
            };
            if deck.is_empty() {
                return Err(format!("{} produced no slides", file.display()).into());
            }

            let out = output.unwrap_or_else(|| file.with_extension("pptx"));
            let report = deckr::write_pptx_file(&deck, &out)?;
            println!("wrote {}", out.display());
            println!("  slides: {}", report.slides);
            println!("  blocks written: {}", report.blocks_written);
            if !report.relocated.is_empty() {
                println!(
                    "  {} block(s) placed loose (kept, but no longer styled by the master):",
                    report.relocated.len()
                );
                for s in &report.relocated {
                    println!("    {s}");
                }
            }
            if report.skipped.is_empty() {
                println!("  loss: none");
            } else {
                println!(
                    "  loss: {} block(s) could not be placed",
                    report.skipped.len()
                );
                for s in &report.skipped {
                    println!("    {s}");
                }
            }
        }

        Command::Check { file } => {
            let original = deckr::read_pptx(&file)?;
            let rebuilt =
                std::env::temp_dir().join(format!("deckr-check-{}.pptx", std::process::id()));
            let report = deckr::write_pptx_file(&original, &rebuilt)?;
            let back = deckr::read_pptx(&rebuilt)?;
            let _ = std::fs::remove_file(&rebuilt);

            let expected = original.slides.len();
            let got = back.slides.len();
            let expected_paras = text_paragraphs(&original).len();
            let got_paras = text_paragraphs(&back).len();
            let titles: usize = original
                .outline()
                .iter()
                .zip(back.outline().iter())
                .filter(|(a, b)| a == b)
                .count();

            println!("{}", file.display());
            println!("  slides:     {got} / {expected}");
            println!("  titles:     {titles} / {expected} identical");
            println!("  paragraphs: {got_paras} / {expected_paras}");
            if !report.relocated.is_empty() {
                println!(
                    "  placed loose: {} block(s) kept without master styling",
                    report.relocated.len()
                );
                for s in &report.relocated {
                    println!("    {s}");
                }
            } else {
                println!("  placed loose: none");
            }
            if report.skipped.is_empty() {
                println!("  unplaceable blocks: none");
            } else {
                println!(
                    "  unplaceable blocks: {} (not counted as loss — needs 0.4)",
                    report.skipped.len()
                );
                for s in &report.skipped {
                    println!("    {s}");
                }
            }
            if expected_paras != got_paras {
                println!(
                    "  ⚠ {} paragraph(s) differ after the round trip",
                    expected_paras.abs_diff(got_paras)
                );
            }
        }

        Command::Render { file, out, png } => {
            let deck = deckr::read_pptx(&file)?;
            std::fs::create_dir_all(&out)?;
            let pages = deckr::render_deck_svgs(&deck);
            for (n, svg) in &pages {
                std::fs::write(out.join(format!("slide_{n}.svg")), svg)?;
            }
            if png {
                let mut written = 0usize;
                for (n, svg) in &pages {
                    match deckr::rasterise_svg(svg) {
                        Ok(bytes) => {
                            std::fs::write(out.join(format!("slide_{n}.png")), &bytes)?;
                            written += 1;
                        }
                        Err(e) => eprintln!("  warning: could not rasterise slide {n}: {e}"),
                    }
                }
                println!("  rasterised {written} slide(s) to PNG");
            }
            std::fs::write(out.join("index.html"), gallery_html(&pages, png))?;
            println!("rendered {} slide(s) to {}", pages.len(), out.display());
            println!("  open {}/index.html to preview", out.display());
        }
    }
    Ok(())
}

/// A minimal gallery page that embeds each slide image for quick eyeballing.
///
/// When `use_png` is set the gallery references the rasterised `slide_N.png`
/// (broadly viewable); otherwise it embeds the vector `slide_N.svg`.
fn gallery_html(pages: &[(usize, String)], use_png: bool) -> String {
    let ext = if use_png { "png" } else { "svg" };
    let mut body = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>deckr render</title>\
         <style>body{font-family:Segoe UI,Arial,sans-serif;background:#f4f4f4;\
         margin:0;padding:24px}h1{font-weight:700;color:#1F3864}figure{background:#fff;\
         margin:0 0 24px;padding:12px;border:1px solid #ddd;border-radius:6px}\
         figcaption{color:#555;margin-top:8px}img{width:100%;height:auto;\
         border:1px solid #eee}</style></head><body><h1>deckr render</h1>",
    );
    for (n, _) in pages {
        body.push_str(&format!(
            "<figure><img src=\"slide_{n}.{ext}\" alt=\"slide {n}\">\
             <figcaption>Slide {n}</figcaption></figure>"
        ));
    }
    body.push_str("</body></html>");
    body
}

/// Every (level, text) pair in the deck that the writer is expected to place:
/// text roles only, because pictures, charts and the page furniture are handled
/// separately and would make this number lie.
fn text_paragraphs(deck: &deckr::Deck) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    for slide in &deck.slides {
        for block in &slide.blocks {
            if block.role.is_chrome() || !block.role.is_text_role() {
                continue;
            }
            if let Some(text) = block.as_text() {
                for p in &text.paragraphs {
                    if !p.text.trim().is_empty() {
                        out.push((p.level, p.text.trim().to_string()));
                    }
                }
            }
        }
    }
    out
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}
