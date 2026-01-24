# Feature: Foundational UI Stack & Documentation Layout

## Goal
Establish robust, developer-friendly "Foundational Stack" for `rontodoc`'s development. This should include a build process with hot reload and a capability for doing UI component design and testing similar to Storybook in typescript. Also, we will define the structural layout of the generated documentation.

## Implementation Plan

### Slice 1a: Project Scaffold & CI Green
Minimal compiling project that exercises the full CI/CD pipeline.

**Acceptance Criteria:**
- [x] `Cargo.toml` with project metadata and initial dependencies
- [x] `src/main.rs` with placeholder CLI (clap) that prints version
- [x] `tests/fixtures/reference.ttl` with minimal valid ontology
- [x] Passes `cargo fmt --check`, `cargo clippy`, `cargo nextest run`
- [x] CI workflow runs green on push

### Slice 1b: Walking Skeleton
Minimal end-to-end pipeline proving the architecture works.

**Acceptance Criteria:**
- [x] CLI accepts `--input` flag with path to .ttl file
- [x] Parser reads Turtle file and extracts basic triples
- [x] Renderer outputs minimal `index.html` with ontology IRI and label
- [x] Output written to `--output` directory (default: `output/`)
- [x] Unit tests for parser and renderer
- [x] Integration test: input reference.ttl → verify HTML output

### Slice 2: Dev Server with Hot Reload
Enable rapid iteration during development.

**Acceptance Criteria:**
- [ ] `rontodoc serve` starts axum-based HTTP server on port 3000
- [ ] Serves generated documentation from output directory
- [ ] File watcher (notify) detects changes to input .ttl
- [ ] Regenerates documentation on change
- [ ] Browser receives update (via reload or WebSocket)

### Slice 3: Component Design Workflow
Storybook-like capability for UI component development.

**Acceptance Criteria:**
- [ ] Isolated component templates can be previewed independently
- [ ] Style guide page showing all UI components
- [ ] Documentation for adding new components
- [ ] Snapshot tests (insta) for component HTML output

### Slice 4: Documentation Layout Structure
Define the structural HTML layout for generated docs using the component workflow.

**Acceptance Criteria:**
- [ ] Base template with header, navigation, content area, footer
- [ ] Responsive CSS (mobile-friendly)
- [ ] Navigation structure for classes, properties, individuals
- [ ] Ontology overview page with metadata (title, description, version)
- [ ] Placeholder pages for class/property/individual listings

### Slice 5: E2E Testing with Playwright
End-to-end browser tests for generated documentation.

**Acceptance Criteria:**
- [ ] playwright-rs dev dependency added and configured
- [ ] E2E test: verify index.html renders correctly in browser
- [ ] E2E test: verify navigation links work
- [ ] E2E test: verify responsive layout on mobile viewport
- [ ] CI runs E2E tests (headless browser)

### Slice 6: Release & CD Setup
Publish to crates.io and enable continuous deployment.

**Acceptance Criteria:**
- [ ] crates.io account configured with API token
- [ ] GitHub secret for CARGO_REGISTRY_TOKEN
- [ ] Release workflow publishes to crates.io on tag
- [ ] Tag v0.1.0 to verify full CD pipeline
- [ ] `cargo install rontodoc` works from crates.io
