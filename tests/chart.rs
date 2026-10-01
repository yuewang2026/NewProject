//! End-to-end check of chart number decoding — `ChartBlob` → `ChartData` (0.4).
//!
//! The fixture chart now ships two real series (`Revenue`, `Cost` across
//! Q1–Q3, with a deliberate gap in `Cost`'s Q2). Everything here reads the
//! real `.pptx` produced by `make_sample_pptx.py` and asserts on what the
//! reader actually decodes — not on a hand-built IR.

use std::path::{Path, PathBuf};
use std::process::Command;

use deckr::ir::{BlockContent, Role};
use deckr::{Change, read_pptx};

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

fn chart_data(path: &Path) -> deckr::ChartData {
    let deck = read_pptx(path).expect("reads");
    deck.slides[0]
        .blocks
        .iter()
        .find(|b| b.role == Role::Chart)
        .expect("a chart block")
        .as_chart_data()
        .cloned()
        .expect("the fixture chart decodes")
}

#[test]
fn the_fixture_chart_decodes_to_real_numbers() {
    ensure_sample();
    let d = chart_data(&sample_path());

    assert_eq!(d.title.as_deref(), Some("growth"));
    assert_eq!(d.kind, deckr::ChartKind::Bar);
    assert_eq!(d.series.len(), 2);

    let revenue = &d.series[0];
    assert_eq!(revenue.name.as_deref(), Some("Revenue"));
    assert_eq!(revenue.categories, vec!["Q1", "Q2", "Q3"]);
    assert_eq!(revenue.values, vec![Some(1.5), Some(2.5), Some(3.5)]);

    // The fixture deliberately leaves Cost's Q2 empty — a gap, not a zero.
    let cost = &d.series[1];
    assert_eq!(cost.name.as_deref(), Some("Cost"));
    assert_eq!(cost.values, vec![Some(1.1), None, Some(3.9)]);
}

#[test]
fn the_decoded_caption_fills_the_chart_block() {
    ensure_sample();
    let deck = read_pptx(&sample_path()).expect("reads");
    let chart = deck.slides[0]
        .blocks
        .iter()
        .find(|b| b.role == Role::Chart)
        .expect("a chart block");
    match &chart.content {
        BlockContent::Chart { caption, data, .. } => {
            assert_eq!(caption.as_deref(), Some("growth"));
            assert!(data.is_some());
        }
        other => panic!("expected a chart, got {other:?}"),
    }
}

#[test]
fn markdown_export_carries_the_numbers_as_a_table() {
    ensure_sample();
    let deck = read_pptx(&sample_path()).expect("reads");
    let md = deckr::markdown::to_markdown(&deck);
    assert!(md.contains("`[chart]`"), "{md}");
    assert!(md.contains("| Revenue | Cost |"), "{md}");
    assert!(md.contains("| Q1 | 1.5 | 1.1 |"), "{md}");
    // The gap is an empty cell, not a 0.
    assert!(md.contains("| Q2 | 2.5 |  |"), "{md}");
    assert!(md.contains("| Q3 | 3.5 | 3.9 |"), "{md}");
}

#[test]
fn decoded_numbers_travel_through_json() {
    ensure_sample();
    let deck = read_pptx(&sample_path()).expect("reads");
    let json = deckr::to_json(&deck).expect("serialises");
    assert!(json.contains("\"data\""), "{json}");
    assert!(json.contains("\"categories\""), "{json}");
    let back = deckr::from_json(&json).expect("deserialises");
    let d = back.slides[0]
        .blocks
        .iter()
        .find(|b| b.role == Role::Chart)
        .and_then(|b| b.as_chart_data())
        .expect("data survives JSON");
    assert_eq!(d.series.len(), 2);
    assert_eq!(d.series[0].values, vec![Some(1.5), Some(2.5), Some(3.5)]);
}

#[test]
fn a_changed_number_is_diffed_at_point_level() {
    ensure_sample();
    let old = read_pptx(&sample_path()).expect("reads");
    let mut new = old.clone();
    let chart = new.slides[0]
        .blocks
        .iter_mut()
        .find(|b| b.role == Role::Chart)
        .and_then(|b| match &mut b.content {
            BlockContent::Chart { data, .. } => data.as_mut(),
            _ => None,
        })
        .expect("a chart with data");
    chart.series[0].values[2] = Some(4.1);

    let diff = deckr::diff_decks(&old, &new);
    assert!(
        diff.changes.contains(&Change::ChartValueChanged {
            slide: 1,
            series: "Revenue".into(),
            category: "Q3".into(),
            from: "3.5".into(),
            to: "4.1".into(),
        }),
        "{:?}",
        diff.changes
    );
    // One number changed, so exactly one value-level report — no byte-level
    // "chart content changed" noise alongside it.
    assert_eq!(diff.changes.len(), 1);
}

#[test]
fn edited_numbers_survive_a_write_and_reopen() {
    let deck = read_pptx(&ensure_sample()).expect("reads");
    let mut edited = deck.clone();
    let chart = edited
        .slides
        .iter_mut()
        .flat_map(|s| &mut s.blocks)
        .find(|b| b.role == Role::Chart)
        .and_then(|b| match &mut b.content {
            BlockContent::Chart { data, .. } => data.as_mut(),
            _ => None,
        })
        .expect("a chart with decoded data");
    chart.series[0].values[2] = Some(4.1);
    chart.series[0].name = Some("Net revenue".into());

    let path = std::env::temp_dir().join(format!("deckr-chart-edit-{}.pptx", std::process::id()));
    deckr::write_pptx_file(&edited, &path).expect("writes");
    let back = read_pptx(&path).expect("re-opens");
    let _ = std::fs::remove_file(&path);

    let back_data = back
        .slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find(|b| b.role == Role::Chart)
        .and_then(|b| b.as_chart_data())
        .expect("chart data decoded after the rebuild");
    assert_eq!(
        back_data.series[0].values,
        vec![Some(1.5), Some(2.5), Some(4.1)]
    );
    assert_eq!(back_data.series[0].name.as_deref(), Some("Net revenue"));
    // The untouched series is exactly as it was.
    assert_eq!(back_data.series[1].values, vec![Some(1.1), None, Some(3.9)]);
}

/// An unedited deck must still round-trip byte-identically: re-authoring
/// kicks in only when the decoded view has drifted from the captured bytes.
#[test]
fn an_unedited_chart_still_round_trips_verbatim() {
    let deck = read_pptx(&ensure_sample()).expect("reads");
    let original = deck
        .slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find(|b| b.role == Role::Chart)
        .and_then(|b| b.as_chart_data())
        .cloned()
        .expect("chart data");

    let path = std::env::temp_dir().join(format!("deckr-chart-same-{}.pptx", std::process::id()));
    deckr::write_pptx_file(&deck, &path).expect("writes");
    let back = read_pptx(&path).expect("re-opens");
    let _ = std::fs::remove_file(&path);

    let back_data = back
        .slides
        .iter()
        .flat_map(|s| &s.blocks)
        .find(|b| b.role == Role::Chart)
        .and_then(|b| b.as_chart_data())
        .expect("chart data after rebuild");
    assert_eq!(*back_data, original);
}

#[test]
fn a_series_added_or_removed_is_named() {
    ensure_sample();
    let old = read_pptx(&sample_path()).expect("reads");
    let mut new = old.clone();
    let chart = new.slides[0]
        .blocks
        .iter_mut()
        .find(|b| b.role == Role::Chart)
        .and_then(|b| match &mut b.content {
            BlockContent::Chart { data, .. } => data.as_mut(),
            _ => None,
        })
        .expect("a chart with data");
    chart.series.truncate(1);

    let diff = deckr::diff_decks(&old, &new);
    assert!(
        diff.changes.contains(&Change::ChartSeriesRemoved {
            slide: 1,
            name: "Cost".into(),
        }),
        "{:?}",
        diff.changes
    );
}

#[test]
fn the_render_draws_the_chart_instead_of_a_placeholder() {
    ensure_sample();
    let deck = read_pptx(&sample_path()).expect("reads");
    let svg = deckr::render_slide(&deck.slides[0]);

    // Bars in the two series colours, a legend naming both series, the chart
    // title, and category labels.
    assert!(svg.contains("fill=\"#4472C4\""), "{svg}");
    assert!(svg.contains("fill=\"#C00000\""), "{svg}");
    assert!(svg.contains(">Revenue<"), "{svg}");
    assert!(svg.contains(">Cost<"), "{svg}");
    assert!(svg.contains(">growth<"), "{svg}");
    assert!(svg.contains(">Q1<"), "{svg}");
    // The y-axis speaks in the data's own numbers — the max is Cost's peak.
    assert!(svg.contains(">3.9<"), "{svg}");
}
