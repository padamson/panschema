# Conformance oracle

A uv-managed Python environment pinning `linkml-runtime`, for settling
questions about LinkML metamodel semantics against the reference
implementation instead of arguing from the docs.

This is a development tool, not part of the test suite: `cargo nextest`
never touches it, CI does not require Python, and nothing here gates a
merge. Run a probe when a semantics question comes up, record the verdict
in the code or `docs/linkml-coverage.md`, and move on.

## Usage

```bash
uv run --project conformance conformance/probe_annotations.py
```

Load an arbitrary schema through the reference implementation:

```bash
uv run --project conformance python -c \
  "from linkml_runtime.utils.schemaview import SchemaView; SchemaView('path/to/schema.yaml')"
```

Add a probe script per topic as questions come up; keep each one small
and self-explaining, with the panschema behavior it checks named in the
docstring.
