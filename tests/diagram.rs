//! End-to-end check of SmartArt (diagram) round-tripping (0.4).
//!
//! The fixture ships a real diagram frame — a data model with three text
//! points, a layout, a quick style, a colour scheme and the pre-rendered
//! drawing. Everything here asserts on what the reader captures from the real
//! `.pptx` and what the writer puts back.

use std::path::{Path, PathBuf};
use std::process::Command;

use deckr::ir::{Block, BlockContent, Deck, Role};
use deckr::{read_pptx, write_pptx_file};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sample_path() -> PathBuf {
    fixtures_dir().join("sample.pptx")
}

fn ensure_sample() -> PathBuf {
    let path = sample_path();
    if path.exists() {
        return path;
    }
    let py = fixtures_dir().join("make_sample_pptx.py");
    for exe in ["python3", "python"] {
        if Command::new(exe)
            .arg(&py)
            .arg(&path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && path.exists()
        {
            return path;
        }
    }
    panic!("sample.pptx is missing and Python was not available to generate it");
}

fn diagram_block() -> (Block, Deck) {
    ensure_sample();
    let deck = read_pptx(&sample_path()).expect("reads");
    let block = deck
        .slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find(|b| b.role == Role::Diagram)
        .expect("a diagram block")
        .clone();
    (block, deck)
}

#[test]
fn the_reader_captures_the_whole_subgraph() {
    let (block, _) = diagram_block();
    let blob = match &block.content {
        BlockContent::Diagram {
            blob: Some(blob), ..
        } => blob,
        other => panic!("expected a diagram with a captured blob, got {other:?}"),
    };
    let names: Vec<&str> = blob.parts.iter().map(|p| p.path.as_str()).collect();
    for want in [
        "ppt/diagrams/data1.xml",
        "ppt/diagrams/layout1.xml",
        "ppt/diagrams/quickStyle1.xml",
        "ppt/diagrams/colors1.xml",
        // The pre-rendered drawing hangs off the data part's relationships.
        "ppt/diagrams/drawing1.xml",
        "ppt/diagrams/_rels/data1.xml.rels",
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
}

#[test]
fn the_data_model_texts_are_decoded() {
    let (block, _) = diagram_block();
    match &block.content {
        BlockContent::Diagram { texts, .. } => {
            assert_eq!(texts, &["Collect", "Convert", "Ship"]);
        }
        other => panic!("expected a diagram, got {other:?}"),
    }
}

#[test]
fn blob_and_texts_travel_through_json() {
    let (block, _) = diagram_block();
    let json = deckr::to_json(&deck_from_blocks(block)).expect("serialises");
    assert!(json.contains("\"blob\""), "{json}");
    assert!(json.contains("\"texts\""), "{json}");
    assert!(json.contains("Convert"), "{json}");
    let back = deckr::from_json(&json).expect("deserialises");
    let back_block = back
        .slides
        .into_iter()
        .flat_map(|s| s.blocks)
        .find(|b| b.role == Role::Diagram)
        .expect("diagram survives JSON");
    match &back_block.content {
        BlockContent::Diagram {
            texts,
            blob: Some(blob),
            ..
        } => {
            assert_eq!(texts, &["Collect", "Convert", "Ship"]);
            assert!(blob.parts.len() >= 5);
        }
        other => panic!("blob lost through JSON: {other:?}"),
    }
}

fn deck_from_blocks(block: Block) -> deckr::Deck {
    deckr::Deck {
        slides: vec![deckr::Slide {
            index: 0,
            blocks: vec![block],
        }],
    }
}

#[test]
fn a_written_diagram_reopens_byte_identical() {
    let (block, _) = diagram_block();
    let deck = deck_from_blocks(block);
    let out = std::env::temp_dir().join(format!("deckr-diagram-{}.pptx", std::process::id()));
    write_pptx_file(&deck, &out).expect("writes");

    let back = read_pptx(&out).expect("the rebuilt package re-opens");
    let rebuilt = back
        .slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find(|b| b.role == Role::Diagram)
        .expect("diagram survives the rebuild");
    match (&rebuilt.content, first_diagram_blob(&deck)) {
        (
            BlockContent::Diagram {
                texts: back_texts,
                blob: Some(back_blob),
                ..
            },
            Some(orig),
        ) => {
            assert_eq!(back_texts, &["Collect", "Convert", "Ship"]);
            assert_eq!(back_blob.parts.len(), orig.parts.len());
            for (a, b) in orig.parts.iter().zip(back_blob.parts.iter()) {
                assert_eq!(a.path, b.path, "part paths must match");
                assert_eq!(a.bytes, b.bytes, "part {} must be byte-identical", a.path);
            }
        }
        other => panic!("expected a diagram with a blob, got {other:?}"),
    }
    let _ = std::fs::remove_file(&out);
}

fn first_diagram_blob(deck: &deckr::Deck) -> Option<&deckr::ir::DiagramBlob> {
    deck.slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find_map(|b| match &b.content {
            BlockContent::Diagram {
                blob: Some(blob), ..
            } => Some(blob),
            _ => None,
        })
}

#[test]
fn markdown_export_carries_the_diagram_texts() {
    let (block, _) = diagram_block();
    let deck = deck_from_blocks(block);
    let md = deckr::markdown::to_markdown(&deck);
    assert!(md.contains("`[diagram]`"), "{md}");
    assert!(md.contains("- Collect"), "{md}");
    assert!(md.contains("- Convert"), "{md}");
    assert!(md.contains("- Ship"), "{md}");
}

#[test]
fn diff_reports_a_reworded_node_and_a_restructure() {
    let (block, deck) = diagram_block();

    // A reworded node: one text point changes.
    let mut reworded = deck.clone();
    reworded.slides = deck_from_blocks(block.clone()).slides;
    let diag = reworded
        .slides
        .iter_mut()
        .flat_map(|s| &mut s.blocks)
        .find(|b| b.role == Role::Diagram)
        .and_then(|b| match &mut b.content {
            BlockContent::Diagram { texts, .. } => Some(texts),
            _ => None,
        })
        .expect("a diagram with texts");
    diag[2] = "Deliver".to_string();

    let diff = deckr::diff_decks(&deck, &reworded);
    assert!(
        diff.changes.contains(&deckr::Change::ParagraphChanged {
            slide: 1,
            role: Role::Diagram,
            level: 0,
            from: "Ship".into(),
            to: "Deliver".into(),
        }),
        "{:?}",
        diff.changes
    );

    // Same words, restructured data model: a structural verdict, not noise.
    let mut restructured = deck.clone();
    let blob = restructured
        .slides
        .iter_mut()
        .flat_map(|s| &mut s.blocks)
        .find(|b| b.role == Role::Diagram)
        .and_then(|b| match &mut b.content {
            BlockContent::Diagram { blob, .. } => blob.as_mut(),
            _ => None,
        })
        .expect("a diagram with a blob");
    if let Some(data) = blob
        .parts
        .iter_mut()
        .find(|p| p.content_type.contains("diagramData"))
    {
        data.bytes[0] ^= 0xFF;
    }
    let diff = deckr::diff_decks(&deck, &restructured);
    assert!(
        diff.changes.iter().any(|c| matches!(
            c,
            deckr::Change::MediaChanged { description, .. }
                if description.contains("diagram structure changed")
        )),
        "{:?}",
        diff.changes
    );
}
