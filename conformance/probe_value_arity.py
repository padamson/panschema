"""Ask LinkML what happens when a slot gets the wrong number of values.

Run with `uv run --project conformance conformance/probe_value_arity.py`.
A single-valued slot should hold one value and a multivalued slot should
hold a list. Each case breaks that on one slot of a `Parcel` record: a
list where one value belongs, one value where a list belongs, or a list
inside a list. It tries each on a slot holding text and on a slot holding
a reference to another record, and prints what `linkml-validate` says
and the JSON `linkml-convert` loads, which is what LinkML's runtime makes
of the value. panschema reports each case and loads none of the values
(see `ingest_field` in `panschema-model/src/instances.rs`).
"""

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SCHEMA = """id: https://example.org/t
name: t
prefixes:
  linkml: https://w3id.org/linkml/
  ex: https://example.org/t/
imports: [linkml:types]
default_prefix: ex
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      parcels: {range: Parcel, multivalued: true, inlined_as_list: true}
  Parcel:
    attributes:
      id: {identifier: true}
      label: {}
      weight: {range: integer}
      tags: {multivalued: true}
      route: {range: Parcel}
      stops: {range: Parcel, multivalued: true}
"""

CASES = {
    "conforming": "label: L, weight: 1, tags: [a], route: p2, stops: [p2]",
    "a one-entry list at a single-valued string": "label: [L]",
    "a two-entry list at a single-valued string": "label: [L, M]",
    "a one-entry list at a single-valued integer": "weight: [1]",
    "a bare string at a multivalued string": "tags: north",
    "a list nested in a multivalued string": "tags: [[a, b]]",
    "a one-entry list at a single-valued reference": "route: [p2]",
    "a bare id at a multivalued reference": "stops: p2",
    "a list nested in a multivalued reference": "stops: [[p2]]",
}


def run(tool: str, *args: str) -> tuple[int, str]:
    exe = shutil.which(tool, path=str(Path(sys.executable).parent))
    if exe is None:
        sys.exit(f"{tool} is not installed in this environment")
    r = subprocess.run([exe, *args], capture_output=True, text=True)
    return r.returncode, r.stdout.strip() or (r.stderr.strip().splitlines() or ["no output"])[-1]


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        schema = Path(tmp) / "schema.yaml"
        schema.write_text(SCHEMA)
        data = Path(tmp) / "data.yaml"
        for case, fields in CASES.items():
            data.write_text(f"parcels:\n  - {{id: p2}}\n  - {{id: p1, {fields}}}\n")
            print(f"== {case}")
            code, out = run("linkml-validate", "-s", str(schema), "-C", "Root", str(data))
            print(f"   validate: {out if code == 0 else f'exit {code}: {out}'}")
            code, out = run("linkml-convert", "-s", str(schema), "-C", "Root", "-t", "json", str(data))
            if code == 0:
                loaded = json.loads(out)["parcels"][1]
                loaded.pop("id")
                print(f"   convert: {json.dumps(loaded, sort_keys=True)}")
            else:
                print(f"   convert: exit {code}: {out}")


if __name__ == "__main__":
    main()
