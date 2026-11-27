"""Ask the reference implementation what an explicit `null` at a slot is.

Run with `uv run --project conformance conformance/probe_null_values.py`.
Each case sets one slot of a `Parcel` record to `null` (a scalar, an
inlined record, a reference, a list of inlined records, a list of
references, a list of scalars, and a required slot), or puts a `null`
inside one of the lists, gives an entry of an identifier-keyed dict a
`null` body, sets a type designator to `null`, sets a slot a rule requires
to `null`, or puts a `null` inside one of the container's own lists, and
prints `linkml-validate`'s verdict and the Turtle `linkml-convert` writes.
panschema reads a slot set to `null` as absent, whatever its range and
whether a rule requires it, and reports a `null` inside a list as a value
outside the slot's range (see `LinkmlLoader::build_record` and
`ingest_field` in `panschema-model/src/instances.rs`).
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
      depots: {range: Depot, multivalued: true, inlined_as_list: true}
      notes: {multivalued: true}
      featured: {range: Depot}
  Parcel:
    attributes:
      id: {identifier: true, range: uriorcurie}
      label: {}
      tags: {multivalued: true}
      size: {range: Size}
      stamps: {range: Size, multivalued: true}
      depot: {range: Depot}
      route: {range: Depot, multivalued: true}
      weight: {range: integer, required: true}
      status: {}
      bins: {range: Bin, multivalued: true, inlined: true}
      lockers: {range: Locker, multivalued: true, inlined: true}
      wrap: {range: Wrap}
      holder: {any_of: [{range: Person}, {range: Org}]}
    rules:
      - preconditions: {slot_conditions: {status: {equals_string: sealed}}}
        postconditions:
          slot_conditions:
            label: {required: true}
            size: {required: true}
            depot: {required: true}
  Size:
    attributes:
      width: {range: integer}
  Depot:
    attributes:
      id: {identifier: true, range: uriorcurie}
  Bin:
    attributes:
      id: {identifier: true, range: uriorcurie}
      label: {}
  Locker:
    attributes:
      id: {identifier: true, range: uriorcurie}
      code: {required: true}
  Wrap:
    attributes:
      kind: {designates_type: true}
      width: {range: integer}
  Crate:
    is_a: Wrap
    attributes:
      depth: {range: integer}
  Person:
    attributes:
      kind: {designates_type: true}
      given_name: {}
  Org:
    attributes:
      kind: {designates_type: true}
      org_name: {}
"""

# Each case: one slot of the `Parcel` record and the value it is given.
CASES = {
    "a scalar": ("label", "null"),
    "a list of scalars": ("tags", "null"),
    "an inlined record": ("size", "null"),
    "a list of inlined records": ("stamps", "null"),
    "a reference": ("depot", "null"),
    "a list of references": ("route", "null"),
    "a required scalar": ("weight", "null"),
    "a null in a list of scalars": ("tags", "[north, null]"),
    "a null in a list of inlined records": ("stamps", "[{width: 1}, null]"),
    "a null in a list of references": ("route", "[ex:d1, null]"),
    "a dict entry with a null body": ("bins", "{ex:b9: null}"),
    "a dict entry with a null body whose class requires a slot": ("lockers", "{ex:k9: null}"),
    "a type designator naming a subclass": ("wrap", "{kind: Crate, depth: 2}"),
    "a type designator set to null": ("wrap", "{kind: null, width: 1}"),
    "a type designator naming a union member": ("holder", "{kind: Person, given_name: Ada}"),
    "a type designator set to null at a union": ("holder", "{kind: null, given_name: Ada}"),
    "a union member's record with no type designator": ("holder", "{given_name: Ada}"),
}

# Each case: a sealed `Parcel` record, which the rule requires to have a
# `label`, a `size`, and a `depot`.
SEALED = "\n    status: sealed\n    label: L\n    size: {width: 1}\n    depot: ex:d1"
RULE_CASES = {
    "a rule's required slots all present": SEALED,
    "a rule's required scalar set to null": SEALED.replace("label: L", "label: null"),
    "a rule's required record set to null": SEALED.replace("size: {width: 1}", "size: null"),
    "a rule's required reference set to null": SEALED.replace("depot: ex:d1", "depot: null"),
    "a rule's required slots absent": "\n    status: sealed",
}

# The container's own slots: a list of scalars and a single-valued
# reference, each a list with a null inside it.
CONTAINER = {
    "a null in the container's list of scalars": "notes: [north, null]\n",
    "a null in a list at the container's single-valued reference": "featured: [ex:d1, null]\n",
}


def run(tool: str, *args: str) -> str:
    exe = shutil.which(tool, path=str(Path(sys.executable).parent))
    if exe is None:
        sys.exit(f"{tool} is not installed in this environment")
    r = subprocess.run([exe, *args], capture_output=True, text=True)
    errors = r.stderr.strip().splitlines() or ["no output"]
    # `linkml-convert` prints a failed check as a dict, its message one line of it.
    message = [line.strip() for line in errors if "'message':" in line]
    out = r.stdout.strip() or (message or errors)[-1]
    return out if r.returncode == 0 else f"exit {r.returncode}: {out}"


def triples(turtle: str) -> str:
    """The statements, without the prefix header."""
    lines = [
        line for line in turtle.splitlines() if line.strip() and not line.startswith("@prefix")
    ]
    return "\n".join(f"     {line}" for line in lines)


def report(case: str, schema: Path, data: Path, text: str) -> None:
    data.write_text(text)
    print(f"== {case}")
    print(f"   validate: {run('linkml-validate', '-s', str(schema), '-C', 'Root', str(data))}")
    print("   convert:")
    print(triples(run("linkml-convert", "-s", str(schema), "-C", "Root", "-t", "ttl", str(data))))


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        schema = Path(tmp) / "schema.yaml"
        schema.write_text(SCHEMA)
        data = Path(tmp) / "data.yaml"
        head = "id: ex:root\ndepots:\n  - id: ex:d1\nparcels:\n  - id: ex:p1"
        for case, (slot, value) in CASES.items():
            weight = "" if slot == "weight" else "\n    weight: 1"
            report(case, schema, data, f"{head}{weight}\n    {slot}: {value}\n")
        for case, fields in RULE_CASES.items():
            report(case, schema, data, f"{head}\n    weight: 1{fields}\n")
        for case, field in CONTAINER.items():
            report(case, schema, data, f"{head}\n    weight: 1\n{field}")


if __name__ == "__main__":
    main()
