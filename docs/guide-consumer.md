# Schema consumer guide

This guide is for projects that **depend on one or more LinkML
schemas** — you want to fetch them reproducibly, run codegen against
them (HTML docs, in v0.3; more writers later), and pin versions in
a lockfile.

The companion document for **schema authors** (who publish the
schemas you consume) is [guide-producer.md](guide-producer.md).

## The three artifacts

panschema's consumer-side workflow uses three files:

| File | What it does | You write it? |
|---|---|---|
| `panschema.toml` | Declares schema dependencies + per-schema codegen config | Yes (or via `panschema add`) |
| `panschema.lock` | Records resolved versions + content checksums | Written by `panschema fetch`, committed to git |
| `panschema-publish.toml` | Lives in each *schema's* repo; declares its name/version/main file | Authored by the schema author, not you |

Both `panschema.toml` and `panschema.lock` live at your project root
and are committed to git, the same way Cargo's `Cargo.toml` +
`Cargo.lock` are.

## Adding a dependency

The fastest way to add a schema is `panschema add`:

```bash
# Remote schema (tagged GitHub release):
panschema add github:padamson/scimantic-schema@0.1.3

# Local schema (sibling project directory):
panschema add ../my-other-project/schema
```

This:

1. Resolves the package (fetches the tarball for `github:` sources;
   reads the directory for `path:` sources).
2. Reads `panschema-publish.toml` to learn the schema's name.
3. Inserts a `[schemas.<name>]` entry into your `panschema.toml`.
4. Adds a starter `[generate.<name>]` block (suppress with
   `--no-generate-config`).
5. Runs `panschema fetch` to populate the cache + update the
   lockfile.

If you want to install the schema under a different local key:

```bash
panschema add github:padamson/scimantic-schema@0.1.3 --name my-alias
```

## Hand-authored `panschema.toml`

You can also write it by hand. The shape:

```toml
[schemas]
# github: source, pinned to a tagged version
scimantic-schema = { source = "github:padamson/scimantic-schema", version = "0.1.3" }

# path: source, pointing at a sibling directory
local-stuff = { path = "../my-other-project/schema" }

[generate.scimantic-schema]
html = "docs/scimantic/"

[generate.local-stuff]
html = "docs/local/"
```

`[schemas.<name>]` keys are the schema name (must match what
`panschema-publish.toml` declares, unless you used `--name <alias>`
on `add`).

`[generate.<name>]` keys configure per-schema codegen. v0.3 supports
the HTML writer. More writers (Rust types, deterministic TTL, SHACL,
JSON Schema) ship in later releases.

## Source types

### `github:owner/repo`

- Anonymous fetch from `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/v<version>`.
- `version` in your manifest must match the publish file's
  `[schema].version` at the tagged commit (`fetch` enforces this).
- The tag is always `v<version>` (with the `v` prefix).
- No GitHub API calls; doesn't count against the 60/hr anonymous
  rate limit.
- Commit SHA recovered from the tarball's top-level directory
  name and recorded in `panschema.lock`.

### `path:./local-pkg`

- Points at a directory containing `panschema-publish.toml`.
- Resolved relative to the manifest's location.
- No version field on the manifest entry — the version is read from
  the publish file each time.
- Useful for monorepo schemas, in-progress consumer/producer
  iteration, and tests.

Other source protocols (`gitlab:`, `https:`, `pypi:`, `zenodo:`)
are deferred to later releases.

## Commands

### `panschema fetch`

```bash
panschema fetch
```

Resolves every entry under `[schemas]`:

- `github:` sources: downloads the tarball if not already cached;
  reuses the cache when the version is already extracted.
- `path:` sources: re-reads from disk.

Then writes (or updates) `panschema.lock` with one entry per
schema: name, version, source spec, and a SHA-256 checksum of the
main file. (A `revision` field is reserved for future
commit-identifier provenance; currently always `None`.)

Cache lives at `~/.cache/panschema/github/<owner>/<repo>/<version>/`
on Linux (XDG cache dir), `~/Library/Caches/...` on macOS,
`~/AppData/Local/...` on Windows. Shared across all projects on
your machine (cargo-style).

### `panschema verify`

```bash
panschema verify
```

Re-checksums every schema and compares against the lockfile. Fails
loudly if:

- A schema's main-file checksum differs (someone edited the schema
  in your cache, or for path sources, in the source directory).
- A schema's publish-toml-declared version differs from what was
  recorded (the schema author bumped the version and you haven't
  refetched).
- A schema is in `panschema.toml` but missing from `panschema.lock`
  (run `fetch` first).
- A schema is in `panschema.lock` but missing from `panschema.toml`
  (run `fetch` to refresh the lockfile, or restore the manifest
  entry).

Run `panschema verify` in CI to guarantee reproducibility.

### `panschema generate`

```bash
panschema generate
```

Walks `[schemas]`, resolves each entry, and runs the writers
configured under the matching `[generate.<name>]` block.

For an entry like:

```toml
[generate.scimantic-schema]
html = "docs/scimantic/"
```

…this produces HTML documentation under `docs/scimantic/` (paths
are resolved relative to the manifest's location).

### `--input <file>` shorthand

For one-off generation against a raw schema file, the old form
still works:

```bash
panschema generate --input ./schema.yaml --output ./out/ --format html
```

This bypasses the manifest entirely. Useful for ad-hoc exploration
or for projects that don't (yet) want the manifest workflow.

## Manifest discovery

`panschema {generate, fetch, verify, add}` discover the manifest by
walking up from CWD (cargo-style). You can run the commands from
any subdirectory of your project; the manifest at the project root
will be found.

If no manifest is found, the command errors out with a clear
message suggesting `panschema init` (for producer-side scaffolding)
or `panschema add` / hand-authoring (for consumer-side).

## Verifying drift in CI

Standard CI shape:

```yaml
- run: panschema verify
```

Run before any codegen step. If the lockfile drifted from what's
on disk (or in github tags), the build fails fast with a clear
diff.

## Lockfile commit hygiene

`panschema.lock` should be committed to git, the same way
`Cargo.lock` is. Two practical guidelines:

1. **Commit lockfile changes in their own commit** when you bump a
   schema version, separately from the bump itself. Makes review
   easier.
2. **Don't gitignore the lockfile.** Reproducibility depends on it
   being in version control.

## Consumer workflow checklist

Bootstrapping a new project:

1. `cd` into the project root.
2. `panschema add github:owner/repo@version` for each schema
   dependency, or write `panschema.toml` by hand.
3. Edit `[generate.<name>]` blocks to point HTML output where you
   want it.
4. `panschema fetch` (already runs implicitly after `add`, but use
   it again whenever you add or update entries).
5. Commit `panschema.toml` and `panschema.lock`.
6. `panschema generate` to produce docs.

Updating to a new schema version:

1. Edit the `version` field in `[schemas.<name>]` (or remove +
   re-add via `panschema add <spec>@<new-version>`).
2. `panschema fetch` to repopulate the cache and refresh the
   lockfile.
3. `panschema generate` to produce updated docs.
4. Commit `panschema.toml` + `panschema.lock` changes.

In CI (or anywhere reproducibility matters):

1. `panschema verify` first.
2. Then `panschema generate`.
