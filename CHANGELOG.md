# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Nothing has been tagged yet, so everything lives under Unreleased. The two
headings inside it mark the phases, not releases.

## [Unreleased]

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
- PNG/PDF rasterisation is deliberately a later step; the SVG is the vector
  source of truth for now.

### Known limitations

- Your own `.potx` template is not yet reusable; `parts.rs` ships one theme.
- Charts are preserved **verbatim**, not decoded — their numbers are not
  extracted into the IR (that is the later numeric-extraction phase). The chart
  is written back exactly as read, so nothing is lost, but deckr cannot yet edit
  a chart's data.
- Diagrams (SmartArt) are detected and reported as skipped; they are not yet
  written back.
- Rasterising the SVG preview to PNG/PDF (vs the vector preview shipped in 0.3)
  and the semantic `diff` (0.4) are not done yet.
- Notes, animations, speaker notes masters and media beyond pictures are ignored.

[Unreleased]: https://github.com/yuewang2026/NewProject/compare/v0.1.0...HEAD
