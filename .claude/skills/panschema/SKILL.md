---
name: panschema
description: Use when working with LinkML or OWL schemas — generating schema documentation or a schema graph, converting a schema to RDF/Turtle, JSON Schema, OpenAPI, SHACL shapes, Rust types or Postgres DDL, validating instance data against a schema, wiring a `panschema.toml` manifest, or publishing versioned schema docs. Also use when a repo contains `panschema.toml`, `panschema-publish.toml`, or `panschema.lock`.
---

# panschema

A universal CLI for schema conversion, documentation, and validation —
pandoc for data modeling. One schema in, many artifacts out.

    Input file → Reader → LinkML IR → Writer → Output

Readers cover OWL/Turtle and LinkML YAML. Writers cover HTML docs, the
RDF/OWL family, graph JSON, Rust, Postgres DDL, SHACL, JSON Schema and
OpenAPI. Any reader pairs with any writer.

## Start here

- **Generate something once** — `panschema generate --schema <file>
  --format <fmt> --output <path>`. Formats and their quirks:
  [references/formats.md](references/formats.md).
- **Generate a repo's declared artifacts** — `panschema generate` with no
  `--schema`, driven by `panschema.toml`:
  [references/manifest.md](references/manifest.md).
- **Check instance data** — `panschema validate --schema <s> --data <d>`.
- **Full CLI surface and recipes** — [references/cli.md](references/cli.md).

## Four things that reliably go wrong

Each of these has actually bitten a consumer. Reading these four lines is
worth more than skimming everything else.

1. **`[generate.<name>]` does nothing without a matching
   `[schemas.<name>]`.** The generate loop iterates the *schemas* table, so
   a lone `[generate]` block emits nothing and exits 0 — a silent no-op.

2. **`path` and `source` are different fields.** `source` is for remote
   packages and understands `github:` only; there is no `path:` protocol.
   To generate from the repo's own package, use `path = "."`.

3. **Manifest key names are not uniform**, and every table is
   `deny_unknown_fields`, so a wrong guess is a hard parse error — not a
   skipped line. It is `postgres` (never `sql`), `json_schema` with an
   underscore although the CLI flag is `--format json-schema`, and
   `graph-json` / `instance-graph-json` with hyphens.

4. **`--strict` is narrower than it sounds.** It fails on unmodeled
   constructs, dangling references, and instance-data violations. It does
   *not* fail on unprojected-construct, Postgres-skip, or SHACL-skip
   warnings — those stay warnings.

## Instance data (A-boxes)

To render or validate instance data, the schema needs a class marked
`tree_root: true` whose class-ranged slots hold the records; records need an
`identifier: true` slot. Then `--instances <file>` (repeatable for `html`)
draws an instance graph beneath the docs, folds individuals into the RDF
output, and runs the same conformance check `validate` runs — so nothing
ships violations.

**Whether the root itself becomes an individual is your decision, made by
giving it an identifier.** A `tree_root` class that declares an
`identifier: true` slot emits as an individual like any other record — RDF,
graph node, card — and its collection slots draw references to what it
holds. One that declares none emits nothing, and its scalars surface only as
dataset metadata. So a bare vessel that exists because a file needs a root
stays silent, while a domain root (an enterprise, a study, a tenant) becomes
the anchor another graph can reference. Adding an identifier later changes
the output; that is the intended signal, not a side effect.

## Keeping this skill honest

The manifest example in `references/manifest.md` is **extracted and executed
by panschema's test suite**, and tests assert that every output format, CLI
subcommand, and `[generate]` key the code offers is mentioned in these
references. A doc example that stops working, or a new feature that ships
undocumented, fails CI.

That covers example rot and undocumented features. It does **not** verify
that prose is accurate — if you find a statement here that contradicts the
code, the statement is the bug; fix it in the same change.
