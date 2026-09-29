# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `0.2` — `deckr build`: turn Deck IR (or Markdown) + your template into a real
  `.pptx` with your theme, by re-binding content to master placeholders.
- `0.2` — per-run text formatting (bold, italic, colour, hyperlinks) in the IR.
- `0.3` — `deckr render`: slide → PNG/PDF so output can actually be looked at.
- `0.4` — `deckr diff`: semantic comparison of two decks.
- `0.4` — chart numbers extracted from `ppt/charts`.

## [0.1.0] - unreleased

First public release — the reading half.

### Added

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
- Unit tests using inline slide XML; integration tests against a generated
  fixture whose slide order is deliberately reversed.

### Known limitations

- Text loses per-run formatting (bold/italic/colour/hyperlink); only characters
  and indent levels survive.
- Charts and diagrams are recorded as present, not decoded.
- Notes, animations, speaker notes masters and media are ignored.
- Writing `.pptx` does not exist yet — this release reads only.

[Unreleased]: https://github.com/yuewang2026/NewProject/compare/v0.1.0...HEAD
