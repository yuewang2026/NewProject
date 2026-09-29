//! `deckr` command line interface.

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Read, write and compare PowerPoint files without PowerPoint.
#[derive(Debug, Parser)]
#[command(
    name = "deckr",
    version,
    about = "Read, write and compare PowerPoint files without Office.",
    long_about = "deckr turns .pptx into a semantic Deck IR and back again.\n\n\
                  Phase 0 ships the reading side: inspect and convert. Writing\n\
                  (build), rendering (render) and comparison (diff) land next."
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
    }
    Ok(())
}
