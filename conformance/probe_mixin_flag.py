"""Ask the reference implementation what `mixin: true` means for a class.

Run with `uv run --project conformance conformance/probe_mixin_flag.py`.
The schema has an abstract class, a mixin, and a concrete class using
both. The probe prints every line of each generator's output that names
one of them, so whether the abstract class or the mixin gets a table, a
shape, a definition, or a model of its own is read off the output, and
then asks `linkml-validate` whether it accepts a record typed as each.
panschema's writers and `verify` are checked against this.
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
  t: https://example.org/t/
imports: [linkml:types]
default_prefix: t
default_range: string
slots:
  id: {identifier: true}
  name: {}
  stamp: {range: integer}
classes:
  Entity:
    abstract: true
    slots: [id]
  Stamped:
    mixin: true
    slots: [stamp]
  Parcel:
    is_a: Entity
    mixins: [Stamped]
    slots: [name]
"""

GENERATORS = ["gen-sqlddl", "gen-shacl", "gen-json-schema", "gen-pydantic", "gen-owl"]
RECORDS = {
    "Entity": "id: e1\n",
    "Stamped": "stamp: 2\n",
    "Parcel": "id: r1\nname: crate\nstamp: 2\n",
}
NAMES = list(RECORDS)


def last_line(output: str) -> str:
    lines = output.strip().splitlines()
    return lines[-1][:160] if lines else "(no output)"


def main() -> None:
    bindir = str(Path(sys.executable).parent)
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "schema.yaml"
        path.write_text(SCHEMA)
        for gen in GENERATORS:
            print(f"== {gen}")
            exe = shutil.which(gen, path=bindir)
            if exe is None:
                print("   not installed")
                continue
            r = subprocess.run([exe, str(path)], capture_output=True, text=True)
            if r.returncode != 0:
                print(f"   failed: {last_line(r.stderr or r.stdout)}")
                continue
            for line in r.stdout.splitlines():
                lowered = line.lower()
                if any(n.lower() in lowered for n in NAMES):
                    print(f"   {line.strip()[:160]}")
        print("== linkml-validate")
        validate = shutil.which("linkml-validate", path=bindir)
        if validate is None:
            print("   not installed")
            return
        for name, record in RECORDS.items():
            data = Path(tmp) / f"{name}.yaml"
            data.write_text(record)
            r = subprocess.run(
                [validate, "-s", str(path), "-C", name, str(data)],
                capture_output=True,
                text=True,
            )
            print(f"   {name}: exit {r.returncode}, {last_line(r.stdout + r.stderr)}")


if __name__ == "__main__":
    main()
