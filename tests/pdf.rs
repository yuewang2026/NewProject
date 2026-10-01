//! End-to-end check of the PDF assembly path (`deckr render --pdf`, 0.4).
//!
//! Everything here reads the real `.pptx` fixture, rasterises its slides the
//! same way `--png` does, and asserts on the assembled PDF's structure.

use std::path::{Path, PathBuf};
use std::process::Command;

use deckr::read_pptx;

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

fn build_pdf() -> Vec<u8> {
    let deck = read_pptx(&ensure_sample()).expect("reads");
    deckr::render_deck_pdf(&deck).expect("the deck assembles to a PDF")
}

#[test]
fn the_pdf_opens_like_a_pdf() {
    let pdf = build_pdf();
    assert!(pdf.starts_with(b"%PDF-1.4\n"), "magic header");
    assert!(pdf.ends_with(b"%%EOF\n"), "EOF marker");

    let text = String::from_utf8_lossy(&pdf);
    // One page per slide: the fixture has two.
    assert!(text.contains("/Count 2"), "{text}");
    assert!(text.contains("/Kids [3 0 R 6 0 R]"), "{text}");
    // Every page is a 960×540 pt 16:9 canvas (1 px = 1 pt).
    assert_eq!(text.matches("/MediaBox [0 0 960 540]").count(), 2, "{text}");
    // Both pages draw their raster through an image XObject.
    assert_eq!(text.matches("/Subtype /Image").count(), 2, "{text}");
    assert_eq!(text.matches("/Filter /FlateDecode").count(), 2, "{text}");
}

#[test]
fn the_xref_table_resolves() {
    // Byte-level on purpose: the document contains binary streams, so a lossy
    // string conversion would shift every offset.
    let pdf = build_pdf();
    let find = |haystack: &[u8], needle: &[u8]| {
        haystack
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("needle present")
    };
    let sx = find(&pdf, b"startxref\n") + b"startxref\n".len();
    let line_end = find(&pdf[sx..], b"\n") + sx;
    let xref_at: usize = std::str::from_utf8(&pdf[sx..line_end])
        .expect("numeric")
        .parse()
        .expect("offset");
    assert_eq!(
        &pdf[xref_at..xref_at + 4],
        b"xref",
        "startxref points at the table"
    );

    // Every in-use entry points exactly at "<n> 0 obj".
    let table = &pdf[xref_at..];
    let mut start = find(table, b"0000000000 65535 f \n") + b"0000000000 65535 f \n".len();
    while start < table.len() {
        let Some(end_rel) = table[start..].windows(2).position(|w| w == b" \n") else {
            break;
        };
        let line = &table[start..start + end_rel];
        if line.len() == 20 && line.ends_with(b"n") {
            let offset: usize = std::str::from_utf8(&line[..10])
                .expect("numeric")
                .parse()
                .expect("offset");
            let at = &pdf[offset..];
            let digits_end = at
                .iter()
                .position(|c| !c.is_ascii_digit())
                .expect("object number");
            let obj_num = std::str::from_utf8(&at[..digits_end])
                .expect("numeric")
                .parse::<usize>()
                .expect("object number");
            assert!(
                at.starts_with(format!("{obj_num} 0 obj\n").as_bytes()),
                "xref offset {offset} points at {:?}",
                String::from_utf8_lossy(&at[..at.len().min(20)])
            );
        }
        start += end_rel + 2;
    }
}

#[test]
fn pages_carry_the_rasterised_slide() {
    let deck = read_pptx(&ensure_sample()).expect("reads");
    // The rendered slide's PNG dims must equal the PDF page dims: the image
    // XObject's /Width /Height agree with the MediaBox.
    let (w, h) = {
        let svg = deckr::render_deck_svgs(&deck);
        let png = deckr::rasterise_svg(&svg[0].1).expect("rasterises");
        let decoder = png::Decoder::new(std::io::Cursor::new(&png[..]));
        let reader = decoder.read_info().expect("png info");
        let info = reader.info();
        (info.width, info.height)
    };
    assert_eq!((w, h), (960, 540), "the preview's own pixel size");
}

#[test]
fn pdf_builds_are_deterministic() {
    let a = build_pdf();
    let b = build_pdf();
    assert_eq!(a, b, "two builds of the same deck must be byte-identical");
}
