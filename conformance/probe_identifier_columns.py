"""Ask the reference implementation how it stores an identified part and an IRI id.

Run with `uv run --project conformance conformance/probe_identifier_columns.py`.

A schema whose every record class has an `identifier` still holds some
records by composition: a root's `inlined_as_list` slot owns them. Three
questions follow, each answered by running LinkML's own generators:

1. Does `gen-sqlddl` hold an identified part held by composition as a
   linking table, or as a column on the part naming its holder? And a
   multivalued reference that is not inlined?
2. What column and JSON Schema type does an id get when its range is
   `string` with a pattern, `uri`, `uriorcurie`, or a custom type derived
   from `uriorcurie`?
3. Does any generator read `implicit_prefix` on an id slot?

Evidence behind the Postgres writer keeping an IRI id a `text` column.
`gen-sqlddl` emits no `CHECK` for a pattern; carrying one is panschema's
choice, as on every other patterned column.
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

HEADER = """id: https://example.org/t
name: t
prefixes:
  linkml: https://w3id.org/linkml/
  ex: https://example.org/t/id/
imports: [linkml:types]
default_prefix: https://example.org/t/
default_range: string
"""

COMPOSITION = HEADER + """
types:
  iri_id:
    typeof: uriorcurie
    pattern: "^ex:[0-9a-f-]{36}$"
slots:
  id: {identifier: true, range: string, pattern: "^ex:[0-9a-f-]{36}$"}
  uri_id: {identifier: true, range: uri}
  curie_id: {identifier: true, range: uriorcurie}
  typed_id: {identifier: true, range: iri_id}
  prefixed_id: {identifier: true, range: string, implicit_prefix: ex}
  parts: {range: Part, multivalued: true, inlined_as_list: true}
  tags: {range: Tag, multivalued: true}
  owner: {range: Holder}
  uri_refs: {range: UriRecord, multivalued: true}
classes:
  Holder:
    tree_root: true
    slots: [id, parts, tags, uri_refs]
  Part:
    slots: [id, owner]
  Tag:
    slots: [id]
  UriRecord:
    slots: [uri_id]
  CurieRecord:
    slots: [curie_id]
  TypedRecord:
    slots: [typed_id]
  PrefixedRecord:
    slots: [prefixed_id]
"""


FAILED: list[str] = []


def run(cmd: list[str]) -> str:
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        FAILED.append(cmd[0])
        return f"[exit {out.returncode}] {out.stderr.strip()[-400:]}"
    return out.stdout


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        schema = Path(tmp) / "schema.yaml"
        schema.write_text(COMPOSITION)

        print("== gen-sqlddl ==")
        print(run(["gen-sqlddl", str(schema)]))

        print("== gen-json-schema: id properties ==")
        js = run(["gen-json-schema", str(schema)])
        try:
            defs = json.loads(js)["$defs"]
            for cls, slot in [
                ("Holder", "id"),
                ("UriRecord", "uri_id"),
                ("CurieRecord", "curie_id"),
                ("TypedRecord", "typed_id"),
                ("PrefixedRecord", "prefixed_id"),
                ("Holder", "parts"),
                ("Holder", "tags"),
                ("Holder", "uri_refs"),
            ]:
                print(f"{cls}.{slot}: {json.dumps(defs[cls]['properties'][slot])}")
        except (ValueError, KeyError) as e:
            FAILED.append("gen-json-schema output")
            print(f"could not read: {e}\n{js[:800]}")

        print("== gen-pydantic: id fields ==")
        py = run(["gen-pydantic", str(schema)])
        id_slots = ("id", "uri_id", "curie_id", "typed_id", "prefixed_id")
        model = ""
        for line in py.splitlines():
            if line.startswith("class "):
                model = line.split("(")[0].removeprefix("class ")
            elif any(line.strip().startswith(f"{s}:") for s in id_slots):
                print(f"{model}.{line.strip()}")

        print("== implicit_prefix in SchemaView ==")
        from linkml_runtime.utils.schemaview import SchemaView

        sv = SchemaView(str(schema))
        s = sv.induced_slot("prefixed_id", "PrefixedRecord")
        print(f"implicit_prefix = {s.implicit_prefix!r}")
    if FAILED:
        print(f"FAILED: {', '.join(FAILED)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
