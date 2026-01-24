# Rontodoc Roadmap

This document outlines the planned features for rontodoc, organized as vertical slices that deliver end-to-end value incrementally.

## Feature Roadmap

| # | Feature | Description | Status |
|---|---------|-------------|--------|
| 01 | [Foundational UI Stack](features/01-foundational-ui-stack.md) | Walking skeleton: CLI, basic parsing, minimal HTML output, dev server with hot reload | Planned |
| 02 | Class Documentation | Full class pages with hierarchy visualization | Planned |
| 03 | Property Documentation | Object and data property pages with domain/range info | Planned |
| 04 | Client-Side Search and Filtering | JSON index generation and in-browser search/filtering | Planned |
| 05 | Graph Visualization | Interactive ontology graph with Cytoscape.js/D3.js | Planned |
| 06 | Individual Documentation | Named individual pages | Planned |

## Delivery Approach

Each feature is a **vertical slice** that delivers working functionality:

1. **Walking Skeleton First** - Feature 01 establishes the full pipeline (parse → model → render) with minimal scope
2. **Incremental Enhancement** - Each subsequent feature adds capability while maintaining a working system
3. **TDD Throughout** - Every slice includes tests before implementation

## Release Strategy

- **v0.1.0** - Features 01-03: Basic ontology documentation (classes, properties)
- **v0.2.0** - Features 04-05: Search and visualization
- **v0.3.0** - Feature 06 + polish: Full documentation with individuals
- **v1.0.0** - Production-ready with comprehensive OWL 2 support
