# ADR: mdbook asset-install lives in a dedicated `mdbook-panschema` plugin

## Status
Accepted (2026-06-30)

## Context

panschema generates schema docs and already links them *back* to an mdbook book (`site_root_url`). The reverse link — an mdbook toolbar button pointing *to* the schema docs — has no owner, so consumers hand-roll per-book JS/CSS that drifts with every mdbook release.

The mdbook ecosystem has a settled idiom for shipping such an asset, established by `mdbook-admonish` / `mdbook-listings`: the tool is an `mdbook-<name>` binary whose `install [path]` subcommand (path defaults to `.`) copies a CSS/JS asset into the book and wires it into `book.toml`'s `additional-*`, re-run after upgrades to refresh.

`panschema` is a general schema tool, not an mdbook plugin. Adding mdbook asset-install verbs to it (`panschema install-book-link`) is off-idiom: an mdbook user doesn't expect them there, and the subcommand can't use the idiomatic bare `install`. A separate `mdbook-panschema` preprocessor (embedding rendered schema components inline in a book) is also planned independently.

## Decision

Introduce **`mdbook-panschema`** as a new **workspace member crate** in this repo, producing an `mdbook-panschema` binary.

- Its first capability is **`mdbook-panschema install [dir]`** (`dir` defaults to `.`): copy the book→schema toolbar asset into the mdbook book and wire `book.toml`'s `additional-js`/`additional-css` (idempotent), reading `[book_link]` from `panschema-publish.toml`. Re-run to refresh after an upgrade — the `mdbook-admonish` idiom.
- It takes a **path dependency on the `panschema` library crate**, reusing `[book_link]` parsing and asset bundling rather than duplicating them across a repo boundary.
- Distributed independently via crates.io (`cargo install mdbook-panschema`), versioned with the workspace.
- The planned inline-schema **preprocessor is a later, separate capability of the same binary** — out of scope here. (Precedent: `mdbook-admonish` is both a preprocessor and an `install` command.)

## Consequences

**Positive**
- Fully idiomatic for mdbook users (`mdbook-panschema install`, bare verb, matches `mdbook-admonish`).
- Reuses panschema's config/asset code via an in-workspace path dependency — no cross-repo coupling.
- Gives the planned preprocessor a home and a walking-skeleton first command.
- Keeps the general `panschema` CLI free of mdbook-specific verbs.

**Negative / costs**
- A third workspace member: more build/CI surface, another crate to release and cover with cargo-vet/deny.
- Two binaries in the family (`panschema`, `mdbook-panschema`) — users must know which to install for what.
- If the preprocessor later warrants its own repo, the member must be extracted (a cheap, well-trodden migration).

## Alternatives considered

- **`panschema install-book-link [dir]` subcommand** — ships without a new crate, but off-idiom (panschema isn't an mdbook plugin; non-bare verb). Rejected for idiom.
- **Fold into `panschema publish`** — couples an mdbook-theme asset to versioned-docs orchestration and conflates the book tree with the schema publish output. Rejected.
- **Defer until the preprocessor exists** — blocks a small, wanted asset on a larger effort. Rejected.

## Addendum — 2026-08-13: same binary, one crate

The dedicated *crate* is retired; the dedicated **binary** stays. The
`mdbook-panschema` binary now ships as a second `[[bin]]` of the
`panschema` crate (`src/mdbook.rs` + `src/bin/mdbook_panschema.rs`), so
`cargo install panschema` provides both tools.

What changed the calculus: the separate crate's only dependency on
`panschema` was the `[book_link]` config types, yet it compiled the
entire library — and publishing it to crates.io would have made users
build that stack for a ~500-line installer, while its actual audience
(repos publishing panschema-generated schema docs) installs panschema
anyway. The version-pinned path dependency also created release
lockstep friction. Everything this ADR valued survives the fold: the
idiomatic `mdbook-panschema install` bare verb, the binary name (which
the planned preprocessor role requires — mdbook discovers a
preprocessor by invoking `mdbook-<name>` — and which the fold keeps as
a `[[bin]]`), and the config-type reuse — now zero-cost within one
crate. The two-binaries-one-install shape follows wasm-pack and
friends.

One thing is given up: the `mdbook-panschema` **crate name** on
crates.io. The dedicated crate was never actually published there, and
after the fold it never will be, so nothing stops a third party from
claiming the name. Accepted: every install instruction points at
`cargo install panschema`, and a preprocessor would ship from the same
crate.
