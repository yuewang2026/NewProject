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

### Known limitations

- Your own `.potx` template is not yet reusable; `parts.rs` ships one theme.
- Charts and diagrams are detected, not decoded — their numbers are not extracted
  and they are not written back. Reported as skipped, never drawn empty.
- Pictures are detected with their alt text, but their media is not carried
  through a round trip. Same reporting policy.
- `render` (0.3) and `diff` (0.4) do not exist yet.
- Notes, animations, speaker notes masters and media are ignored.

[Unreleased]: https://github.com/yuewang2026/NewProject/compare/v0.1.0...HEAD
