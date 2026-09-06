# ADR-012: Distributing the CLI for Agents

## Status

Accepted 2026-09-06, sequenced: the decision is made, the work waits on two
prerequisites below. Extends [ADR-007](007-mdbook-panschema-plugin.md), which
settled how the binaries are built and archived, with how they reach a
consumer.

## Context

The shipped skill ([feature 40](../features/40-agent-facing-skill-that-cannot-drift.md))
tells an agent what panschema does. It cannot tell the agent to *run* it
without also asking for a toolchain: the documented install is
`cargo install wasm-pack` followed by `cargo install --git … panschema`.
`panschema/build.rs` bootstraps `wasm-pack` whenever neither the packaged
`viz-bundle/` nor the workspace `panschema-viz/pkg/` is present, which is
exactly the git-install case, so a fresh install compiles wasm-pack and then
panschema's ~330-crate tree. That is minutes of setup before the first
command, and it is why the skill carries a setup section at all.

The comparison that prompted this is firecrawl's `anydoc`: a Rust CLI whose
crates.io entry is library-only, with the command shipped as prebuilt npm
platform packages (`@firecrawl/anydoc-darwin-arm64` and siblings) behind a
`cli.js` shim. `npx -y @firecrawl/anydoc` runs with nothing installed, and
their SKILL.md invokes exactly that — which is why it needs no setup section.

What panschema already has, measured 2026-09-06:

- The release workflow builds and archives both binaries (`panschema`,
  `mdbook-panschema`) for four targets: `x86_64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- The binary is self-contained. The visualization bundle is embedded at
  compile time, so a prebuilt binary needs no companion files.
- Size is not an obstacle: 14 MB stripped for `panschema`, 1.2 MB for
  `mdbook-panschema`.
- Both npm names are free: `panschema` and `@padamson/panschema`.

## Decision

**Adopt the npm-wrapped prebuilt-binary distribution**, so an agent can run
`npx -y panschema …` with nothing installed and the skill can drop its setup
section. The crate remains the canonical artifact; npm carries a convenience
copy of what the release already builds.

**Two prerequisites, in order.** Neither is optional, and both are why this
is sequenced rather than started:

1. **Fill the target matrix.** There is no `aarch64-unknown-linux-gnu` and
   no musl target today. Agents commonly run in arm64 containers and on
   Alpine, so publishing `npx panschema` against the current four targets
   would promise a command that fails on the platforms most likely to try
   it. Adding arm64 Linux improves the GitHub Releases for everyone, not
   just the npm path; musl is a separate judgment about whether Alpine is
   worth a static build.
2. **Publish to crates.io.** panschema is installed from git today. Adding
   a third install story before the canonical one exists would leave the
   README advertising git, crates.io, and npm at once, with no clear answer
   to which is authoritative.

**Then** the wrapper itself: one platform package per target carrying the
prebuilt binaries, a shim package that selects among them, and a publish
step in the release workflow beside the existing `cargo publish`.

## Consequences

- The skill's install section shrinks to one command that needs nothing but
  Node, which is the outcome the whole skill effort is for. An agent that
  reads the skill can act on it in the same turn.
- **npm is a second supply chain**, outside `cargo vet`, `cargo deny`, and
  `cargo audit` — the gates this repo otherwise runs on every push. The
  wrapper's own dependency surface must stay near zero (a shim that selects
  a binary needs no runtime dependencies), and npm publishing needs its own
  trusted-publishing setup rather than a long-lived token.
- The primary agent-facing install gains a Node dependency. `npx` is close
  to ubiquitous where coding agents run, and `cargo install` stays for
  anyone who would rather build, so this adds a path rather than replacing
  one.
- Four artifacts per release become four archives plus five npm packages.
  The release workflow already builds every binary involved; the added step
  publishes, it does not compile.
- Version skew becomes possible between the crate and the npm packages.
  They publish from one workflow run off one tag, which is what keeps them
  together; a separate npm release would break that and should not be added.

## Alternatives considered

- **`cargo binstall`.** Reads the existing GitHub Release archives, so it
  needs no new publishing infrastructure and no compile. Rejected as the
  primary answer because it still requires a Rust toolchain, which is the
  cost this ADR exists to remove; it remains a good path for consumers who
  already have cargo and is worth documenting once the crate is published.
- **crates.io alone.** `cargo install panschema` from a published crate
  skips the wasm-pack bootstrap, since the packaged crate carries
  `viz-bundle/`. It removes one compile, not the toolchain requirement.
  This is prerequisite 2, not a substitute.
- **Doing nothing.** Leaves a skill whose first instruction is to install a
  toolchain and wait. That is the status quo the campaign step was opened
  to evaluate, and the anydoc comparison shows what it costs.
