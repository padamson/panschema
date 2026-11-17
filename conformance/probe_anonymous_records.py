"""Ask the reference implementation what a record with no identifier is.

Run with `uv run --project conformance conformance/probe_anonymous_records.py`.
A class that declares neither an `identifier` nor a `key` can only be held
inline. Each case below holds records of such a class a different way (a
list, a single value, nested inside another such record, two records
sharing a `name`, a bare string where a record belongs, a dict spelling)
and prints `linkml-validate`'s verdict and the Turtle `linkml-convert`
writes. panschema gives such a record no IRI and no `rdfs:label`, projects
it as a blank node, never merges two of them, names it in a finding by
where it sits in the data, and reports a key in a dict spelling (see
`Instance::anonymous` in `panschema-model/src/instances.rs`).
"""

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
      id: {identifier: true, range: uriorcurie}
      parcels: {range: Parcel, multivalued: true, inlined_as_list: true}
  Parcel:
    attributes:
      id: {identifier: true, range: uriorcurie}
      stamps: {range: Stamp, multivalued: true}
      size: {range: Size}
  Stamp:
    attributes:
      name: {}
      value: {range: integer}
      size: {range: Size}
  Size:
    attributes:
      width: {range: integer}
"""

# Each case: the `Parcel` record held by the root.
CASES = {
    "a list of records": """
    stamps:
      - {name: north, value: 1}
      - {name: south, value: 2}
""",
    "a single-valued record": """
    size: {width: 3}
""",
    "a record nested in a record": """
    stamps:
      - {name: north, size: {width: 4}}
""",
    "two records sharing a name": """
    stamps:
      - {name: north, value: 1}
      - {name: north, value: 2}
""",
    "a bare string where a record belongs": """
    stamps: [north]
""",
    "records in a dict spelling": """
    stamps:
      north: {value: 1}
      south: {value: 2}
""",
}


def run(tool: str, *args: str) -> str:
    exe = shutil.which(tool, path=str(Path(sys.executable).parent))
    if exe is None:
        sys.exit(f"{tool} is not installed in this environment")
    r = subprocess.run([exe, *args], capture_output=True, text=True)
    out = r.stdout.strip() or (r.stderr.strip().splitlines() or ["no output"])[-1]
    return out if r.returncode == 0 else f"exit {r.returncode}: {out}"


def triples(turtle: str) -> str:
    """The statements, without the prefix header."""
    lines = [
        line for line in turtle.splitlines() if line.strip() and not line.startswith("@prefix")
    ]
    return "\n".join(f"     {line}" for line in lines)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        schema = Path(tmp) / "schema.yaml"
        schema.write_text(SCHEMA)
        for case, body in CASES.items():
            data = Path(tmp) / "data.yaml"
            data.write_text(f"id: ex:root\nparcels:\n  - id: ex:p1{body}")
            print(f"== {case}")
            print(f"   validate: {run('linkml-validate', '-s', str(schema), '-C', 'Root', str(data))}")
            print("   convert:")
            print(triples(run("linkml-convert", "-s", str(schema), "-C", "Root", "-t", "ttl", str(data))))


if __name__ == "__main__":
    main()
