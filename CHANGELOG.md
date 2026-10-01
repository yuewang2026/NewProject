# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-10-01

The first release: reading and writing are both real, and every number deckr
reports is measured rather than claimed. Everything below was previously
listed under Unreleased; the phase headings mark the work, not releases.

### Added — the reading half

- `deckr inspect` — per-slide titles and block counts, `--roles` histogram,
  `--json` for the full Deck IR.
- `deckr convert` — `.pptx` → Markdown (`md`) or Deck IR JSON (`json`).
- Deck IR: `Deck`, `Slide`, `Block`, `Role`, `TextContent`, `Paragraph`.
- OOXML reader covering titles, body text with indent levels, soft line breaks,
  tables, pictures (with alt text), charts, diagrams and free-form shapes.
- Slide order resolved from `ppt/presentation.xml` (`sldIdLst` → relationships)
  rather than from filenames.
- Markdown exporter that renders bullet nesting, GitHub-flavoured tables and
  skips page furniture and empty placeholders.

### Added — the writing half

- `deckr build` — Markdown or Deck IR JSON → `.pptx`. Every block binds to the
  master placeholder of its role; no caller ever supplies geometry.
- `deckr check` — round-trip a file through the writer and report four numbers
  separately: slides, identical titles, paragraphs, and blocks that could not be
  placed.
- Per-run text formatting through the whole pipeline: bold, italic, underline,
  strike, size, colour and hyperlinks survive a read → write → read cycle.
  `Paragraph` gained an optional `runs` list; older JSON without it still loads.
- Markdown → Deck IR parser, so `pptx → md → pptx` is a real round trip rather
  than a demo.
- Three built-in layouts (Title Slide, Title and Content, Title Only), a compact
  but schema-complete theme, and an OPC package writer producing every part a
  consumer expects.
- Deterministic builds: fixed ZIP and document timestamps mean two builds of the
  same deck are byte-identical, which is the precondition for `diff`.
- `BuildReport.relocated` — blocks that found no placeholder are written as loose
  shapes and reported, instead of being dropped with a shrug.

### Added — verification

- `tests/fixtures/validate_pptx.py` — standard-library-only OPC validator. Seven
  checks: every XML part parses, every declared part exists, every part is
  declared, every internal relationship resolves, every `r:id` referenced in XML
  is defined, the presentation's slide list resolves, and shape ids are unique
  per slide. Runs against both the fixture and every generated package in CI.
- `examples/roundtrip.rs` — the README's library snippet, compiled by CI so the
  documentation cannot rot.

### Added — media fidelity (0.2)

- **Pictures survive round trips.** `BlockContent::Picture` now carries the raw
  media bytes and MIME type; the reader resolves the `r:embed` relationship to
  the `ppt/media/` part and loads it, and the writer drops the file back into the
  package as a `<p:pic>` with a freshly assigned relationship id.
- **Charts survive round trips, verbatim.** A chart's whole relationship
  subgraph — the chart XML, its `.rels`, and the embedded workbook — is captured
  as opaque bytes (`ChartBlob` / `ChartPart`) and re-emitted untouched. The
  writer remaps the `<c:chart>` `r:id` onto a fresh relationship and re-registers
  the content types, so the rebuilt chart still opens.
- **Both travel through the JSON IR.** Picture `data` is base64-encoded inline;
  the chart `blob` (including the embedded-workbook bytes) is serialised too, so
  `pptx → json → pptx` is lossless for media, not just `pptx → pptx`.
- `deckr check` now reports **zero unplaceable blocks** on a deck that carries a
  real picture and a real chart, which was the headline goal of this phase.

### Fixed

- Spaces between adjacent text runs were being swallowed, so `"Hello & welcome"`
  followed by `" to deckr"` came back as `"Hellowelcometo deckr"`. The
  whitespace collapser had an emptiness guard that made no sense mid-string.
- A slide carrying a subtitle *and* a chart was given the "Title and Content"
  layout, which has no `subTitle` placeholder, and its caption was dropped. The
  layout is now chosen by whether anything actually needs the content slot.
- The fixture named chart and image relationships it never declared, so it was
  not a valid OPC package and was teaching the tests the wrong lesson. It now
  ships real chart and media parts.
- README's Rust example did not compile: `std::fs::write(...)?` cannot convert
  into `Error`, which deliberately keeps the file path attached to its I/O
  errors. The snippet now uses `Box<dyn Error>` and is compiled by CI.

### Added — SVG preview (0.3)

- `deckr render` — turns each slide into a standalone, **dependency-free** SVG
  preview: one `slide_N.svg` per slide plus a gallery `index.html`. Layout is by
  `Role`, like PowerPoint's outline view, so it needs no `resvg` / `cosmic-text`
  / `Typst` and builds anywhere deckr builds.
- Title, body bullets (with level indentation), GitHub-flavoured tables and media
  placeholder boxes (image / chart / diagram, with alt or caption text) all
  render. Output is deterministic — no timestamps, no random ids — which keeps a
  `diff` between two renders meaningful.
- `deckr render --png` — rasterises each slide to a real `slide_N.png` bitmap with
  `resvg` (pure Rust, no Cairo/HarfBuzz/fontconfig), so you get images you can
  drop straight into a document or email. `resvg` is the only renderer dependency
  and the SVG stays the vector source of truth.
- New public API: `rasterise_svg`, `render_slide_png`, `render_deck_pngs` and the
  `RasterError` type, all in the `render` module.

### Added — template reuse (0.3)

- `deckr build --template corp.potx` — build the deck inside **your** PowerPoint
  template. The `.potx`'s theme, slide master, slide layouts and their
  relationship graphs (plus `presProps` / `viewProps` / `tableStyles` when the
  template declares them) are copied verbatim into the output, and every
  generated slide binds to the template's own layout placeholders. Corporate
  colours, fonts and masters come for free; deckr's built-in theme is not
  written at all.
- A `Chrome` trait decouples the writer from the visual scaffolding:
  `DefaultChrome` (deckr's built-in theme, the previous behaviour) and `Template`
  (a loaded `.potx`) share one writer core, so template reuse touches no other
  module. The package is read, never executed: sample slides, notes masters and
  media in the template are deliberately left behind, so the output carries no
  orphaned parts.
- New public API: `Template::load`, `write_pptx_template` (any `Write + Seek`
  sink) and `write_pptx_file_template`; the CLI's `build` command gained
  `--template FILE`.
- Layout choice and placeholder geometry come from the template's own layouts,
  re-parsed at load time — an unbound block still lands loose and is reported
  in `BuildReport.relocated`, exactly as with the default theme.
- Template builds are deterministic (sorted part order, fixed ZIP timestamp) and
  covered by four integration tests: theme inheritance (the fixture's accent1 is
  a deliberate non-Office `C00000`), OPC validity of the merged package,
  placeholder binding plus a read-back round trip, and byte-identical rebuilds.

### Added — semantic diff (0.4)

- `deckr diff old.pptx new.pptx` — compares two **decks**, not two zips. Slides
  are aligned by presentation order, blocks paired by role, and the report
  speaks in edits: "slide 2 title changed, one bullet added, table cell (3,2)
  is now 4.1%", plus added/removed slides named by their titles. Exits 1 when
  differences exist (like `diff(1)`), 0 when the decks are semantically
  identical; `--json` emits the same change list as structured data.
- Paragraph-level granularity for text: changed, added and removed bullets,
  indent-level changes, and a separate `ParagraphFormattingChanged` kind when
  the characters are identical but the character properties (bold, colour,
  links) are not — the formatting fidelity 0.2 earned is diffable too.
- Tables are compared cell by cell, with whole-row added/removed reports and a
  `TableReshaped` verdict when a row's column count changed.
- Pictures and charts are compared as media: replaced picture bytes, changed
  alt text, chart captions, and byte-level chart content changes (charts stay
  opaque until numeric extraction lands).
- Page furniture (`footer` / `datetime` / `slide_number`) is deliberately
  excluded — it regenerates from the master on rebuild and would be noise.
- New public API: `diff_decks`, `diff_files`, `Change`, `DeckDiff` (the change
  list is `Serialize`/`Deserialize`, so it travels through JSON). The IR types
  `BlockContent`, `TextContent`, `Paragraph`, `ChartBlob` and `ChartPart` now
  derive `PartialEq`, which the diff needs and callers may want.

### Added — chart numbers (0.4)

- **Chart numbers are decoded.** `ChartBlob` → `ChartData`: a new `chart`
  module walks the captured chart part's `c:chartSpace` XML and pulls out what
  a human calls "the data" — `ChartKind` (bar/line/pie/area/scatter/radar) and
  one `ChartSeries` per series, each with its name, category labels and values.
  An empty `<c:v/>` decodes to `None` — a gap, never a zero. New public API:
  `ChartData`, `ChartSeries`, `ChartKind`, `decode`, `decode_blob`, and the
  `Block::as_chart_data` accessor.
- `BlockContent::Chart` gained `data: Option<ChartData>` (serialised through
  the JSON IR, gaps as `null`). The blob remains the write-side source of
  truth; `data` is a best-effort reading. A chart's own title now also fills
  the block's `caption` when the slide did not name it.
- **Markdown export carries the numbers.** A chart with decoded data exports
  as its `[chart]` marker plus a GitHub-flavoured table — one row per
  category, one column per series, gaps as empty cells.
- **The diff speaks in numbers.** When both sides decode, chart changes are
  reported per point: `ChartValueChanged` (series × category, `3.5` → `4.1`),
  series added/removed/renamed, category label changes — replacing the old
  byte-level "chart content changed". The byte comparison remains the
  fallback for charts neither side can decode.
- **The render draws the chart.** A chart with decoded numbers renders as a
  real plot — grouped bars, polylines, or a pie from the first numeric
  series — with axis gridlines, category labels and a legend, all
  deterministic. Charts without decodable data keep the honest placeholder.
- The fixture chart now ships two real series (Revenue/Cost over Q1–Q3, with
  a deliberate gap in Cost's Q2), so every consumer above is tested against
  actual numbers in CI.

### Added — PDF assembly (0.4)

- `deckr render --pdf` — assemble every slide into **one PDF**: one page per
  slide, each page the rasterised preview at 1 px = 1 pt, so the 960×540 SVG
  becomes a 13.3×7.5 inch 16:9 page (PowerPoint's own default size). Output is
  a single `slides.pdf` beside the SVG/PNG gallery.
- The PDF writer is hand-rolled and dependency-light rather than a `printpdf`
  import: catalog, pages tree, per-page (page, content stream, image XObject)
  objects and a byte-exact xref table. Image streams are RGBA composited over
  white, packed RGB and flate-compressed — `flate2` and `png` were already in
  the dependency tree through `zip` and `resvg`, so the feature adds zero new
  transitive crates.
- New public API: `render_deck_pdf`, `rasterise_svg_rgba`, `PdfPage`. As with
  everything deckr emits, two builds of the same deck are byte-identical.
- Covered by 6 unit tests (structure, alpha compositing, stream lengths vs
  declared lengths, xref offsets, determinism, page tree) and 4 integration
  tests against the real fixture.

### Added — diagram (SmartArt) round-trip (0.4)

- **Diagrams survive round trips.** A `dgm:relIds` frame's whole subgraph —
  the data model, layout, quick style and colours the slide references, plus
  everything their relationship graph reaches (the pre-rendered drawing and
  its rels) — is captured verbatim (`DiagramBlob` / `DiagramPart`) and
  re-emitted untouched. The writer rebuilds the `dgm:relIds` frame with fresh
  relationship ids, restores every part at its original path (so the data
  part's rels resolve unchanged) and registers the content types. `deckr
  check` on a deck with a diagram now reports it as written, not unplaceable.
- **The diagram's words are decoded.** The data model's text points
  (`dgm:pt > dgm:t`) are extracted in document order into
  `Diagram { texts }` — best-effort reading over the verbatim blob, exactly
  like a chart's numbers. The Markdown export lists them as bullets under the
  `[diagram]` marker; the semantic diff reports a reworded node as a
  `ParagraphChanged` on the `diagram` role, and an unchanged text with changed
  data-model bytes as "diagram structure changed".
- New public API: `DiagramBlob`, `DiagramPart`, and the widened
  `BlockContent::Diagram` (serialised through the JSON IR, blob base64 as
  before). The fixture now ships a real three-node diagram, so all of the
  above is tested against an actual SmartArt in CI.

### Added — re-authoring from edited views (0.4)

- **Edit the decoded view, and the writer re-authors the file.** The writer
  compares the caller's `ChartData` / `Diagram.texts` against what the
  captured blob decodes to; when they have drifted, the affected part is
  rewritten — surgically. Chart: only the `c:tx` / `c:cat` / `c:val` /
  `c:xVal` / `c:yVal` caches (`strCache` / `numCache`, `ptCount`, `pt` entries)
  are regenerated; axes, titles, formatting and the external-data reference
  pass through byte-for-byte, and a series the data dropped is removed from
  the XML. Diagram: only the text points' `dgm:t` subtrees are replaced, in
  document order. An unedited round trip stays byte-identical.
- Known edges, stated plainly: the embedded workbook inside a rewritten chart
  still carries the original values until PowerPoint recalculates it; adding
  brand-new series or diagram points is refused (it would mean inventing
  workbook references and model ids).
- New public API: `chart::rewrite_chart_xml`; the writer-side drift detection
  uses the existing decoders, so what you edit is exactly what is compared.
- Covered by 5 unit tests (fidelity, edited values/categories, dropped
  series, gap representation, title isolation) and 3 integration tests
  (edited chart numbers and diagram text survive a write and reopen; the
  unedited round trip stays verbatim).

### Known limitations

- Charts and diagrams re-author their *caches* from edited views, but the
  embedded workbook inside a rewritten chart still holds the original values
  until PowerPoint recalculates it, and brand-new series/points are not
  synthesised.
- Notes, animations, speaker notes masters and media beyond pictures are ignored.

[0.1.0]: https://github.com/yuewang2026/deckr-pptx-oxide/releases/tag/v0.1.0

## [Unreleased]

Nothing yet — the roadmap lives in [docs/DESIGN.md](docs/DESIGN.md).

[Unreleased]: https://github.com/yuewang2026/deckr-pptx-oxide/compare/v0.1.0...HEAD
