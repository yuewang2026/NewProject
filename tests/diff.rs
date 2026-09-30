//! End-to-end check of `deckr diff` (0.4).
//!
//! The changes here are made to a *real package*, not to an IR in memory: a
//! deck is written to disk, a second deck with known edits is written beside
//! it, and both are read back through the OOXML reader before being compared.
//! That is the whole point of the feature — the diff must work on what a
//! consumer actually holds, two `.pptx` files.

use std::io::Cursor;
use std::path::PathBuf;

use deckr::ir::{Block, BlockContent, Deck, Paragraph, Role, Slide, TextContent};
use deckr::{Change, diff_decks, diff_files, read_pptx, write_pptx_file};

fn text(role: Role, paragraphs: Vec<(&str, u8)>) -> Block {
    Block {
        role,
        content: BlockContent::Text(TextContent {
            paragraphs: paragraphs
                .into_iter()
                .map(|(t, l)| Paragraph::new(l, t))
                .collect(),
        }),
    }
}

fn table(rows: Vec<Vec<&str>>) -> Block {
    Block {
        role: Role::Table,
        content: BlockContent::Table {
            rows: rows
                .into_iter()
                .map(|r| r.into_iter().map(String::from).collect())
                .collect(),
        },
    }
}

/// The baseline: a two-slide deck with a table.
fn deck_v1() -> Deck {
    Deck {
        slides: vec![
            Slide {
                index: 0,
                blocks: vec![
                    text(Role::CenteredTitle, vec![("Quarterly Update", 0)]),
                    text(Role::Subtitle, vec![("Q2 results", 0)]),
                ],
            },
            Slide {
                index: 1,
                blocks: vec![
                    text(Role::Title, vec![("Roadmap", 0)]),
                    text(
                        Role::Body,
                        vec![("ship the reader", 0), ("keep it simple", 1)],
                    ),
                    table(vec![vec!["Phase", "Target"], vec!["0.2 write", "Q4"]]),
                ],
            },
        ],
    }
}

/// The revision: title changed, a bullet added, a cell updated, a slide added.
fn deck_v2() -> Deck {
    Deck {
        slides: vec![
            Slide {
                index: 0,
                blocks: vec![
                    text(Role::CenteredTitle, vec![("Quarterly Update", 0)]),
                    text(Role::Subtitle, vec![("Q3 results", 0)]),
                ],
            },
            Slide {
                index: 1,
                blocks: vec![
                    text(Role::Title, vec![("Roadmap", 0)]),
                    text(
                        Role::Body,
                        vec![
                            ("ship the reader", 0),
                            ("keep it simple", 1),
                            ("measure the loss", 1),
                        ],
                    ),
                    table(vec![vec!["Phase", "Target"], vec!["0.2 write", "4.1%"]]),
                ],
            },
            Slide {
                index: 2,
                blocks: vec![text(Role::Title, vec![("Thanks", 0)])],
            },
        ],
    }
}

/// Write a deck to a real .pptx and read it back — the diff input must be
/// what a consumer gets, not the IR we started from.
fn written(deck: &Deck, name: &str) -> (PathBuf, Deck) {
    let path = std::env::temp_dir().join(format!("deckr-diff-{}-{name}", std::process::id()));
    write_pptx_file(deck, &path).expect("writes");
    let back = read_pptx(&path).expect("reads back");
    (path, back)
}

#[test]
fn identical_files_report_nothing() {
    let (a, back) = written(&deck_v1(), "same-a.pptx");
    let (b, back2) = written(&deck_v1(), "same-b.pptx");
    let diff = diff_decks(&back, &back2);
    assert!(diff.is_empty(), "{:?}", diff.changes);
    assert!(diff_files(&a, &b).expect("diff").is_empty());
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

#[test]
fn real_edits_are_found_in_a_real_package() {
    let (pa, v1) = written(&deck_v1(), "v1.pptx");
    let (pb, v2) = written(&deck_v2(), "v2.pptx");

    let diff = diff_decks(&v1, &v2);
    assert_eq!(diff.old_slides, 2);
    assert_eq!(diff.new_slides, 3);

    // Subtitle changed on slide 1.
    assert!(diff.changes.contains(&Change::ParagraphChanged {
        slide: 1,
        role: Role::Subtitle,
        level: 0,
        from: "Q2 results".into(),
        to: "Q3 results".into(),
    }));
    // A bullet was added to the body on slide 2.
    assert!(diff.changes.contains(&Change::ParagraphAdded {
        slide: 2,
        role: Role::Body,
        level: 1,
        text: "measure the loss".into(),
    }));
    // The table cell moved from Q4 to 4.1%.
    assert!(diff.changes.contains(&Change::TableCellChanged {
        slide: 2,
        row: 2,
        col: 2,
        from: "Q4".into(),
        to: "4.1%".into(),
    }));
    // Slide 3 is new and named.
    assert!(diff.changes.contains(&Change::SlideAdded {
        slide: 3,
        title: Some("Thanks".into()),
    }));

    // And the report reads like the README promised.
    let report = diff.to_text("v1.pptx", "v2.pptx");
    assert!(report.contains("slides: 2 -> 3"), "{report}");
    assert!(report.contains("slide 1: ~ subtitle"), "{report}");
    assert!(report.contains("slide 2: + body bullet"), "{report}");
    assert!(
        report.contains("table cell (2,2) \"Q4\" -> \"4.1%\""),
        "{report}"
    );
    assert!(report.contains("slide 3: + slide \"Thanks\""), "{report}");

    let _ = std::fs::remove_file(&pa);
    let _ = std::fs::remove_file(&pb);
}

#[test]
fn a_removed_slide_is_found_in_a_real_package() {
    let (pa, v1) = written(&deck_v2(), "full.pptx");
    let (pb, v1_only) = written(&deck_v1(), "short.pptx");
    let diff = diff_files(&pa, &pb).expect("diff");
    assert!(
        diff.changes.contains(&Change::SlideRemoved {
            slide: 3,
            title: Some("Thanks".into()),
        }),
        "{:?}",
        diff.changes
    );
    let _ = std::fs::remove_file(&pa);
    let _ = std::fs::remove_file(&pb);
    let _ = v1_only;
    let _ = v1;
}

#[test]
fn the_change_list_travels_through_json() {
    let diff = diff_decks(&deck_v1(), &deck_v2());
    let json = serde_json::to_string(&diff).expect("serialises");
    let back: deckr::DeckDiff = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(diff, back);
}

/// A diff against an empty deck is just "everything was added" — and both
/// sides must survive a `Cursor` build too, since the writer takes any sink.
#[test]
fn empty_deck_side_reports_only_additions() {
    let empty = Deck::default();
    let mut buf: Vec<u8> = Vec::new();
    deckr::write_pptx(&deck_v1(), &mut Cursor::new(&mut buf)).expect("writes");
    let dir = std::env::temp_dir().join(format!("deckr-diff-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let path = dir.join("one.pptx");
    std::fs::write(&path, &buf).expect("write");
    let deck = read_pptx(&path).expect("reads");
    let diff = diff_decks(&empty, &deck);
    assert_eq!(diff.changes.len(), 2, "{:?}", diff.changes);
    let _ = std::fs::remove_dir_all(&dir);
}
