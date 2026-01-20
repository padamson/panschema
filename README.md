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

We use `cargo-nextest` for comprehensive testing:

```bash
cargo nextest run
```

## 🤝 Contributing

Contributions are welcome! Please match our existing standards:
- **TDD First**: Write tests before implementation.
- **Strict Linting**: Pass `cargo fmt` and `cargo clippy`.
- **Pre-commit**: Use our pre-commit hooks to ensure quality.

## 📄 License

Apache-2.0
