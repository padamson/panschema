# ADR-002: Crate Selection

## Status
Accepted

## Context

Rontodoc requires external crates for RDF parsing, templating, CLI, and other functionality. We need to choose crates that are:
- Well-maintained and actively developed
- Pure Rust where possible (for single-binary goal)
- Fast (millisecond generation target)
- Appropriate for the task complexity

## Decision

### Core Dependencies

| Purpose | Crate | Version | Rationale |
|---------|-------|---------|-----------|
| CLI | `clap` (derive) | 4.x | Industry standard, derive macros for ergonomics, excellent docs |
| RDF/OWL Parsing | `sophia` | 0.8.x | Pure Rust, comprehensive RDF support, OWL-aware, active maintainer |
| Templating | `askama` | 0.12.x | Compile-time templates, type-safe, fastest option |
| Serialization | `serde` + `serde_json` | 1.x | De facto standard, search index and graph data |
| Error handling | `thiserror` | 1.x | Derive macros for custom error types |
| Error handling | `anyhow` | 1.x | Ergonomic error propagation in main/CLI |
| Logging | `tracing` | 0.1.x | Structured logging, spans for pipeline stages |

### Development Dependencies

| Purpose | Crate | Rationale |
|---------|-------|-----------|
| File watching | `notify` | Cross-platform file system events for hot reload |
| Dev server | `axum` | Lightweight, tokio-based HTTP for preview server |
| Snapshot testing | `insta` | Snapshot testing for HTML output verification |
| E2E testing | `playwright` | Browser automation for testing generated docs (search, graph, navigation) |

### Crate-Specific Rationale

**sophia over oxigraph**: While oxigraph is excellent for SPARQL and graph databases, sophia is lighter-weight and focused on parsing/serialization. We don't need a query engine—just parsing and traversal.

**askama over tera**: Tera offers runtime flexibility but askama's compile-time approach aligns with our performance goals. Template errors are caught at compile time, and rendering is essentially string concatenation.

**axum over warp**: Both are excellent. Axum has stronger momentum and tower ecosystem integration. The dev server is minimal, so either works.

**playwright for E2E**: The generated documentation includes client-side JavaScript (search, graph visualization) that cannot be tested with unit tests alone. Playwright enables real browser testing to verify search works, graphs render, and navigation functions correctly. The `playwright` crate (Rust bindings) keeps us in-ecosystem. As rontodoc's author maintains playwright-rs, this project serves as a real-world validation of the crate.

## Consequences

### Positive

- **Pure Rust stack**: No C dependencies, simplifies cross-compilation
- **Compile-time safety**: askama catches template errors early
- **Performance**: All chosen crates are optimized for speed
- **Ecosystem fit**: All crates follow Rust conventions and integrate well

### Negative

- **sophia learning curve**: Less documentation than oxigraph; RDF semantics require careful modeling
- **askama rigidity**: Template changes require recompilation (acceptable for release builds)
- **Dependency count**: Multiple crates increase binary size slightly (mitigated by LTO)
