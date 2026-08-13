# `panschema.toml` — the consumer manifest

Drives `panschema generate` when run with **no `--schema`**. Discovered
cargo-style by walking up from the current directory.

> Every table is `deny_unknown_fields`. A misspelled key is a **hard parse
> error**, not a silently ignored line.

## The one thing that trips everyone

`[generate.<name>]` only runs if there is a **matching `[schemas.<name>>]`**.
The generate loop iterates the *schemas* table; a `[generate]` block with no
counterpart prints `no [generate.<name>] block; skipping` for the schemas it
does know, produces nothing, and **exits 0**. A silent no-op, not an error.

## A complete, working manifest

This exact example is extracted from this file and executed by panschema's
test suite, so it cannot rot.

```toml
[schemas.demo]
path = "."

[generate.demo]
ttl = "out/demo.ttl"
json_schema = "out/demo.schema.json"
```

Run `panschema generate` from the directory holding this file.

## `[schemas.<name>]` — where a schema comes from

Exactly one of two forms. **`path` and `source` are different fields.**

| Form | Keys | Meaning |
|---|---|---|
| Local package | `path = "./pkg"` | a directory containing `panschema-publish.toml`, resolved relative to the manifest |
| Remote package | `source = "github:owner/repo"` + `version = "1.2.3"` | `version` is required; matches the git tag modulo a leading `v` |

- `source` is the **remote** field and understands `github:` only. There is
  no `path:` protocol — `source = "path:."` fails with "unrecognized source
  protocol." Use the `path` field instead.
- **A repo can generate from its own package**: `path = "."` when
  `panschema-publish.toml` sits at the repo root. This is the normal way to
  emit non-HTML artifacts for your own schema.
- Caveat for that self-reference: `fetch` checksums the main file into
  `panschema.lock`, so `verify` reports drift after every edit to a live
  schema. Keep `fetch`/`verify` for real dependencies; `generate` never
  consults the lockfile.

## `[generate.<name>]` — what to emit

Every key is optional; an absent key means that writer does not run. Paths
are manifest-relative.

| Key | Output |
|---|---|
| `html` | **A directory** — the docs site, plus the viz assets |
| `instances` | Array of LinkML instance-data files (A-boxes). Declaration order drives the in-page selector |
| `html_graph_aspect` | `"W:H"`, default `16:8`. Only meaningful with `html` |
| `html_default_layout` | Layout name; see the formats reference |
| `rust` | Rust structs/enums |
| `rust_time` | Time crate for generated temporal fields: `"chrono"` (default) or `"jiff"`. Wire format (RFC 3339 / ISO 8601 strings) is identical either way; pick the crate the consuming workspace already carries. Only meaningful beside `rust` |
| `postgres` | Postgres DDL — **the key is `postgres`, there is no `sql`** |
| `shacl` | SHACL shapes graph |
| `json_schema` | JSON Schema — **underscore**, though the CLI flag is `--format json-schema` |
| `openapi` | OpenAPI 3.1 `components/schemas` |
| `ttl` | OWL/Turtle |
| `jsonld` | JSON-LD |
| `rdfxml` | RDF/XML |
| `ntriples` | N-Triples |
| `graph-json` | Schema graph wire format — **hyphen** |
| `instance-graph-json` | A-box graph wire format — **hyphen** |
| `migrations` | **A directory** of versioned migration files. Written by `panschema migrate`, *not* by `generate` |

Note the naming is not uniform: `json_schema` and `html_graph_aspect` use
underscores, `graph-json` and `instance-graph-json` use hyphens. With
`deny_unknown_fields`, guessing wrong is a parse error.

`migrations` is the odd one out in a second way. Every other key names an
output regenerated from scratch on each run, so `generate` owns it. A
migration directory is append-only — a runner checksums each file it has
applied and aborts when the bytes change — so `generate` never writes there.
Run `panschema migrate` to add to it.

## `[label_sources]`

Maps a **prefix name** (not a namespace IRI) to a label-source URL,
overriding the built-in map. Used to resolve labels for external groundings.

## `panschema.lock`

Written by `fetch`, checked by `verify`. Records each dependency's resolved
source and a `sha256:` checksum of its main file. `generate` does not read
it.
