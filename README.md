# deckr

**Read, write and diff PowerPoint decks — without PowerPoint, without LibreOffice, without Python.**

[![CI](https://github.com/yuewang2026/NewProject/actions/workflows/ci.yml/badge.svg)](https://github.com/yuewang2026/NewProject/actions/workflows/ci.yml)
![license](https://img.shields.io/badge/license-MIT-blue.svg)
![rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)

```console
$ deckr inspect board-deck.pptx
board-deck.pptx
  slides: 23
  1. Q3 Board Update  [6 block(s)]
  2. Revenue vs Plan  [4 block(s)]
  ...

$ deckr convert board-deck.pptx -o board-deck.md
wrote board-deck.md
```

---

## Why this exists

Every existing PPTX library is half of a tool.

| | reads | writes | keeps your theme | diffs two versions |
|---|---|---|---|---|
| `python-pptx` | yes | yes | no — you get question marks in the outline pane | no |
| `pptx2md` / `markitdown` | yes | no | — | no |
| LLM "generate me a PPT" | no | yes | it emits raw XML and prays | no |
| **deckr** | **yes** | **read next** | **by construction** | **once both halves land** |

The interesting failure is the second column of `python-pptx`. Its write API is
`slide.shapes.add_textbox(left, top, width, height)`. That is a *drawing* API: it
places a box at coordinates and asks you to restyle it. Do that and every slide
you generate loses the connection to the master — that is precisely why
AI-generated decks look identical (default template, blue title on white) no
matter how good the prose is.

deckr takes the opposite position: **you may not author geometry, you may only
declare meaning.**

## The core idea: Deck IR

Every conversion passes through one semantic intermediate representation:

```
.pptx ──parse──▶ Deck IR ──▶ Markdown
                (JSON)   └──▶ JSON
                         └──▶ .pptx   (0.2)
```

A `Deck` is slides; a slide is `Block`s; and every block carries a `Role`:

```json
{
  "index": 1,
  "blocks": [
    { "role": "title",  "content": { "text": { "paragraphs": [ … ] } } },
    { "role": "body",   "content": { "text": { "paragraphs": [ … ] } } },
    { "role": "table",  "content": { "table": { "rows": [ … ] } } },
    { "role": "chart",  "content": { "chart": { "caption": null } } }
  ]
}
```

`Role` mirrors `p:ph/@type` from PresentationML. It is the load-bearing decision
in the whole design: because content is forced to say *what it is*, writing it
back means re-binding it to the master placeholder of the same role rather than
inventing a new textbox. Theme, fonts, bullet glyphs and position are the
master's business, not the callers'. A tool that can do this produces decks that
look like someone made them in PowerPoint — because structurally, they were.

Three things fall out of having a real IR rather than string munging:

1. **Loss can be measured.** Round-trip a deck, diff the IR against the original, count the damage.
2. **Diff becomes possible.** `deckr diff old.pptx new.pptx` compares two *decks*, not two zips — "slide 7 title changed, one bullet added, table cell 3 now 4.1%" instead of "binary files differ".
3. **AI can see what it made.** An IR is trivially renderable, so a generation loop can look at its own output and fix it. Today's pipeline is generate → ship → hope.

## Status: Phase 0 — the reading half

| command | state | what it does |
|---|---|---|
| `deckr inspect` | done | structure, per-slide titles, role histogram, JSON IR |
| `deckr convert` | done | `.pptx` → Markdown or JSON |
| `deckr build` | 0.2 | Markdown / IR + your template → a real `.pptx` with your theme |
| `deckr render` | 0.3 | slide → PNG/PDF so you can actually look at it |
| `deckr diff` | 0.4 | semantic diff between two decks |

Phase 0 already handles: titles and free-form text boxes, nested bullet levels,
soft line breaks, tables, pictures (with alt text), charts and SmartArt
(detected, numbers still to come), empty placeholders, page furniture, **and
slide order as the author intended it** rather than as filenames sort it.

## Install

```sh
git clone https://github.com/yuewang2026/NewProject
cd NewProject
cargo build --release
# target/release/deckr
```

Not yet published to crates.io — say the word in an issue and we will cut 0.1.0.

## CLI

```console
# What is in here?
$ deckr inspect deck.pptx
$ deckr inspect deck.pptx --roles        # how much of each kind of block
$ deckr inspect deck.pptx --json         # the Deck IR itself

# Get the content out.
$ deckr convert deck.pptx                # Markdown to stdout
$ deckr convert deck.pptx -o deck.md
$ deckr convert deck.pptx --to json -o deck.json
```

`deckr convert` output looks like this (from `tests/fixtures/sample.pptx`):

```markdown
---

## Slide 1 — deckr — a bidirectional deck engine

Read it. Diff it. Build it back.

`[chart]`

![architecture diagram](media://slide1)

---

## Slide 2 — Roadmap & Milestones

- Ship the reader
  - OOXML & Deck IR
    - placeholders stay bound
  - Write the builder

| Phase | Target |
| --- | --- |
| 0.1 read | 2026 Q3 |
| 0.2 write | 2026 Q4 |
```

Note what did *not* show up: the slide-number field, the empty subtitle
placeholder. Page furniture is regenerated by the theme on rebuild, so carrying
it through would bake a duplicate into the file.

## Library

```rust
use std::path::Path;

fn main() -> Result<(), deckr::Error> {
    let deck = deckr::read_pptx(Path::new("deck.pptx"))?;

    println!("{} slides", deck.len());
    for (i, title) in deck.outline().iter().enumerate() {
        println!("{}. {}", i + 1, title.as_deref().unwrap_or("(untitled)"));
    }

    std::fs::write("deck.md", deckr::markdown::to_markdown(&deck))?;
    Ok(())
}
```

The `Role` of each block is what you branch on; see
[`docs/DESIGN.md`](docs/DESIGN.md) for the reasoning and the roadmap.

## Why Rust

Because "parse this untrusted office file" and "memory safety" belong together,
and because a single static binary that vendors no Python runtime is something
you can drop into a server, a CI job or a WASM sandbox without an argument. The
same ecosystem already rewritten consolidators in this space (`pdf_oxide`,
`extractous`, `franken_ocr`) — slides were the missing half.

## Contributing

Bug reports about decks that do not parse are the most valuable thing you can
give us right now. Attach the file if you can (or a reduced version of it) — see
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
