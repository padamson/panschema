# ADR-015: The lockfile checksums the whole package

**Status:** Accepted (2026-10-18)

## Context

A `panschema.lock` entry's `checksum` covers the pin's main schema file
and nothing else. What a pin renders and resolves from is wider: the
`imports:` siblings the main file names, the package's
`panschema-publish.toml` (its version, its `[[instances]]`), and the
dataset files that lists. An edited or re-published tag could change any of
those and pass `fetch --check` and the `publish` gate on dependency pages,
both of which compared only the main file (issue #123).

Two ways to widen it were open. One is the tarball's own digest, the
approach Cargo takes with `.crate` files. GitHub generates codeload
tarballs on request, and their bytes are not a stable contract: in January
2023 a change to GitHub's archive compression altered the tarball of
unchanged tags and broke checksums across package managers until GitHub
reverted it. The other is a digest over the extracted
files, which is what Go's `go.sum` records for a module (`dirhash`).

## Decision

Each entry gains `package_checksum`, written by `fetch`, compared by
`fetch --check`, and gated on by `publish`. It is `sha256:` over a summary
of the package directory: one line per file, `<sha256-hex>  <path>\n`,
sorted by path, the path relative to the package root and `/`-separated
whatever the platform. A symlink's line hashes its target, which is not
followed, and a path holding a newline is refused because it could forge a
line. For a `github:` source the package directory is the whole extracted
tag, which is also the boundary `imports:` cannot climb out of.

The main-file `checksum` stays. It still gates every entry, and it gives
the sharper message: `fetch --check` reports the main file when it changed,
and reports the package only when the main file is intact ("a file beside
the main schema changed").

An entry without `package_checksum` (any lockfile written before this) is
read as before. `fetch --check` checks its main file, passes, and notes
that a `fetch` would record a package checksum. `publish` gates it on the
main file alone, since a committed lockfile at a historical tag can never
be re-fetched.

## Consequences

- Any change to a pinned tag's tree is drift, including files that render
  nothing, such as a README. A re-published tag is the event the lock exists
  to catch, so a false alarm on a tag nobody should have moved is the
  intended reading.
- `fetch` does not trust the cache it records from. For a pin the lockfile
  already holds at the same source and version, it compares the cached copy
  first. When it disagrees, or no longer resolves, the tag is downloaded
  again into a scratch directory beside the cache and compared there; only a
  copy that agrees replaces the cached one, under the version's cache lock.
  A copy that still disagrees means the tag was re-published after it was
  locked, which `fetch` refuses, leaving the lockfile and the cache as they
  were. Accepting
  new content for a locked version means removing its entry, the way Cargo
  treats a changed checksum. A pin recorded for the first time, or an entry
  that predates the package checksum, is trusted on first use.
- A lockfile written by this version is refused by earlier versions, whose
  `LockEntry` rejects unknown fields. The release that introduces the field
  is therefore a breaking one (0.5.0), as is the `LockEntry` struct itself,
  which gains a public field.
- The digest is the same across re-compressions and on every platform, so a
  lock written on one machine checks on another, and the format is frozen by
  a test against a value computed by hand. One exception: a tag holding two
  paths that differ only in case extracts as one file on a case-insensitive
  filesystem (macOS, Windows), and its digest there differs from Linux's. A
  path that is not UTF-8 is refused rather than made printable, since two
  such names could print the same.
- The checksum covers what a pin renders from because everything read from
  it is confined to the package. The main file must exist and resolve inside
  it, and its `imports:` may not resolve above it. A dataset is checked when
  a consumer names it, and may not exist yet: the part of its path that
  exists is resolved, symlinks followed, and the whole must stay inside, so a
  `[[instances]]` path that leads out through `../` or a symlink is refused
  at the name.
- `fetch --check` and `publish` now read and hash the whole cached tag tree
  on each run, where they read one file before. The cost grows with the
  size of the dependency's repository, not only its schema.
