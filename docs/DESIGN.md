# deckr design notes

This is the reasoning behind the code. If you want to change any of it, change
it here first and argue it out in an issue.

Read together with the [Phase 0 source](../src/ooxml.rs).

## 1. Placeholder philosophy — the one decision everything else follows from

Every other library gives you a *drawing* API. `python-pptx`:

```python
slide.shapes.add_textbox(Inches(1), Inches(1), Inches(8), Inches(1))
```

You supply coordinates; you also inherit responsibility for fonts, colour,
alignment and every future restyle. Worse, the new shape is not bound to any
placeholder, so PowerPoint's outline pane does not show it, and later edits to
the master slide do not reach it.

deckr refuses to let you place things. `BlockContent` has no coordinates in it at
all. Instead every block carries a `Role`, and the builder's job is to find the
matching placeholder on the slide's layout:

```
Role::Title  ->  layout placeholder with p:ph/@type = "title"
Role::Body   ->  layout placeholder with p:ph/@type = "body" (or untyped @idx)
Role::Table  ->  graphicFrame containing a:tbl
```

Consequences worth understanding before proposing changes:

- **Theming is free.** We never write a single `<a:solidFill>`. The master owns appearance. Change the corporate template and 200 generated decks update with it.
- **Outline pane works.** Content lives in real placeholders, so PowerPoint's outline view, screen readers and any other consumer of `p:ph` see it.
- **You cannot express "put this at x=137".** That is deliberately impossible. If you need pixel control, use a template whose layout already has that placeholder. This is a feature.

The trap we must avoid in 0.2: it is tempting to add `add_freeform(x, y, w, h)`
because someone asks for it. That reintroduces exactly the pathology we are
built to prevent. The right answer is `add_slide_with_layout(template, "Title and Content")`.

## 2. Loss accounting, not "good enough"

Every existing converter is a one-way street, and nobody can tell you what you
lost because there is nothing to compare against. With an IR there is:

```
original.pptx ──parse──▶ IR_A ──build──▶ rebuilt.pptx ──parse──▶ IR_B
                          └──── diff(IR_A, IR_B) ────┘
```

We will publish that number per fixture file, the way `pdf_oxide` publishes its
render pass rate. Any feature that cannot be measured this way should be treated
with suspicion.

## 3. Slide order comes from `presentation.xml`, never from filenames

Sorting `ppt/slides/slideN.xml` numerically is wrong. After a user drags a slide
to the front in PowerPoint, `slide14.xml` can be what the audience sees first.
The authoritative order is `<p:sldIdLst>` in `ppt/presentation.xml`, whose
`r:id`s resolve through `ppt/_rels/presentation.xml.rels`.

Two traps here, both of which we hit while building this:

- `<p:sldId id="257" r:id="rId2"/>` has **two** attributes whose local name is
  `id`. Only the namespaced one is a relationship id. Taking the first match
  silently degrades us to filename order — see `rel_id()`.
- Relationship targets are relative to `ppt/`, not to the package root, and may
  contain `..`. See `resolve_part()`.

Filename scanning is retained as a fallback only, for files where
`presentation.xml` is missing or damaged.

## 4. Text carries structure, not just characters

`Paragraph { level, text }` keeps the DrawingML indent level, so `- a / - b` and
the nesting under it survive. `<a:br/>` stays a newline. Entity references are
resolved — this sounds trivial until a deck titled "Rock & Roll" comes back as
"Rock Roll", because the XML reader hands you `Text("Rock ")`, `GeneralRef("amp")`,
`Text(" Roll")` as three separate events and trims the fragments either side.

Currently dropped: per-run bold/italic/colour/hyperlink markup. That is the
single biggest loss in 0.1 and the first thing 0.2 must fix by adding `runs` to
`Paragraph` while keeping `text` as a fast path for plain text.

## 5. Charts are a different problem and get their own phase

A chart in a PPTX is a `graphicFrame` pointing at an embedded workbook. The
*numbers* live in `ppt/charts/chartN.xml` and its embedded spreadsheet, not in
the picture people see. That is why `BlockContent::Chart { caption: None }` exists
and currently holds nothing: recording "there is a chart here" is already more
than any Markdown converter does, and restoring `ChartData` (series × categories
→ values) is what enables both vector redraw in `render` and real numbers in the
Markdown export.

## 6. What Phase 0 deliberately does not do

|Thing | Why not | When |
|---|---|---|
| Write `.pptx` | Placeholder resolution needs the layout/master graph first | 0.2 |
| Render to PDF/PNG | Borrowing Typst first; a native `cosmic-text + resvg` backend later | 0.3 |
| Read chart numbers | Needs chart XML + embedded workbook | 0.4 |
| Notes, animations, media | Real, but low value per line of code | later |
| Per-run text styling | `Paragraph` needs a `runs` field first | 0.2 |

## Crate layout (today)

```
deckr
├── src
│   ├── ir.rs        Deck / Slide / Block / Role — the contract, nothing else
│   ├── ooxml.rs     .pptx package -> Deck IR (single pass, streaming)
│   ├── markdown.rs  Deck IR -> Markdown, outline, role histogram
│   ├── error.rs     one error type for every stage
│   └── bin/deckr.rs CLI
└── tests/fixtures   sample.pptx generator (no binary in git)
```

`ir.rs` must stay free of OOXML concepts. The moment `p:ph` leaks into it, every
downstream consumer stops being format-agnostic. When "write" and "render" land
with real weight this splits into a workspace (`deckr-ooxml`, `deckr-core`,
`deckr-render`), but not before — a workspace of one-line crates is overhead, not
architecture.

## Testing doctrine

- Every parser branch has a unit test with hand-written slide XML. No test needs
  a real file to exercise a rule.
- The integration suite needs one real package. Rather than commit a binary blob,
  `tests/fixtures/make_sample_pptx.py` generates it, and the fixture's
  `sldIdLst` is deliberately *reversed* relative to filenames so that a
  regression in order handling fails loudly.
