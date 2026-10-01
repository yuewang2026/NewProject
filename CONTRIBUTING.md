# Contributing

Thanks for wanting to help.

## The most useful contribution right now

**Decks that fail.** `deckr` is a parser for a format with twenty years of
accumulated weirdness. The single highest-value thing you can do is run it
against a real deck and file an issue when something is wrong:

```sh
deckr inspect my-deck.pptx        # what did it see?
deckr convert my-deck.pptx        # what text survived?
```

If you can attach the file — or a reduced version that still reproduces the
problem — do. If you cannot (confidential), attach the relevant slide XML:
unzip the `.pptx` and send `ppt/slides/slideN.xml`.

Please say what you expected and what you got. "Slide 4's table comes out empty"
is actionable; "it's broken" is not.

## Development

```sh
cargo build                                   # needs Rust 1.85+ (edition 2024)
cargo test                                    # unit + integration + doctests

python tests/fixtures/make_sample_pptx.py tests/fixtures/sample.pptx
cargo test --test fixture                     # end-to-end against a real package

cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

CI runs all of the above on Linux, macOS and Windows.

## Ground rules

1. **New behaviour gets a test.** For parser rules that means a hand-written
   slide XML snippet in `src/ooxml.rs`'s test module — no binary fixture needed.
2. **Keep `ir.rs` free of OOXML concepts.** No `p:ph`, no element names, no
   coordinate types. It is the contract, and it has to stay format-agnostic.
3. **Do not add geometry APIs.** If you find yourself wanting
   `add_freeform(x, y, w, h)`, read [docs/DESIGN.md](docs/DESIGN.md) §1 first.
   The whole point is that callers declare meaning, not position.
4. **Explain non-obvious constants.** If you write `4_572_000` (EMU per inch,
   to pick one), name it and cite the spec section.

## Adding support for a new OOXML feature

1. Unit test with inline XML covering the happy path and the empty case.
2. Add the variant to `BlockContent` only if existing ones cannot express it —
   every new variant is a burden on every future renderer.
3. Extend `to_markdown` so humans can see it in `convert` output.
4. Note the loss (if any) in `docs/DESIGN.md` §6.

## Commit messages

Imperative mood, one logical change per commit. No "wip". No AI-coauthored
trailers on every line — use them if the contribution is AI-assisted, otherwise
keep it clean.

## Releasing

Releases are cut by hand, and the workflow only does the upload:

1. Bump `version` in `Cargo.toml` (this project follows SemVer).
2. Move the CHANGELOG's `Unreleased` heading to a dated `## [x.y.z] — YYYY-MM-DD`
   and update the link at the bottom of the file.
3. Commit, then tag and push: `git tag v0.1.0 && git push origin main --tags`.
4. Pushing the tag runs `.github/workflows/release.yml`, which re-runs the
   suite, verifies the `.crate` builds, and publishes with
   `CARGO_REGISTRY_TOKEN` (a repository secret).

To publish from a machine instead, `cargo login` once and run
`cargo publish --locked`. Either way the crate must build with `--locked`, so
`Cargo.lock` is committed.
