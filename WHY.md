# Why rontodoc?

## Information Architecture for the AI Era

We are at a transition point in how we define and document data structures:

1.  **Ontologies are becoming critical infrastructure** - as AI agents need structured context to operate reliably.
2.  **Documentation tools are stuck in the past** - relying on heavy Java runtimes (Widoco, LODE) and complex setups.
3.  **CI/CD is non-negotiable** - documentation must drift-check against code/schema automatically.

`rontodoc` exists to bring the speed, safety, and simplicity of **Rust** to the world of ontology documentation.

## The Problem

Current ontology documentation tools often require:
- A JVM installed
- Complex XML/XSLT configurations
- Slow startup times making them painful for "save-and-view" loops
- Heavy CI containers

Meanwhile, the Rust ecosystem offers blazing fast parsers and template engines, but lacks a dedicated, opinionated documentation generator for OWL/RDF.

## The Solution

`rontodoc` aims to be:

- **Single Binary**: No JVM, no Python venv. Just one executable.
- **Blazing Fast**: Generates complex ontology documentation sites with graph visualization, live search, and filtering in milliseconds.
- **Modern Design**: Responsive, searchable, and AI-readable documentation out of the box.
- **CI Native**: Designed to run in GitHub Actions with zero setup.

## Vision

We want to make documenting an OWL/RDF ontology as easy as documenting a Rust crate.

> "If it's not documented, it doesn't exist. If the documentation is hard to build, it won't be documented."
