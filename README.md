# deckr

**English** · [简体中文](#简体中文)

**Read, write and account for PowerPoint decks — without PowerPoint, without LibreOffice, without Python.**

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

$ deckr check board-deck.pptx          # what survives a full read → write → read?
board-deck.pptx
  slides:     23 / 23
  titles:     23 / 23 identical
  paragraphs: 96 / 96
  placed loose: none
  unplaceable blocks: none

$ deckr convert board-deck.pptx -o board-deck.md
$ deckr build board-deck.md -o board-deck.pptx
wrote board-deck.pptx
```

---

## Why this exists

Every existing PPTX library is half of a tool.

| | reads | writes | keeps your theme | measures its own loss |
|---|---|---|---|---|
| `python-pptx` | yes | yes | no — you get question marks in the outline pane | no |
| `pptx2md` / `markitdown` | yes | no | — | no |
| LLM "generate me a PPT" | no | yes | it emits raw XML and prays | no |
| **deckr** | **yes** | **yes** | **by construction** | **yes, every time** |

The interesting failure is `python-pptx`'s second column. Its write API is
`slide.shapes.add_textbox(left, top, width, height)`. That is a *drawing* API: it
puts a box at coordinates and asks you to restyle it. Do that and every slide you
generate loses its connection to the master — which is precisely why
AI-generated decks look identical (default template, blue title on white) no
matter how good the prose is.

deckr takes the opposite position: **you may not author geometry, you may only
declare meaning.**

## The core idea: Deck IR

Every conversion passes through one semantic intermediate representation.

```
.pptx ──parse──▶ Deck IR ──▶ Markdown
                (JSON)   ├──▶ JSON
                         └──▶ .pptx
```

A `Deck` is slides; a slide is `Block`s; every block carries a `Role`:

```json
{
  "index": 1,
  "blocks": [
    { "role": "title",  "content": { "text":  { "paragraphs": [ … ] } } },
    { "role": "body",   "content": { "text":  { "paragraphs": [ … ] } } },
    { "role": "table",  "content": { "table": { "rows": [ … ] } } },
    { "role": "chart",  "content": { "chart": { "caption": null, "blob": { "parts": [ … ] } } } }
  ]
}
```

`Role` mirrors `p:ph/@type` from PresentationML. It is the load-bearing decision
in the whole design: because content is forced to say *what it is*, writing it
back means re-binding it to the master placeholder of the same role instead of
inventing a textbox. Theme, fonts, bullet glyphs and position are the master's
business, not the caller's. A tool that works this way produces decks that look
like someone made them in PowerPoint — because structurally, they were.

Three things fall out of having a real IR instead of string munging:

1. **Loss can be measured.** Round-trip a deck and compare the IR against the original. `deckr check` does this on every run and refuses to round a number down.
2. **Nothing disappears quietly.** A block that cannot bind to a placeholder is either placed loose and reported, or reported as unplaceable. There is no fourth option where it simply vanishes.
3. **Diff becomes possible.** `deckr diff old.pptx new.pptx` compares two *decks*, not two zips — "slide 7 title changed, one bullet added, table cell 3 now 4.1%" rather than "binary files differ".

## Status

| command | state | what it does |
|---|---|---|
| `deckr inspect` | done | structure, per-slide titles, role histogram, JSON IR |
| `deckr convert` | done | `.pptx` → Markdown or JSON |
| `deckr build` | done | Markdown or IR → `.pptx`, bound to the master by role |
| `deckr check` | done | round-trip a file and report exactly what did not survive |
| `deckr render` | 0.3 | slide → SVG, or `--png` for real bitmaps (resvg, pure Rust) |
| `deckr diff` | 0.4 | semantic diff between two decks |

Already handled: titles and free-form text boxes, nested bullet levels, soft
line breaks, tables, pictures (with alt text *and* their media bytes), charts
(captured verbatim, embedded workbook included), empty placeholders, page
furniture, per-run formatting (bold, italic, underline, strike, size, colour,
hyperlinks), **and slide order as the author intended it** rather than as
filenames sort it.

Still missing, deliberately: decoding chart *numbers* into editable data,
SmartArt (diagram) round-tripping, and reusing your own `.potx` instead of the
built-in theme.

## The loss report

This is the feature no other converter can offer, because none of them have
something in the middle to compare against.

```console
$ deckr check tests/fixtures/sample.pptx
tests/fixtures/sample.pptx
  slides:     2 / 2
  titles:     2 / 2 identical
  paragraphs: 8 / 8
  placed loose: 1 block(s) kept without master styling
    slide 1: 'freeform' has no placeholder on layout 'Title Slide' — placed loose,
             so it no longer follows the template
  unplaceable blocks: none
```

(The sample ships a real picture and a real chart; both round-trip, so nothing
is tallied. A diagram, or a chart deckr cannot decode, would be listed here
rather than faked as a blank frame.)

Three separate verdicts, because "did everything survive" and "did everything
stay bound to the master" are different questions:

- **paragraphs 8 / 8** — every paragraph reachable through a text role came back. This is the number that matters.
- **placed loose: 1** — a free-form text box had no placeholder of its type on the chosen layout. The words are still in the file; they just no longer inherit the master's styling. Reported, not hidden.
- **unplaceable: none** — on this fixture the chart and picture both round-trip, so nothing is tallied here. On a deck with a diagram or a chart deckr cannot decode, the offending blocks would be listed instead of being faked as a blank frame.

Getting from 6/8 paragraphs to 8/8 on this fixture took two fixes that unit
tests could not have found: a space was being eaten between adjacent text runs,
and a slide with a subtitle plus a chart was being given the wrong layout, which
dropped its caption. Both came from running against a real file.

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

# And put it back. Same theme, no coordinates involved.
$ deckr build deck.md -o deck.pptx
$ deckr build deck.json -o deck.pptx

# How much of the original survived?
$ deckr check deck.pptx

# Actually look at it — one SVG per slide plus a gallery page.
$ deckr render deck.pptx
rendered 23 slide(s) to deckr_render
  open deckr_render/index.html to preview

# ...or get real PNG bitmaps (one resvg raster per slide) for docs/email.
$ deckr render deck.pptx --png
rendered 23 slide(s) to deckr_render
  rasterised 23 slide(s) to PNG
  open deckr_render/index.html to preview
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

`deckr build` turns that straight back into a `.pptx`. Note what did *not* show
up in the Markdown: the slide-number field and the empty subtitle placeholder.
Page furniture is regenerated by the theme on rebuild, so carrying it through
would bake a duplicate into the file.

## Verification

No PowerPoint and no LibreOffice is involved anywhere in CI, so "it opens" has
to be proved structurally. Two layers:

- 59 tests — 51 unit, 7 integration against the generated fixture, 1 doctest.
- `tests/fixtures/validate_pptx.py`, a standard-library-only checker that treats every generated `.pptx` as an OPC package and asserts all seven properties a consumer actually relies on: every XML part parses, every declared part exists, every part is declared, every internal relationship resolves, every `r:id` referenced in XML is defined, the presentation's slide list resolves, and shape ids are unique within each slide.

It lives outside the Rust tests on purpose. When it fails, the bug is in our
understanding of OPC, not in our agreeing with ourselves. It earned its place
immediately by finding two real defects: a missing namespace distinction between
package relationships and `r:` references, and a fixture that named chart and
image relationships it never declared.

Builds are also **deterministic**: every part shares a fixed ZIP timestamp, so
two builds of the same deck are byte-identical and therefore diffable.

## Library

```rust
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let deck = deckr::read_pptx(Path::new("deck.pptx"))?;

    println!("{} slides", deck.len());
    for (i, title) in deck.outline().iter().enumerate() {
        println!("{}. {}", i + 1, title.as_deref().unwrap_or("(untitled)"));
    }

    std::fs::write("deck.md", deckr::markdown::to_markdown(&deck))?;

    let report = deckr::write_pptx_file(&deck, Path::new("rebuilt.pptx"))?;
    println!(
        "{} block(s) written, {} lost, {} placed loose",
        report.blocks_written,
        report.skipped_count(),
        report.relocated.len()
    );
    Ok(())
}
```

The `Role` of each block is what you branch on; see
[`docs/DESIGN.md`](docs/DESIGN.md) for the reasoning and the roadmap.

deckr keeps the file path attached to its own I/O errors rather than offering a
blanket `From<std::io::Error>`, so a caller that also touches the filesystem
wants `Box<dyn Error>` — as above. This snippet lives at
[`examples/roundtrip.rs`](examples/roundtrip.rs) and is compiled by CI, so it
cannot drift from this page.

## Why Rust

Because "parse this untrusted office file" and "memory safety" belong together,
and because a single static binary that vendors no Python runtime is something
you can drop into a server, a CI job or a WASM sandbox without an argument. The
same ecosystem has already rewritten consolidators in this space (`pdf_oxide`,
`extractous`, `franken_ocr`) — slides were the missing half.

Five dependencies, no `windows-sys`, no `openssl-src`: `clap`, `quick-xml`,
`serde`, `serde_json`, `thiserror` and `zip`, the last with
`default-features = false` to keep Deflate without dragging 25 transitive crates
along with it.

## Contributing

Bug reports about decks that do not parse are the most valuable thing you can
give us right now. Attach the file if you can (or a reduced version of it) — see
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).

---

<a id="简体中文"></a>

# deckr

[English](#deckr) · **简体中文**

**读取、写回、并交代清楚每一页 PowerPoint —— 不需要装 PowerPoint，不需要 LibreOffice，也不需要 Python。**

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

$ deckr check board-deck.pptx        # 完整读 → 写 → 读，丢了多少？
board-deck.pptx
  slides:     23 / 23
  titles:     23 / 23 identical
  paragraphs: 96 / 96
  placed loose: none
  unplaceable blocks: none

$ deckr convert board-deck.pptx -o board-deck.md
$ deckr build board-deck.md -o board-deck.pptx
wrote board-deck.pptx
```

---

## 为什么做这个

现有的 PPTX 库都只做了一半。

| | 能读 | 能写 | 保住你的母版样式 | 能量化自己的损耗 |
|---|---|---|---|---|
| `python-pptx` | 是 | 是 | 不能 —— 大纲栏里只剩一堆问号 | 不能 |
| `pptx2md` / `markitdown` | 是 | 不能 | —— | 不能 |
| LLM「帮我生成 PPT」 | 不能 | 是 | 硬写 XML，然后祈祷 | 不能 |
| **deckr** | **是** | **是** | **结构上天然保住** | **每次都给数字** |

真正耐人寻味的是 `python-pptx` 的第三列。它的写接口长这样：
`slide.shapes.add_textbox(left, top, width, height)`。这是一个**绘图**接口 ——
在坐标处摆一个框，然后请你自己重新设置样式。这么写出来的每一页都与母版失去
联系，这正是为什么 AI 生成的 PPT 长一个样（默认模板、白底蓝标题），无论文案
写得多好。

deckr 的立场相反：**不允许你指定几何位置，只允许你声明语义。**

## 核心设计：Deck IR

所有转换都经过同一个语义中间层。

```
.pptx ──解析──▶ Deck IR ──▶ Markdown
                (JSON)   ├──▶ JSON
                         └──▶ .pptx
```

`Deck` 由若干 `Slide` 组成，`Slide` 由若干 `Block` 组成，而每个 `Block` 都带着
一个 `Role`：

```json
{
  "index": 1,
  "blocks": [
    { "role": "title",  "content": { "text":  { "paragraphs": [ … ] } } },
    { "role": "body",   "content": { "text":  { "paragraphs": [ … ] } } },
    { "role": "table",  "content": { "table": { "rows": [ … ] } } },
    { "role": "chart",  "content": { "chart": { "caption": null, "blob": { "parts": [ … ] } } } }
  ]
}
```

`Role` 对应 PresentationML 里的 `p:ph/@type`。这是整个设计中最吃重的决定：
既然内容被迫说明**自己是什么**，写回时就该把它重新绑到母版上同角色的占位符，
而不是新造一个文本框。主题、字体、项目符号和位置都是母版的职责，不该由调用方
操心。按这个思路做出来的东西，看起来就像人在 PowerPoint 里做的 —— 因为从结构上
说，它就是那么被造出来的。

有了真正的 IR 而不是字符串拼接，会顺带得到三件事：

1. **损耗可度量。** 把 deck 往返一次，拿 IR 和原文对比。`deckr check` 每次运行都做这件事，且不会把这个数字抹成好看的样子。
2. **内容不会悄悄消失。** 绑不上占位符的块，要么「落地并上报」，要么「明确报为无法放置」。不存在第三种情况 —— 不会就这么没了。
3. **diff 成为可能。** `deckr diff old.pptx new.pptx` 比较的是两份 *deck*，不是两个 zip —— 输出是「第 7 页标题变了、加了一条要点、表格第 3 格从 3 改成 4.1%」，而不是「二进制文件不同」。

## 当前状态

| 命令 | 状态 | 作用 |
|---|---|---|
| `deckr inspect` | 已完成 | 结构、每页标题、角色分布、JSON IR |
| `deckr convert` | 已完成 | `.pptx` → Markdown 或 JSON |
| `deckr build` | 已完成 | Markdown 或 IR → `.pptx`，按角色绑回母版 |
| `deckr check` | 已完成 | 往返一个文件，逐项报告哪些内容没能存活 |
| `deckr render` | 0.3 | 页面 → SVG，或加 `--png` 出真实位图（resvg，纯 Rust） |
| `deckr diff` | 0.4 | 两份 deck 的语义 diff |

已支持：标题与自由文本框、多层缩进的项目符号、软换行、表格、图片（含 alt
文本与媒体字节）、图表（原样捕获，含内嵌工作簿）、空占位符、页眉页码等页面装饰、
run 级排版（粗斜体、下划线、删除线、字号、颜色、超链接），**以及作者真正想要的
页序**，而不是文件名排序的页序。

有意暂缺：把图表*数字*解码成可编辑数据、SmartArt（图示）的往返、复用你自己的
`.potx` 而非内置主题。

## 损耗报告

这是别的转换器给不了的功能 —— 因为它们中间没有东西可比。

```console
$ deckr check tests/fixtures/sample.pptx
tests/fixtures/sample.pptx
  slides:     2 / 2
  titles:     2 / 2 identical
  paragraphs: 8 / 8
  placed loose: 1 block(s) kept without master styling
    slide 1: 'freeform' has no placeholder on layout 'Title Slide' — placed loose,
             so it no longer follows the template
  unplaceable blocks: none
```

(The sample ships a real picture and a real chart; both round-trip, so nothing
is tallied. A diagram, or a chart deckr cannot decode, would be listed here
rather than faked as a blank frame.)

三个分开的结论，因为「内容都在吗」和「都还绑着母版吗」是两个不同的问题：

- **paragraphs 8 / 8** —— 所有文本角色的段落都回来了。这是最关键的那个数字。
- **placed loose: 1** —— 一个自由文本框在选中的版式上找不到同类占位符。文字还在文件里，只是不再继承母版样式。明说了，没藏着。
- **unplaceable: none** —— 在这个样例里，图表和图片都能往返，所以这里不计入任何块。若某页有图示（SmartArt）或 deckr 无法解码的图表，相关块会列在这里，而不是被伪造成一个空白框。

在这个 fixture 上把段落数从 6/8 提到 8/8，靠的是两个单元测试根本发现不了的
bugfix：相邻 run 之间的空格被吞掉了；以及「标题 + 副标题 + 图表」的页面被判成了
错误的版式，导致副标题被丢掉。两个问题都是跑真实文件跑出来的。

## 安装

```sh
git clone https://github.com/yuewang2026/NewProject
cd NewProject
cargo build --release
# target/release/deckr
```

还没发布到 crates.io —— 在 issue 里说一声，我们就发 0.1.0。

## 命令行

```console
# 里面有什么？
$ deckr inspect deck.pptx
$ deckr inspect deck.pptx --roles        # 各类 block 的数量分布
$ deckr inspect deck.pptx --json         # 完整的 Deck IR

# 把内容取出来。
$ deckr convert deck.pptx                # Markdown 打到标准输出
$ deckr convert deck.pptx -o deck.md
$ deckr convert deck.pptx --to json -o deck.json

# 再装回去。主题不变，全程不需要坐标。
$ deckr build deck.md -o deck.pptx
$ deckr build deck.json -o deck.pptx

# 原稿有多少活着回来了？
$ deckr check deck.pptx

# 真的看一眼 —— 每页一个 SVG，外加一个画廊页。
$ deckr render deck.pptx
rendered 23 slide(s) to deckr_render
  open deckr_render/index.html to preview

# ……或加 --png 拿到真实 PNG 位图（每页一张 resvg 栅格），方便塞进文档/邮件。
$ deckr render deck.pptx --png
rendered 23 slide(s) to deckr_render
  rasterised 23 slide(s) to PNG
  open deckr_render/index.html to preview
```

`deckr convert` 的输出长这样（取自 `tests/fixtures/sample.pptx`）：

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

`deckr build` 能把它原样变回 `.pptx`。注意 Markdown 里**没有**出现的两样东西：
页码字段、空的副标题占位符。页面装饰在重建时由主题重新生成，带过去反而会在文件
里留下重复的一份。

## 怎么验证

CI 里全程没有 PowerPoint，也没有 LibreOffice，所以「打得开」只能靠结构证明。
两层保障：

- 59 个测试 —— 51 个单元测试、7 个针对生成 fixture 的集成测试、1 个文档测试。
- `tests/fixtures/validate_pptx.py`，一个只用标准库的校验器。它把每个生成的 `.pptx` 当作 OPC 包来查，断言七项真正会被消费方依赖的性质：每个 XML part 能解析、声明的 part 都存在、存在的 part 都被声明、每个内部关系都能落地、XML 里引用的每个 `r:id` 都有定义、presentation 的页序能解析、以及每页内 shape id 不重复。

它刻意放在 Rust 测试之外。当它报错时，问题出在我们对 OPC 的理解上，而不是在我们
跟自己达成一致上。刚上线就抓到两个真缺陷：把「包级 relationships」和「`r:` 引用」
的命名空间搞混了；以及 fixture 里引用了从未声明的图表与图片关系。

构建同时是**确定性的**：所有 part 共用一个固定的 ZIP 时间戳，因此同一份 deck 的
两次构建字节完全相同，可以直接 diff。

## 作为库使用

```rust
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let deck = deckr::read_pptx(Path::new("deck.pptx"))?;

    println!("{} slides", deck.len());
    for (i, title) in deck.outline().iter().enumerate() {
        println!("{}. {}", i + 1, title.as_deref().unwrap_or("(untitled)"));
    }

    std::fs::write("deck.md", deckr::markdown::to_markdown(&deck))?;

    let report = deckr::write_pptx_file(&deck, Path::new("rebuilt.pptx"))?;
    println!(
        "{} block(s) written, {} lost, {} placed loose",
        report.blocks_written,
        report.skipped_count(),
        report.relocated.len()
    );
    Ok(())
}
```

分支判断的依据是每个块的 `Role`；设计取舍与路线图见
[`docs/DESIGN.md`](docs/DESIGN.md)。

deckr 刻意把文件路径留在自己的 I/O 错误上（而不是提供一揽子的
`From<std::io::Error>`），所以既要调 deckr 又要读写文件的调用方应当用
`Box<dyn Error>` —— 就像上面这样。这段代码同时存在于
[`examples/roundtrip.rs`](examples/roundtrip.rs)，由 CI 编译，因此不会与本页面脱节。

## 为什么是 Rust

因为「解析这个来路不明的 Office 文件」和「内存安全」本就该绑在一起；也因为一个
不需要附带 Python 运行时的静态二进制，可以随手丢进服务器、CI 或 WASM 沙箱而无须
争论。Rust 生态里已经有人把这条路上的其它部分重写了（`pdf_oxide`、`extractous`、
`franken_ocr`）—— 幻灯片是缺掉的那一半。

五个依赖，没有 `windows-sys`，没有 `openssl-src`：`clap`、`quick-xml`、`serde`、
`serde_json`、`thiserror` 和 `zip`；最后一个用 `default-features = false`，只要
Deflate，不必为了压缩格式多拖 25 个传递依赖进来。

## 参与贡献

现阶段最有价值的贡献，是报告**解析失败**的 deck。如果可以，请附上文件（或者抽掉
敏感内容后的精简版）—— 详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
