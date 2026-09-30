# deckr design notes

This is the reasoning behind the code. If you want to change any of it, change
it here first and argue it out in an issue.

Read together with [`src/ooxml.rs`](../src/ooxml.rs) (reading) and
[`src/build.rs`](../src/build.rs) (writing).

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

- **Theming is free.** Slides carry no `<a:solidFill>` and no list styling. The master owns appearance. Change the corporate template and 200 generated decks update with it.
- **Outline pane works.** Content lives in real placeholders, so PowerPoint's outline view, screen readers and any other consumer of `p:ph` see it.
- **You cannot express "put this at x=137".** That is deliberately impossible. If you need pixel control, use a template whose layout already has that placeholder. This is a feature.

The temptation in 0.2 was real: somebody always asks for
`add_freeform(x, y, w, h)`. That reintroduces exactly the pathology we exist to
prevent. The right answer is `add_slide_with_layout(template, "Title and Content")`.

### 1a. What happens when there is no placeholder to bind to

This case took two passes to get right, and the answer is worth writing down
because it is where "IR with principles" meets "actually openable file".

The naive builder drops the block and records a skip. That is defensible until
you run it against a real deck: the sample fixture lost a subtitle this way, and
a dropped caption is indistinguishable from a converter bug.

The current behaviour is a three-way verdict, surfaced separately in
`BuildReport`:

| situation | outcome | reported as |
|---|---|---|
| a placeholder of this role exists | bind to it: full inheritance | `blocks_written` |
| no placeholder of this role exists | write it as a loose shape — the words survive, the master's styling does not | `relocated` |
| picture with captured bytes | written verbatim from the PNG/JPEG data, bound to the picture placeholder (no coordinates — the master owns placement) | `blocks_written` |
| chart with captured `ChartBlob` | written verbatim — the whole relationship-reached subgraph (chart XML + `.rels` + embedded workbook) rounds-trips intact | `blocks_written` |
| diagram / SmartArt | not representable in the IR yet — recorded as skipped, not faked | `skipped` |

One detail in the loose case is load-bearing. A loose shape must carry **no**
`<p:ph>` element, not an empty one: ECMA-376 §19.3.1.25 says a `<p:ph/>` with no
`type` defaults to `body`, so writing "some placeholder" silently re-binds the
shape to the layout's body and restores the inheritance we just told the user was
lost. The report line would have been a lie.

### 1b. Choosing the layout

Which layout a slide gets is derived from what it contains, not configured:

```
any non-chrome block whose role needs a content slot (Body | Object | Table)
    -> "Title and Content"
otherwise
    -> "Title Slide"
```

The predicate matters. An earlier version classified by "does this slide contain
anything beyond chrome?", which sent a slide carrying a subtitle *and* a chart to
"Title and Content" — a layout with no `subTitle` placeholder — and dropped the
caption. Charts and pictures do not occupy the content placeholder, so they must
not influence the choice. Again: found by running against a real file, not by a
unit test.

## 2. Loss accounting, not "good enough"

Every existing converter is a one-way street, and nobody can tell you what you
lost because there is nothing to compare against. With an IR there is:

```
original.pptx ──parse──▶ IR_A ──build──▶ rebuilt.pptx ──parse──▶ IR_B
                          └──── diff(IR_A, IR_B) ────┘
```

`deckr check` does exactly this and prints the numbers, with deliberate
separation between *content survived* (paragraph count) and *binding survived*
(`relocated`). Collapsing those two into one "fidelity percentage" would hide the
interesting half of the answer.

Current figure on the committed fixture: **8/8 paragraphs, 2/2 titles, 2/2
slides, unplaceable: none** — see the README for the exact transcript. The two
blocks that earlier counted as `skipped` (a picture and a chart) now round-trip
verbatim, which is the whole point of the 0.2 media-fidelity work. We publish it
per fixture the way `pdf_oxide` publishes its render pass rate. Any feature that
cannot be measured this way should be treated with suspicion.

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

## 4. Text carries structure, formatting and nothing else

A `Paragraph` is `{ level, text, runs }`. `text` is the flattened string that
most consumers want; `runs` is optional and carries per-run bold, italic,
underline, strike, size, colour and hyperlink. Every field is `Option` meaning
*inherit*, never *off* — `Some(false)` and `None` are different statements about
the same run, and conflating them is how you turn "unstyled" into "explicitly not
bold" and break theme updates.

Two things bit us and are worth remembering:

- Entity references arrive as three separate events — `Text("Rock ")`,
  `GeneralRef("amp")`, `Text(" Roll")` — and `trim_text(true)` trims the
  fragments either side, so a deck titled "Rock & Roll" comes back as "Rock Roll". Solved by disabling trim and gating on `<a:t>`.
- The leading-space guard in `collapse_spaces` (`!out.is_empty()`) ate the space
  *between* two adjacent runs. `"Hello & welcome"` + `" to deckr"` became
  `"Hellowelcometo deckr"`. A whitespace-collapse rule that only fires mid-string
  needs no emptiness guard at all.

Hyperlinks resolve through the slide's own `.rels`, and only relationships with
`TargetMode="External"` are eligible. Without that filter, internal part paths
like `../media/image1.png` leak into the Markdown as link targets.

## 5. Determinism is a feature, not hygiene

Every part in a generated package shares one fixed ZIP timestamp (1980-01-01),
and docProps carry a frozen timestamp too. Two builds of the same deck are
therefore **byte-identical**.

This is what makes `deckr diff old.pptx new.pptx` meaningful rather than merely
possible: if the writer inserted `DateTime::now()`, every diff would report all
-slides as changed. The cost is a slightly odd timestamp on files we generate;
the benefit is that the diff command has something to say.

## 6. Verification without installing Office

Neither PowerPoint nor LibreOffice appears in CI, so "it opens" must be proved
structurally. Two layers, kept deliberately separate:

- **Rust tests** prove deckr agrees with itself: 51 unit, 7 integration against the generated fixture (2 of them cover picture + chart survival), 1 doctest.
- **`tests/fixtures/validate_pptx.py`** treats a generated package as an OPC package and asserts seven properties consumers actually depend on: every XML part parses, every declared part exists, every part is declared, every internal relationship resolves, every `r:id` referenced in XML is defined, the presentation's slide list resolves, and shape ids are unique within a slide.

The second layer is Python and lives outside the Rust tests on purpose. When it
fails, the bug is in our understanding of OPC, not in our agreeing with
ourselves. It repaid itself immediately: it caught the fixture naming chart and
image relationships it never declared, and it caught our own validator
conflating the package relationships namespace
(`…/package/2006/relationships`) with the `r:` prefix namespace
(`…/officeDocument/2006/relationships`) — two different strings that look like
the same idea.

The fixture generator is itself validated by the validator. A fixture that lies
about how real packages hang together would teach every other test the wrong
lesson.

## 7. Charts survive verbatim, but their numbers stay encoded

A chart in a PPTX is a `graphicFrame` pointing at an embedded workbook. The
*numbers* live in `ppt/charts/chartN.xml` and its embedded spreadsheet, not in
the picture people see. That makes a chart a small OPC package of its own: the
chart XML, its `_rels/chartN.xml.rels`, and every part that relationship graph
reaches — most often `ppt/embeddings/workbookN.xlsx`.

We do **not** decode any of it. `ChartBlob` carries the *whole* relationship-
reached subgraph as opaque bytes, and the writer remaps the `<c:chart>` r:id,
re-registers content types, and streams the bytes back unchanged. The result is
a chart that reopens in PowerPoint intact and is reported as `blocks_written`,
not `skipped` — so `deckr check` on a real, chart-bearing deck reads
`unplaceable blocks: none`. The enhanced fixture carries exactly such a subgraph
(chart1.xml + its `.rels` + an embedded workbook), and the integration tests
assert byte-identity of all three parts through both `pptx → json → pptx` and
`pptx → pptx`.

What stays deferred to 0.4 is *semantic* access to the numbers: turning
`ChartBlob` into `ChartData` (series × categories → values) is what eventually
enables real numbers in the Markdown export and vector redraw in `render`. Until
then, a chart is a faithful but opaque block: an honest, byte-exact copy, not an
empty frame masquerading as a render.

## 8. What we deliberately do not do (yet)

| Thing | Why not | When |
|---|---|---|
| Reuse your `.potx` template | `parts.rs` ships one theme; real picking/extraction needs a layout inventory | 0.3 |
| Render to SVG (vector preview) | **Shipped in 0.3** — `deckr render` is pure-SVG, no native deps | 0.3 |
| Rasterise to PNG/PDF | Needs `resvg`/`cosmic-text` or an external renderer; the SVG is the source of truth | later |
| Read chart numbers | Capture ships in 0.2; decoding the `ChartBlob` into `ChartData` is a separate pass | 0.4 |
| Semantic diff between decks | Needs both halves first; they now exist | 0.4 |
| Notes, animations, audio/video | Real, but lower value per line of code now that picture + chart media round-trip | later |

## Crate layout (today)

```
deckr
├── src
│   ├── ir.rs        Deck / Slide / Block / Role / Run — the contract, nothing else
│   ├── ooxml.rs     .pptx package -> Deck IR (single pass, streaming)
│   ├── md.rs        Markdown -> Deck IR
│   ├── markdown.rs  Deck IR -> Markdown, outline, role histogram
│   ├── parts.rs     OPC scaffolding: content types, rels, master, layouts, theme
│   ├── build.rs     Deck IR -> .pptx, plus BuildReport
│   ├── error.rs     one error type for every stage
│   └── bin/deckr.rs CLI
├── examples/roundtrip.rs   the README snippet, compiled so docs cannot rot
└── tests/fixtures   sample.pptx generator + OPC validator (no binary in git)
```

`ir.rs` must stay free of OOXML concepts. The moment `p:ph` leaks into it, every
downstream consumer stops being format-agnostic. When "render" lands with real
weight this splits into a workspace (`deckr-ooxml`, `deckr-core`,
`deckr-render`), but not before — a workspace of one-line crates is overhead, not
architecture.

## Testing doctrine

- Every parser branch has a unit test with hand-written slide XML. No test needs a real file to exercise a rule.
- The integration suite needs one real package. Rather than commit a binary blob, `tests/fixtures/make_sample_pptx.py` generates it, and the fixture's `sldIdLst` is deliberately *reversed* relative to filenames so that a regression in order handling fails loudly.
- Anything in the README that looks like code compiles. The library example lives in `examples/roundtrip.rs` for exactly this reason — it was wrong the first time, and nothing caught it until CI did.
- Two builds of one deck produce identical bytes. There is a test for this, because it is the precondition for `diff`.
