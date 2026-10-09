"""Ask the reference validator which value forms a class-ranged slot accepts.

Run with `uv run --project conformance conformance/probe_inlining.py`.
Each case is one slot declaration crossed with one authored form; the
output is the reference implementation's verdict. panschema's `verify`
reports the same mismatches as range-kind violations (see
`panschema/src/validate.rs`), and reads inlined records by shape.
"""

from linkml.validator import validate
from linkml_runtime.utils.schemaview import SchemaView
import linkml_runtime

SCHEMA = """id: https://example.org/t
name: t
prefixes: {{linkml: https://w3id.org/linkml/}}
imports: [linkml:types]
default_range: string
classes:
  Container:
    tree_root: true
    attributes:
      {slot}
  Item:
    attributes:
      id: {{identifier: true}}
      label: {{}}
  Note:
    attributes:
      text: {{}}
  Keyed:
    attributes:
      code: {{key: true}}
      label: {{}}
"""

SLOTS = {
    "multi, id range, unflagged": "items: {range: Item, multivalued: true}",
    "multi, id range, inlined": "items: {range: Item, multivalued: true, inlined: true}",
    "multi, id range, inlined_as_list": "items: {range: Item, multivalued: true, inlined_as_list: true}",
    "multi, id range, inlined: false": "items: {range: Item, multivalued: true, inlined: false}",
    "single, id range, unflagged": "items: {range: Item}",
    "single, id range, inlined": "items: {range: Item, inlined: true}",
    "multi, no-id range, unflagged": "items: {range: Note, multivalued: true}",
    "multi, no-id range, inlined: false": "items: {range: Note, multivalued: true, inlined: false}",
    "single, no-id range, unflagged": "items: {range: Note}",
    "multi, key range, unflagged": "items: {range: Keyed, multivalued: true}",
    "multi, key range, inlined": "items: {range: Keyed, multivalued: true, inlined: true}",
}

FORMS = {
    "list of ids": {"items": ["a", "b"]},
    "list of objects": {"items": [{"id": "a", "label": "A"}, {"id": "b"}]},
    "dict keyed by id": {"items": {"a": {"label": "A"}, "b": {}}},
    "one id": {"items": "a"},
    "one object": {"items": {"id": "a", "label": "A"}},
    "list of note objects": {"items": [{"text": "x"}]},
    "one note object": {"items": {"text": "x"}},
    "list of keyed objects": {"items": [{"code": "a", "label": "A"}]},
    "dict keyed by code": {"items": {"a": {"label": "A"}}},
}


def main() -> None:
    print("linkml-runtime", linkml_runtime.__version__)
    for slot_name, slot in SLOTS.items():
        schema = SCHEMA.format(slot=slot)
        # The schema must at least load; a declaration the metamodel
        # rejects is a verdict of its own.
        try:
            SchemaView(schema)
        except Exception as e:  # noqa: BLE001
            print(f"{slot_name:36s} SCHEMA ERROR: {type(e).__name__}: {str(e)[:100]}")
            continue
        for form_name, data in FORMS.items():
            try:
                report = validate(data, schema, "Container")
                verdict = "ok" if not report.results else "; ".join(
                    r.message.split("\n")[0][:90] for r in report.results[:2]
                )
            except Exception as e:  # noqa: BLE001
                verdict = f"ERROR {type(e).__name__}: {str(e).split(chr(10))[0][:90]}"
            print(f"{slot_name:36s} | {form_name:20s} -> {verdict}")
        print()


if __name__ == "__main__":
    main()
