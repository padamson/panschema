# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CLI with `--input` and `--output` flags for specifying ontology file and output directory.
- Turtle (.ttl) parser using sophia crate to extract ontology metadata (IRI, label, comment, version).
- HTML renderer using askama templates to generate documentation.
- Reference ontology (`tests/fixtures/reference.ttl`) for testing.
