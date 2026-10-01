//! PDF assembly — the raster path from slides to a single `.pdf` file.
//!
//! The SVG preview is the vector source of truth and the PNG is the single
//! bitmap; the PDF is where they land together: one page per slide, each page
//! an image XObject of the rasterised slide, sized so that one SVG pixel is
//! one PDF point (the 960×540 preview becomes a 13.3×7.5 inch 16:9 page —
//! exactly PowerPoint's default slide size at 72 dpi).
//!
//! The writer is deliberately tiny and hand-rolled rather than a `printpdf`
//! dependency: a PDF that only carries flate-compressed RGB image pages needs
//! catalog + pages tree + per-page (page, content, image) objects and an xref
//! table — a few hundred lines, no features we would not use, and full control
//! over determinism. Like every other deckr output, two builds of the same
//! deck are byte-identical: no timestamps, no random ids.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::ir::Deck;
use crate::render::{RasterError, rasterise_svg, render_deck_svgs};

/// One rendered page: pixel width, pixel height, and 8-bit RGBA rows.
pub struct PdfPage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row-major, top-down.
    pub rgba: Vec<u8>,
}

/// Render a whole deck to one PDF, one page per slide.
///
/// Every slide's SVG preview is rasterised with `resvg` (the same path as
/// `--png`) and the bitmaps are assembled into a deterministic PDF. Slides
/// that fail to rasterise abort the build — a page silently missing from a
/// document is exactly the kind of quiet loss deckr refuses to commit.
pub fn render_deck_pdf(deck: &Deck) -> std::result::Result<Vec<u8>, RasterError> {
    let mut pages = Vec::new();
    for (n, svg) in render_deck_svgs(deck) {
        let (w, h, rgba) =
            rasterise_svg_rgba(&svg).map_err(|e| RasterError::Render(format!("slide {n}: {e}")))?;
        pages.push(PdfPage {
            width: w,
            height: h,
            rgba,
        });
    }
    Ok(assemble_pdf(&pages))
}

/// Rasterise one standalone SVG to raw RGBA pixels rather than PNG bytes.
pub fn rasterise_svg_rgba(svg: &str) -> std::result::Result<(u32, u32, Vec<u8>), RasterError> {
    let png = rasterise_svg(svg)?;
    decode_png_rgba(&png).ok_or_else(|| RasterError::Render("png could not be decoded".into()))
}

/// Minimal PNG decode for the rasteriser's own output (RGBA8). The `png`
/// crate is already in the tree through resvg, so decoding through it costs
/// nothing and hand-rolling inflate-then-unfilter buys nothing.
fn decode_png_rgba(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().ok()?;
    if reader.info().color_type != png::ColorType::Rgba
        || reader.info().bit_depth != png::BitDepth::Eight
    {
        return None; // resvg always emits RGBA8; anything else is unexpected.
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

// ---------------------------------------------------------------------------
// the PDF writer proper
// ---------------------------------------------------------------------------

/// Assemble pages into a PDF 1.4 document. Deterministic by construction.
///
/// Object layout: 1 catalog, 2 pages tree, then per page `i` (0-based) —
/// `3 + 3i` page, `4 + 3i` content stream, `5 + 3i` image XObject.
pub fn assemble_pdf(pages: &[PdfPage]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new(); // offset of object i+1

    let n = pages.len();
    let page_id = |i: usize| 3 + 3 * i;

    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    object(
        &mut out,
        &mut offsets,
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
    );

    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_id(i))).collect();
    object(
        &mut out,
        &mut offsets,
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n).as_bytes(),
    );

    for (i, page) in pages.iter().enumerate() {
        let (w, h) = (page.width, page.height);
        let image_id = 5 + 3 * i;
        let contents_id = 4 + 3 * i;

        // The page bitmap: composited over white, packed RGB, deflated.
        let compressed = deflate(&flatten_alpha(page));
        stream_object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
                 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode \
                 /Length {} >>\nstream\n",
                compressed.len()
            ),
            &compressed,
        );

        // Content stream: draw the image filling the page (1 px = 1 pt).
        let content = format!("q\n{w} 0 0 {h} 0 0 cm\n/Im0 Do\nQ");
        stream_object(
            &mut out,
            &mut offsets,
            format!("<< /Length {} >>\nstream\n", content.len()),
            content.as_bytes(),
        );

        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
                 /Resources << /XObject << /Im0 {image_id} 0 R >> >> \
                 /Contents {contents_id} 0 R >>"
            )
            .as_bytes(),
        );
    }

    // Cross-reference table and trailer.
    let xref_at = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

/// Append one plain (`obj ... endobj`) object, recording its offset.
fn object(out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]) {
    offsets.push(out.len());
    out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendobj\n");
}

/// Append one object whose body is a stream. `dict` must end with
/// `stream\n`; the data's `/Length` is the caller's to state. The trailing
/// newline before `endstream` is the EOL the spec allows and excludes.
fn stream_object(out: &mut Vec<u8>, offsets: &mut Vec<usize>, dict: String, data: &[u8]) {
    offsets.push(out.len());
    out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
    out.extend_from_slice(dict.as_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
}

/// Composite RGBA over an opaque white page, yielding packed RGB rows.
fn flatten_alpha(page: &PdfPage) -> Vec<u8> {
    let mut out = Vec::with_capacity((page.width * page.height * 3) as usize);
    for px in page.rgba.chunks_exact(4) {
        let a = px[3] as u32;
        for c in &px[..3] {
            // out = c*a + 255*(1-a), rounded, in integer arithmetic.
            let v = (*c as u32 * a + 255 * (255 - a) + 127) / 255;
            out.push(v as u8);
        }
    }
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 page: left pixel red, right pixel transparent (reads as white).
    fn two_pixel_page() -> PdfPage {
        PdfPage {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
        }
    }

    #[test]
    fn the_document_carries_the_pdf_skeleton() {
        let pdf = assemble_pdf(&[two_pixel_page()]);
        assert!(pdf.starts_with(b"%PDF-1.4\n"), "magic header");
        assert!(pdf.ends_with(b"%%EOF\n"), "EOF marker");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Catalog"), "{text}");
        assert!(text.contains("/Type /Pages"), "{text}");
        assert_eq!(
            text.matches("/Type /Page ").count(),
            1,
            "exactly one page: {text}"
        );
        assert!(text.contains("/Filter /FlateDecode"), "{text}");
    }

    #[test]
    fn alpha_composites_onto_white_not_black() {
        let rgb = flatten_alpha(&two_pixel_page());
        assert_eq!(rgb, vec![255, 0, 0, 255, 255, 255]);
    }

    #[test]
    fn stream_lengths_match_the_data_on_the_wire() {
        // Byte-level on purpose: the document contains binary streams, so a
        // lossy string conversion would shift every offset.
        let pdf = assemble_pdf(&[two_pixel_page()]);
        let mut cursor = 0usize;
        while let Some(pos) = find(&pdf[cursor..], b"/Length ") {
            let abs = cursor + pos + b"/Length ".len();
            let digits_end = abs + pdf[abs..].iter().take_while(|c| c.is_ascii_digit()).count();
            let declared: usize = std::str::from_utf8(&pdf[abs..digits_end])
                .expect("numeric")
                .parse()
                .expect("length");
            let stream_kw = find(&pdf[digits_end..], b"stream\n").expect("stream keyword")
                + digits_end
                + b"stream\n".len();
            assert_eq!(
                pdf[stream_kw + declared],
                b'\n',
                "stream payload of {declared} byte(s) not followed by EOL"
            );
            assert!(
                pdf[stream_kw + declared + 1..].starts_with(b"endstream"),
                "payload not immediately before endstream"
            );
            cursor = digits_end;
        }
        assert!(cursor > 0, "the document declares at least one stream");
    }

    /// `slice.windows(len).position(|w| w == needle)` for byte needles.
    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn every_xref_offset_points_at_its_object() {
        let pages: Vec<PdfPage> = (0..3)
            .map(|i| PdfPage {
                width: 2,
                height: 1,
                rgba: vec![i as u8, 0, 0, 255, 0, 0, 0, 0],
            })
            .collect();
        let pdf = assemble_pdf(&pages);

        let xref = find(&pdf, b"xref\n0 ").expect("xref table");
        let lines = split_lines(&pdf[xref..]);
        for line in lines.iter().skip(2) {
            if !line.ends_with(b"n") {
                continue;
            }
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
    }

    /// Line-splitting over raw bytes (the document is partly binary).
    fn split_lines(data: &[u8]) -> Vec<&[u8]> {
        let mut out = Vec::new();
        let mut start = 0usize;
        for (i, b) in data.iter().enumerate() {
            if *b == b'\n' {
                out.push(&data[start..i]);
                start = i + 1;
            }
        }
        if start < data.len() {
            out.push(&data[start..]);
        }
        out
    }

    #[test]
    fn builds_are_byte_identical() {
        let a = assemble_pdf(&[two_pixel_page(), two_pixel_page()]);
        let b = assemble_pdf(&[two_pixel_page(), two_pixel_page()]);
        assert_eq!(a, b);
    }

    #[test]
    fn page_count_is_in_the_pages_tree() {
        let pdf = assemble_pdf(&[two_pixel_page(), two_pixel_page(), two_pixel_page()]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 3"), "{text}");
        assert!(text.contains("/Kids [3 0 R 6 0 R 9 0 R]"), "{text}");
    }
}
