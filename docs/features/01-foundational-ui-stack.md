# Feature: Foundational UI Stack & Documentation Layout

## Goal
Establish robust, developer-friendly "Foundational Stack" for `rontodoc`'s development. This should include a build process with hot reload and a capability for doing UI component design and testing similar to Storybook in typescript. Also, we will define the structural layout of the generated documentation.

## Implementation Plan

### Slice 1a: Project Scaffold & CI Green
Minimal compiling project that exercises the full CI/CD pipeline.

**Acceptance Criteria:**
- [ ] `Cargo.toml` with project metadata and initial dependencies
- [ ] `src/main.rs` with placeholder CLI (clap) that prints version
- [ ] `tests/fixtures/reference.ttl` with minimal valid ontology
- [ ] Passes `cargo fmt --check`, `cargo clippy`, `cargo nextest run`
- [ ] CI workflow runs green on push
- [ ] Tag `v0.0.1` to verify release workflow builds binaries

### Slice 1b: Walking Skeleton
Minimal end-to-end pipeline proving the architecture works.

**Acceptance Criteria:**
- [ ] CLI accepts `--input` flag with path to .ttl file
- [ ] Parser reads Turtle file and extracts basic triples
- [ ] Renderer outputs minimal `index.html` with ontology IRI and label
- [ ] Output written to `--output` directory (default: `output/`)
- [ ] Unit tests for parser and renderer
- [ ] Integration test: input reference.ttl → verify HTML output

### Slice 2: Dev Server with Hot Reload
Enable rapid iteration during development.

**Acceptance Criteria:**
- [ ] `rontodoc serve` starts axum-based HTTP server on port 3000
- [ ] Serves generated documentation from output directory
- [ ] File watcher (notify) detects changes to input .ttl
- [ ] Regenerates documentation on change
- [ ] Browser receives update (via reload or WebSocket)

### Slice 3: Documentation Layout Structure
Define the structural HTML layout for generated docs.

**Acceptance Criteria:**
- [ ] Base template with header, navigation, content area, footer
- [ ] Responsive CSS (mobile-friendly)
- [ ] Navigation structure for classes, properties, individuals
- [ ] Ontology overview page with metadata (title, description, version)
- [ ] Placeholder pages for class/property/individual listings

### Slice 4: Component Design Workflow
Storybook-like capability for UI component development.

**Acceptance Criteria:**
- [ ] Isolated component templates can be previewed independently
- [ ] Style guide page showing all UI components
- [ ] Documentation for adding new components
- [ ] Snapshot tests (insta) for component HTML output
