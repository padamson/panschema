# Upstream Ontology Label Cache - Implementation Plan

**Feature:** Fetch, cache, and render `rdfs:label`s for external ontology terms referenced by a schema.

**User Story:** As a schema author grounding my classes and slots in upstream ontologies (BFO, CCO, IAO, PROV-O, SKOS, CiTO, …), I want every external CURIE in my rendered docs — `subclass_of`, `*_mappings`, `meaning:` on permissible values — to read as the upstream term's human-readable label (e.g. `Process` instead of `cco:ont00000958`), with the CURIE/IRI as secondary context, so I can read my schema's grounding story without clicking through to each upstream page.

**Related ADR (if applicable):** None — extends the HTML writer's view-model layer and reuses the cache conventions established by the schema manager ([docs/features/05-schema-manager.md](05-schema-manager.md)).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why Now

The scimantic-schema v0.3 spine authoring loop surfaced this as the active bottleneck after external grounding landed: every `subclass_of: cco:ont00000958` and `exact_mappings: [cito:supports]` now renders as a working hyperlink, but the link text is the opaque CURIE. Reading the rendered docs means read-then-look-up-then-resume on every grounding. Modern ontology browsers (BioPortal, OLS, Protégé Web) all render upstream labels; panschema's docs should too.

## Architecture Overview

```
schema.prefixes (cco: → https://www.commoncoreontologies.org/)
        │
        ▼
LabelSource (trait) ──fetch──► upstream RDF ──sophia──► {IRI → TermInfo}
        │                                                          │
        │                                                          ▼
        └──────────── on-disk cache ◄──── ~/.cache/panschema/labels/<hash>.json
                            │
                            ▼
                     LabelStore::lookup(iri) → Option<&TermInfo>
                            │
                            ▼
        html_writer view-models (Mapping, ExternalLink: label + definition)
                            │
                            ▼
        templates render label as link text; tooltip carries
        "CURIE = IRI" plus the upstream definition
```

Design decisions:

- **Keyed by expanded IRI, not CURIE.** `expand_curie` already produces the canonical form; two prefixes mapping to the same namespace share cache entries.
- **Pluggable `LabelSource` trait** (same pattern as `TarballSource` in the schema manager) so unit tests use local fixture files, never the network.
- **Fail open, always.** A missing label, fetch error, or parse error degrades to today's CURIE rendering — generation never blocks on the network.
- **Fetch only on cache miss.** Repeat generates are offline-fast. An explicit `--refresh-labels` CLI flag invalidates; there is no TTL-based auto-expiry (upstream ontology releases are infrequent and versioned).
- **Source resolution order:** (1) explicit URL configured in the manifest, (2) built-in map for the well-known OBO/CCO/PROV/SKOS/CiTO ecosystem, (3) none — prefix renders unlabeled. The built-in map is data, not behavior: a `&[(&str, &str)]` of namespace-IRI → download-URL pairs.
- **Labels extracted:** `rdfs:label` preferred, `skos:prefLabel` as fallback. **Definitions extracted** (shipped with 13.4 rather than deferred — the dogfood asked for them immediately): `skos:definition` (CCO) > `IAO:0000115` (OBO/BFO) > `dc:description` > `rdfs:comment` (CiTO / W3C vocabularies), priorities matched against what those sources actually publish. English (or untagged) literals only in v1.
- **Cache location:** `<cache-root>/labels/<sha256(source-url)>.json` — one JSON `{iri: {label, definition}}` map per upstream source, under the same ProjectDirs cache root as the schema manager (`~/Library/Caches/dev.padamson.panschema` on macOS, `~/.cache/panschema` on Linux). A format change to the cache value self-heals: old-shape files fail to parse, are skipped with a warning, and refetch on the next run.
- **Upstream versioning is URL-level, not metadata-level.** Labels are render-only and fail-open, so drift can't break semantics — a lighter-weight stance than the schema manager's lockfile is appropriate. Versioning still falls out of the design: the cache key is the URL hash, so a *versioned* URL (e.g. a tagged release TTL) is automatically a pinned cache entry, and the 13.5 manifest override is the pinning mechanism for authors who want reproducible labels. The built-in map points at latest-release PURLs; combined with no-auto-refetch, labels are stable per machine until an explicit `--refresh-labels`. Cross-machine byte-level label reproducibility (lockfile-style checksums for label maps) is deliberately out of scope until a publishing workflow needs it.

## Vertical Slices

### Slice 13.1: `LabelStore` — on-disk cache with lookup API

**Status:** ✅ Complete

**User Value:** The cache layer exists and is testable in isolation: store a label map for a source URL, look labels up by IRI, survive process restarts.

**Acceptance Criteria (write tests first):**
- [x] `LabelStore::open(cache_dir)` loads/initializes the store rooted at a directory (injectable for tests; production callers pass `<cache-root>/labels`).
- [x] `store.insert_source(source_url, terms: BTreeMap<String, TermInfo>)` persists one JSON file per source, named by SHA-256 of the URL.
- [x] `store.lookup(iri) -> Option<&TermInfo>` searches all loaded sources; first hit wins.
- [x] Round-trip test: insert → drop → reopen → lookup still hits.
- [x] Corrupt/unparseable cache file is skipped with a `tracing::warn!`, not a panic — fail open. (Doubles as cache-format migration: pre-TermInfo flat files refetch automatically.)
- [x] No network, no async; pure fs + serde_json.

### Slice 13.2: `LabelSource` trait + RDF label extraction

**Status:** ✅ Complete

**User Value:** Given upstream ontology RDF, panschema can extract an `{IRI → TermInfo}` map. The network is behind a trait; tests use fixture strings.

**Acceptance Criteria (write tests first):**
- [x] `trait LabelSource { fn fetch(&self, url: &str) -> Result<String, LabelFetchError>; }` — returns the RDF document body. Production impl uses `ureq` (already a dep); test impl serves canned strings.
- [x] `extract_terms(rdf: &str) -> BTreeMap<String, TermInfo>` parses Turtle via sophia — falling back to RDF/XML, since OBO PURLs serve that — and collects labels (`rdfs:label` > `skos:prefLabel`) and definitions (`skos:definition` > `IAO:0000115` > `dc:description` > `rdfs:comment`) per subject IRI.
- [x] Language-tagged literals: `@en` and untagged win; other languages ignored in v1.
- [x] Fixture tests cover: label priority; definition priority; a definition-only term still produces an entry; the RDF/XML fallback.
- [x] Malformed RDF → `Err`, which callers treat as fail-open (warn + skip).

### Slice 13.3: Orchestration — populate cache for a schema's prefixes

**Status:** ✅ Complete

**User Value:** `ensure_labels(schema, store, source)` walks the schema's declared prefixes, resolves each to a label-source URL (manifest override → built-in map → skip), fetches on cache miss, and returns a ready `LabelStore`.

**Acceptance Criteria (write tests first):**
- [x] Built-in source map covers: BFO, RO, IAO (via OBO PURLs), CCO (merged release TTL), PROV-O, SKOS, Dublin Core terms, CiTO, OA. Stored as data (`const` slice), unit-tested for URL well-formedness.
- [x] Prefixes whose namespace IRI matches a built-in entry get fetched on cache miss; already-cached sources are not re-fetched (verified with a counting test double).
- [x] Unknown prefixes are skipped silently — no error, no fetch attempt.
- [x] A fetch error on one source doesn't abort the others (fail open per source).
- [x] No manifest config in this slice — built-in map only; the manifest override lands in 13.5.

### Slice 13.4: Render labels in HTML

**Status:** ✅ Complete

**User Value:** The dogfood payoff: external references in the rendered docs read as names. `Subclass of (external): Process` with `cco:ont00000958` in the tooltip, instead of the bare CURIE.

**Acceptance Criteria (write tests first):**
- [x] `ExternalLink` and `Mapping` gain `label` + `definition` fields, populated from `LabelStore::lookup` on the expanded IRI. A `tooltip()` method composes the `"{curie} = {iri}"` identity line with the definition on its own paragraph (browsers render literal newlines in `title`).
- [x] `class_card.html`: external-subclass-of links render the label as link text when present, with the tooltip carrying identity + definition; CURIE remains the text when no label is cached.
- [x] Mappings rows (class and property cards): same treatment per mapping value.
- [x] `HtmlWriter` carries an optional `LabelStore` (None = unlabeled rendering); **both** the `generate` CLI path and the `panschema publish` per-version path wire a real store via the shared `labels::open_default_store` fail-open helper. (The publish gap was caught in the scimantic dogfood — `scripts/dev.sh` builds through publish, not generate.)
- [x] Unit tests: view-model population with a pre-seeded store covers label-present, label-absent, and definition-tooltip paths.
- [x] `--offline` CLI flag degrades cleanly: cached labels still render; uncached external references show CURIEs.

### Slice 13.5: Manifest override + `--refresh-labels`

**Status:** ✅ Complete

**User Value:** Authors can point a prefix at a custom label source (e.g. a corporate ontology) and force-refresh stale caches after an upstream release.

**Acceptance Criteria (write tests first):**
- [x] `panschema.toml` / `panschema-publish.toml` accept `[label_sources]` mapping prefix → URL; entries override the built-in map.
- [x] `panschema generate --refresh-labels` deletes the relevant cache files before the ensure-labels pass.
- [x] Manifest parse round-trips through `toml_edit` preserving comments (consistent with existing manifest handling).

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| 13.1: LabelStore | Must Have | None | ✅ Complete |
| 13.2: LabelSource + extraction | Must Have | None | ✅ Complete |
| 13.3: Orchestration + built-in map | Must Have | 13.1, 13.2 | ✅ Complete |
| 13.4: Render labels in HTML | Must Have | 13.3 | ✅ Complete |
| 13.5: Manifest override + refresh | Should Have | 13.4 | ✅ Complete |

## Out of Scope (deferred)

- **Graph hover-card labels.** The wasm graph payload could carry labels too; defer until the HTML loop proves the cache design.
- **Reverse lookup / label search.** "Find the CCO term whose label contains X" is an authoring-assist feature, not a rendering one.
- **Non-English labels.** v1 takes `@en`/untagged only.
- **Auto-TTL-expiry.** Upstream releases are infrequent; explicit `--refresh-labels` is enough.

## Verification

Dogfood loop in scimantic-schema after 13.4:

```bash
cargo install --path panschema --debug   # in panschema
./scripts/dev.sh                          # in scimantic-schema
# Claim / Act / State cards: "Subclass of (external)" shows the CCO label,
# not cco:ont00000958. First generate fetches (one-time); repeats are offline.
```
