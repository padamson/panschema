# rontodoc

> 🦕 A blazing fast, Rust-based ontology documentation generator.

**Status:** 🚧 Active Development

## 🎯 Why rontodoc?

Read our [WHY.md](WHY.md) to understand the vision behind this project.

**TL;DR:** Ontology documentation needs to be CI-native, fast, and easy to deploy. `rontodoc` replaces heavy Java-based tools with a single, high-performance binary that fits perfectly into modern development workflows.

## 🚀 Vision

We aim to:
- Generate complete documentation sites in milliseconds
- Run natively in CI without complex dependencies (JVM, etc.)
- Provide modern, responsive, and accessible UI templates
- Support OWL and RDF standards out of the box
- Develop a robust tool using test-driven development with unit and integration tests as well as E2E tests using [playwright-rs](https://crates.io/crates/playwright-rs)

## 📦 Installation

```bash
cargo install rontodoc
```

*(Note: Not yet published to crates.io)*

## 🛠️ Development

### Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo-nextest` (recommended for testing)
- Node.js 20+ and Playwright browsers (for E2E tests)

```bash
# Install Playwright browsers (version must match playwright-rs)
npx playwright@1.56.1 install
```

### Building

```bash
cargo build
```

### Running Tests

Run the full test suite (unit + E2E):

```bash
cargo nextest run
```

Cross-browser E2E testing:

```bash
# Default: chromium only
cargo nextest run

# Specific browser
BROWSER=firefox cargo nextest run
BROWSER=webkit cargo nextest run

# All browsers (used in CI)
BROWSER=all cargo nextest run
```

### Manual Verification (Preview with hot reload)

```bash
rontodoc serve --input path/to/reference.ttl
```

This will:
1. Generate documentation from `tests/fixtures/reference.ttl` to `output/`
2. Start a local server at `http://localhost:3000`
3. Watch for changes and regenerate automatically

### UI Component Style Guide (rontodoc Contributors Only)

For contributors developing rontodoc's UI components, build with the `dev` feature:

```bash
# Build with dev tools enabled
cargo build --features dev

# Generate and serve the style guide
cargo run --features dev -- styleguide --serve
```

Open http://localhost:3000/styleguide.html to see all components.

See [docs/components.md](docs/components.md) for the full component development guide.

### Serve Reference Ontology with Auto-Recompile (rontodoc Contributors Only)

To auto-recompile and restart the server when Rust source or templates change, use [cargo-watch](https://crates.io/crates/cargo-watch):

```bash
cargo watch -x 'run -- serve --input tests/fixtures/reference.ttl'
```

## 🤝 Contributing

Contributions are welcome! Please match our existing standards:
- **TDD First**: Write tests before implementation.
- **Strict Linting**: Pass `cargo fmt` and `cargo clippy`.
- **Pre-commit**: Use our pre-commit hooks to ensure quality.

## 📄 License

Apache-2.0
