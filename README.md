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

## 📦 Installation

```bash
cargo install rontodoc
```

*(Note: Not yet published to crates.io)*

## 🛠️ Development

### Prerequisites

- Rust 1.75+
- `cargo-nextest` (recommended for testing)

### Building

```bash
cargo build
```

### Running Tests

Run the full test suite (Unit + E2E):

```bash
cargo test
```

Or using `cargo-nextest` (recommended for speed):

```bash
cargo nextest run
```

### Manual Verification (Preview)

To generate the reference documentation used for testing and serve it locally for manual inspection:

```bash
./scripts/preview.sh
```

This will:
1. Generate documentation from `tests/fixtures/reference.ttl` to `target/doc-preview`
2. Start a local server at `http://localhost:3030` using a pure Rust file server

### Hot Reload

For a faster development loop, you can run the preview script in watch mode. This requires `cargo-watch` to be installed (`cargo install cargo-watch`).

```bash
./scripts/preview.sh --watch
```

This will automatically rebuild the binary and regenerate the documentation whenever you change the source code (`src/`) or the reference ontology (`tests/fixtures/reference.ttl`).


## 🤝 Contributing

Contributions are welcome! Please match our existing standards:
- **TDD First**: Write tests before implementation.
- **Strict Linting**: Pass `cargo fmt` and `cargo clippy`.
- **Pre-commit**: Use our pre-commit hooks to ensure quality.

## 📄 License

Apache-2.0
