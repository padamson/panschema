# Feature 29: Shared schema load pipeline and writer-family consistency

**Feature:** One shared load pipeline (read → resolve imports → schema-level
diagnostics) used by every command that renders a schema, plus closure of the
writer-family inconsistencies a 2026-07 full-codebase review surfaced — so
that a schema means the same thing to every command and every writer, before
cross-schema `imports:` ([feature 15](15-multi-file-schema-modularity.md)
slices 3–4) multiplies the ways a definition can go missing.

**User Story:** As a schema author splitting a schema across files (and soon
across fetched packages), I want `generate`, `serve`, and `publish` to render
the same resolved schema, and I want a loud diagnostic when a name fails to
resolve — instead of each output format silently degrading in its own way —
so a broken import is one clear warning, not six different silent artifacts.

**Related ADR:** [004 (Reader/Writer architecture)](../adr/004-reader-writer-architecture.md)
— this feature adds the missing load layer above `Reader`.

**Approach:** Vertical Slicing with Outside-In TDD. Findings below were
verified against the running binary during the review, not just read from
code; each slice cites the observed behavior it fixes.

---

## Why Now

A full review of `main` (2026-07-06) found, verified by execution:

- `resolve_imports` runs only inside `generate`; `serve` and `publish` go
  reader → writer directly, so they render **un-merged schemas** and skip
  every generate-time diagnostic. The hot-reload path is exactly where
  authors will iterate on imports.
- An unresolvable range name (what a failed import merge produces) degrades
  **six different silent ways**: generated Rust that does not compile, a
  fabricated `xsd:` datatype in SHACL, phantom IRIs in OWL, a foreign key
  silently becoming a `text` column in Postgres, a dropped edge in the
  graph, a bare string in HTML — with zero warnings on any path.
- The RDF family (`ttl`/`jsonld`/`rdfxml`/`ntriples`) never resolves
  effective slots at all: a schema whose properties are inline
  `attributes:` (the dominant LinkML idiom) emits RDF containing classes
  and **no properties**. Every other writer resolves inheritance and
  attributes.
- Output-path handling differs per writer (some create parent directories,
  some fail with a message that doesn't name the path, HTML treats the
  output as a directory), and several naming/CURIE/type-mapping helpers
  exist in two or three drifted copies.

Cross-schema imports work starts only after slices 1–3 land.

---

## Vertical Slices

### Slice 1: Shared load pipeline used by generate, serve, and publish

**Status:** Complete

**Priority:** Must Have

**User Value:** `serve` and `publish` render the same resolved schema as
`generate`; an import-using schema previews correctly under hot reload.

**Acceptance Criteria:**
- [x] A single load path performs read → import resolution → schema-level diagnostics, and `generate`, `serve` (including hot-reload regeneration), and `publish` all render through it.
- [x] For a schema with local `imports:`, the HTML `serve` renders and the `publish` output contain the imported elements (a test pins each command's output against the merged schema).
- [x] Generate-time diagnostics (unmodeled constructs, unresolved unique-key slots) fire on the `serve` and `publish` paths too. (Writer-projection warnings are format-specific — empty for HTML, which `serve`/`publish` always emit — so they stay at the `generate` site.)

---

### Slice 2: Dangling-reference diagnostic

**Status:** Complete

**Priority:** Must Have

**User Value:** A name that fails to resolve after loading — a slot `range`,
`is_a` parent, mixin, or `inverse` naming no class, enum, type, or known
primitive — produces one clear warning naming the referrer and the missing
name, on every command, instead of six per-format silent degradations.

**Acceptance Criteria:**
- [x] Loading a schema in which a slot's `range` names a class that doesn't exist warns, naming the slot and the missing name; likewise for `is_a`, `mixins`, and `inverse` references.
- [x] The diagnostic runs in the shared load pipeline (slice 1), so `generate`, `serve`, and `publish` all surface it for any output format.
- [x] `generate --strict` turns the warning into a non-zero exit.
- [x] Writers' per-format degradation behavior is unchanged by this slice (the warning makes it visible; changing per-writer fallbacks is separate).

---

### Slice 3: RDF family renders effective slots

**Status:** Complete

**Priority:** Must Have

**User Value:** A schema using inline `attributes:` (or inherited/mixed-in
slots) emits RDF that actually declares its properties; the SHACL shapes and
the OWL ontology describe the same vocabulary.

**Acceptance Criteria:**
- [x] For a class with inline `attributes:`, the TTL/JSON-LD/RDF-XML/N-Triples output declares each attribute as a property (type, label, domain, range), verified through the independent triple-store oracle.
- [x] Properties reached via `is_a` and mixins render on the RDF output consistently with how the HTML and SHACL writers already resolve them.
- [x] Every `sh:path` IRI in the SHACL shapes graph has a corresponding property declaration in the OWL output for the same schema (a test loads both into one store and checks).
- [x] A property defined both as a top-level slot and reachable as an effective slot emits once, not twice.

**Notes:**
- The OWL round-trip tests pass today only because the reference fixture is
  TTL-sourced (slots land top-level); this slice must add attribute-style
  fixtures to the round-trip and oracle suites.

---

### Slice 4: Uniform output-path handling

**Status:** Complete

**Priority:** Should Have

**Acceptance Criteria:**
- [x] `generate --output some/new/dir/out.<ext>` behaves identically for every file-producing format: parent directories are created (or the command fails with an error that names the path) — one behavior, asserted across all registered writers in a single parameterized test.
- [x] The duplicate parent-directory guards in individual writers are replaced by one shared implementation.

---

### Slice 5: Consolidate naming/CURIE/type-mapping helpers

**Status:** Not Started

**Priority:** Should Have

**User Value:** CURIE expansion, prefix maps, and primitive-type aliases
behave identically in every output; a prefix or alias handled in one format
can't silently misrender in another.

**Acceptance Criteria:**
- [ ] One CURIE-expansion implementation (single unknown-prefix and default-prefix behavior) used by the RDF builders and the HTML writer.
- [ ] One prefix-map builder used by the Turtle-emitting writers (with per-writer builtin additions like `sh:`/`xsd:` layered, not forked); the OWL output declares `xsd:` when it emits `xsd:`-typed terms.
- [ ] One primitive-alias table shared by the Rust, Postgres, and XSD type mappings (aliases like `str`/`int`/`bool` map consistently; no fabricated `xsd:` IRIs from the alias fallback).
- [ ] Casing helpers move out of `rust_writer` into a shared module.

---

### Slice 6: Writer-level diagnostics surface — as demand confirms

**Status:** Not Started

**Priority:** Could Have

**User Value:** Per-writer "couldn't project X" reporting (today: Postgres's
skip diagnostics, hardcoded per-format in the CLI) becomes a writer-owned
surface, so new writers and new gap classes don't require coordinated edits
across three files.

**Acceptance Criteria:**
- [ ] Writers report projection gaps through a common interface the CLI renders generically; the format-gated blocks in the CLI dispatch collapse.

---

## Tracked, deliberately not in this feature

- **JSON-LD determinism**: JSON-LD output differs run-to-run (serializer
  ordering; all other formats are byte-stable). Needs a canonicalization
  decision; until then consumers should exclude JSON-LD from
  regenerate-and-diff gates.
- **Manifest coverage**: `[generate.<name>]` drives only `html` and `rust`
  of the nine formats, and per-writer options can't thread through the
  `Writer` trait — the manifest/trait redesign is its own future feature.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: shared load pipeline | Must Have | None | Complete |
| Slice 2: dangling-reference diagnostic | Must Have | Slice 1 | Complete |
| Slice 3: RDF family effective slots | Must Have | None | Complete |
| Slice 4: uniform output paths | Should Have | None | Complete |
| Slice 5: consolidate helpers | Should Have | None | Not Started |
| Slice 6: writer diagnostics surface | Could Have | Slice 2 | Not Started |

Cross-schema imports ([feature 15](15-multi-file-schema-modularity.md)
slices 3–4) depend on slices 1–3.

---

## Definition of Done

- [ ] Slices 1–3 acceptance criteria met (4–5 strongly recommended before imports; 6 as demand confirms)
- [ ] All tests passing: `cargo nextest run`
- [ ] Code formatted + clippy clean
- [ ] CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) RDF column updated for constructs slice 3 newly renders
