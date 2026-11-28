"""Ask the reference implementation whether a container with no identifier
has its own fields checked.

Run with `uv run --project conformance conformance/probe_vessel_fields.py`.
The `tree_root` class `Depot` declares no identifier, so its data holds
records and describes the dataset but is no record itself. Each case
breaks one of its own fields (a `null` in its list of scalars, an object
or a wrong-kind value at a scalar slot, a field the class does not
declare, a bare number at its union of classes, a required slot left
out, a number or an empty list where its required collection belongs, a
bare string where a list belongs, a list where one value belongs, a list
inside its collection) or sets an id or a record at the union, or a
number at a union that also allows integers, and prints
`linkml-validate`'s verdict, with `linkml-convert`'s for the empty list.
A last case gives `Depot` an identifier and leaves it out. panschema
checks such a container's fields as it checks any record's, without
emitting it (see `InstanceSet::from_linkml_data` in
`panschema-model/src/instances.rs`).
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
  Depot:
    tree_root: true
    attributes:
      title: {required: true}
      notes: {multivalued: true}
      count: {range: integer}
      shelves: {range: Shelf, multivalued: true, inlined_as_list: true, required: true}
      pick: {any_of: [{range: Shelf}, {range: Crate}]}
      either: {any_of: [{range: Shelf}, {range: Crate}, {range: integer}]}
      one: {any_of: [{range: Shelf}, {range: integer}]}
      many: {multivalued: true, any_of: [{range: Shelf}, {range: integer}]}
      featured: {range: Shelf}
  Shelf:
    attributes:
      id: {identifier: true}
      held: {}
  Crate:
    attributes:
      id: {identifier: true}
      weight: {range: integer}
"""

BASE = "title: T\nshelves:\n  - id: s1\n"

# The one case the runtime's verdict is printed for too: the two tools split.
EMPTY = "an empty list at the container's required collection"

CASES = {
    "conforming": BASE,
    "a null in the container's list of scalars": BASE + "notes: [north, null]\n",
    "an object at the container's scalar slot": BASE + "notes: {a: 1}\n",
    "a wrong-kind value at the container's integer slot": BASE + "count: abc\n",
    "a field the container does not declare": BASE + "extra: 1\n",
    "a bare number at the container's union of classes": BASE + "pick: 42\n",
    "a bare string at the container's list of scalars": BASE + "notes: north\n",
    "a list at the container's single-valued reference": BASE + "featured: [s1]\n",
    "a list at the container's union of classes": BASE + "pick: [s1]\n",
    "a list inside the container's required collection": "title: T\nshelves: [[{id: s1}]]\n",
    "an id at the container's union of classes": BASE + "pick: s1\n",
    "a number at the container's union of classes and integer": BASE + "either: 42\n",
    "a number at the container's union of one class and integer": BASE + "one: 42\n",
    "numbers at the container's list of one class or integer": BASE + "many: [1, 2]\n",
    "a record at the container's union of classes": BASE + "pick: {id: c1, weight: 2}\n",
    "a required slot of the container left out": BASE.replace("title: T\n", ""),
    "a number where the container's required collection belongs": "title: T\nshelves: 7\n",
    EMPTY: "title: T\nshelves: []\n",
}

# The same container declaring an identifier, which the data leaves out.
IDENTIFIED = SCHEMA.replace("      title:", "      id: {identifier: true}\n      title:")


def run(tool: str, *args: str) -> str:
    exe = shutil.which(tool, path=str(Path(sys.executable).parent))
    if exe is None:
        sys.exit(f"{tool} is not installed in this environment")
    r = subprocess.run([exe, *args], capture_output=True, text=True)
    out = (r.stdout.strip() or r.stderr.strip() or "no output").splitlines()
    return " | ".join(out) if r.returncode == 0 else f"exit {r.returncode}: {' | '.join(out)}"


def report(case: str, schema: Path, data: Path, text: str) -> None:
    data.write_text(text)
    print(f"== {case}")
    print(f"   validate: {run('linkml-validate', '-s', str(schema), '-C', 'Depot', str(data))}")
    if case == EMPTY:
        loaded = run("linkml-convert", "-s", str(schema), "-C", "Depot", "-t", "json", str(data))
        # A refusal is a traceback, its last line the message.
        print(f"   convert: {loaded.split(' | ')[-1] if loaded.startswith('exit') else 'loads'}")


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        schema = Path(tmp) / "schema.yaml"
        data = Path(tmp) / "data.yaml"
        schema.write_text(SCHEMA)
        for case, text in CASES.items():
            report(case, schema, data, text)
        schema.write_text(IDENTIFIED)
        report("a container leaving its declared identifier out", schema, data, BASE)


if __name__ == "__main__":
    main()
