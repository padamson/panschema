# CLI surface

`panschema` with no subcommand prints help and exits non-zero. There is no
bare no-subcommand form.

| Subcommand | What it does |
|---|---|
| `generate` | Render a schema to an output format. With no `--schema`, discovers `panschema.toml` and generates every manifested schema |
| `validate` | Check a LinkML **instance-data** file against a schema. Exits non-zero listing every violation. *Not* schema-vs-metaschema validation |
| `publish` | Build versioned HTML docs per git ref, per `[publishing]` in `panschema-publish.toml` |
| `serve` | Hot-reload dev server for HTML output |
| `init` | Scaffold a `panschema-publish.toml` |
| `add` | Add a schema dependency to `panschema.toml` and fetch it |
| `fetch` | Resolve every dependency, checksum it, write `panschema.lock` |
| `verify` | Re-checksum and fail on drift from the lockfile |
| `release` | Bump the schema version in `panschema-publish.toml`, optionally commit/tag/push |
| `completions` | Emit a shell completion script |
| `styleguide` | Component preview page (requires the `dev` feature) |

## Flags worth knowing

- `--instances <PATH>` — repeatable. Several are only meaningful for `html`;
  the single-A-box formats reject more than one.
- `--strict` — **narrower than it sounds.** It fails on unmodeled
  constructs, dangling references, and instance-data violations. It does
  *not* promote unprojected-construct, Postgres-skip, or SHACL-skip
  warnings to errors.
- **Pointing at a record in another graph.** In instance data, a
  class-ranged value that is an absolute IRI or a CURIE against a prefix the
  schema declares (`catalog:aws`) is an *external reference*: it emits as an
  IRI in RDF, is exempt from the dangling check, and is listed in a
  cross-graph summary. A **bare** id always means "a record in this file"
  and is still dangling-checked — including one carrying an undeclared
  prefix, which is read as a typo, not as a link outward.
- `--offline` / `--refresh-labels` — control upstream label fetching for
  external groundings. Fail-open: unreachable sources fall back to CURIEs.
- `--no-graph`, `--viz-mode` — HTML only; warn if used with another format.

## Common recipes

    # docs for one schema
    panschema generate --schema schema/my.yaml --output site/

    # docs plus curated instance graphs
    panschema generate --schema schema/my.yaml \
        --instances data/preview.yaml --instances data/full.yaml \
        --output site/

    # every artifact declared in panschema.toml
    panschema generate

    # is this instance data conformant?
    panschema validate --schema schema/my.yaml --data data/full.yaml
