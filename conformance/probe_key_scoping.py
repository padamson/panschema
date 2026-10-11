"""Ask the reference implementation which forms of per-class id scoping load.

Run with `uv run --project conformance conformance/probe_key_scoping.py`.
A schema that shares one `id` slot across its record classes scopes some
of them to their dataset (`key`) and leaves others global (`identifier`).
Each case below spells that split a different way; the output is whether
`SchemaLoader` resolves it, the flags `SchemaView` induces per class, which
generators run, and whether `linkml-validate` accepts a dataset holding
one scoped record and one shared record it references. panschema refuses
at load every case where `SchemaView` reports both flags (see
`identifier_key_conflicts` in `panschema-model/src/diagnostics.rs`).
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from linkml.validator import validate
from linkml_runtime.utils.schemaview import SchemaView

HEADER = """id: https://example.org/t
name: t
prefixes: {linkml: https://w3id.org/linkml/}
imports: [linkml:types]
default_prefix: https://example.org/t/
default_range: string
"""

COLLECTIONS = """\
  shared: {range: Shared, multivalued: true, inlined_as_list: true}
  records: {range: Scoped, multivalued: true, inlined_as_list: true}
  uses: {range: Shared}
"""

# Each case: the schema body, and the slot naming a scoped record.
CASES = {
    # The rule the CHANGELOG documented: a global identifier, narrowed per class.
    "identifier on the slot, key through slot_usage": (
        """
slots:
  id: {identifier: true}
{collections}classes:
  Root:
    tree_root: true
    slots: [id, shared, records]
  Shared:
    slots: [id]
  Scoped:
    slots: [id, uses]
    slot_usage:
      id: {key: true}
""",
        "id",
    ),
    # The inverse: a global key, promoted per class.
    "key on the slot, identifier through slot_usage": (
        """
slots:
  id: {key: true}
{collections}classes:
  Root:
    tree_root: true
    slots: [id, shared, records]
    slot_usage:
      id: {identifier: true, key: false}
  Shared:
    slots: [id]
    slot_usage:
      id: {identifier: true, key: false}
  Scoped:
    slots: [id, uses]
""",
        "id",
    ),
    # Neither flag on the shared slot; each class sets exactly one.
    "neither on the slot, one per class through slot_usage": (
        """
slots:
  id: {required: true}
{collections}classes:
  Root:
    tree_root: true
    slots: [id, shared, records]
    slot_usage:
      id: {identifier: true}
  Shared:
    slots: [id]
    slot_usage:
      id: {identifier: true}
  Scoped:
    slots: [id, uses]
    slot_usage:
      id: {key: true}
""",
        "id",
    ),
    # The same, with the flag carried by an abstract parent each class inherits.
    "neither on the slot, one per abstract parent": (
        """
slots:
  id: {required: true}
{collections}classes:
  Entity:
    abstract: true
    slots: [id]
    slot_usage:
      id: {identifier: true}
  ScopedRecord:
    abstract: true
    slots: [id]
    slot_usage:
      id: {key: true}
  Root:
    is_a: Entity
    tree_root: true
    slots: [shared, records]
  Shared:
    is_a: Entity
  Scoped:
    is_a: ScopedRecord
    slots: [uses]
""",
        "id",
    ),
    # The same, carried by mixins.
    "neither on the slot, one per mixin": (
        """
slots:
  id: {required: true}
{collections}classes:
  Identified:
    mixin: true
    slots: [id]
    slot_usage:
      id: {identifier: true}
  Keyed:
    mixin: true
    slots: [id]
    slot_usage:
      id: {key: true}
  Root:
    mixins: [Identified]
    tree_root: true
    slots: [shared, records]
  Shared:
    mixins: [Identified]
  Scoped:
    mixins: [Keyed]
    slots: [uses]
""",
        "id",
    ),
    # A child narrowing the identifier its parent set through slot_usage.
    "identifier on a parent's slot_usage, key on the child's": (
        """
slots:
  id: {required: true}
{collections}classes:
  Entity:
    abstract: true
    slots: [id]
    slot_usage:
      id: {identifier: true}
  Root:
    is_a: Entity
    tree_root: true
    slots: [shared, records]
  Shared:
    is_a: Entity
  Scoped:
    is_a: Entity
    slots: [uses]
    slot_usage:
      id: {key: true}
""",
        "id",
    ),
    # A mixin's key beside an identifier inherited through is_a.
    "identifier on the is_a parent, key on a mixin": (
        """
slots:
  id: {required: true}
{collections}classes:
  Entity:
    abstract: true
    slots: [id]
    slot_usage:
      id: {identifier: true}
  Keyed:
    mixin: true
    slots: [id]
    slot_usage:
      id: {key: true}
  Root:
    is_a: Entity
    tree_root: true
    slots: [shared, records]
  Shared:
    is_a: Entity
  Scoped:
    is_a: Entity
    mixins: [Keyed]
    slots: [uses]
""",
        "id",
    ),
    # Two slots: a global identifier and a separate key.
    "separate identifier and key slots": (
        """
slots:
  id: {identifier: true}
  local_id: {key: true}
{collections}classes:
  Root:
    tree_root: true
    slots: [id, shared, records]
  Shared:
    slots: [id]
  Scoped:
    slots: [local_id, uses]
""",
        "local_id",
    ),
}

GENERATORS = [
    "gen-python",
    "gen-rdf",
    "gen-jsonld",
    "gen-jsonld-context",
    "gen-shex",
    "gen-graphql",
    "gen-owl",
    "gen-pydantic",
    "gen-json-schema",
    "gen-shacl",
    "gen-sqltables",
]


def data_for(scoped_id: str) -> dict:
    return {
        "id": "root",
        "shared": [{"id": "s1"}],
        "records": [{scoped_id: "k1", "uses": "s1"}],
    }


def loads(path: Path) -> str:
    from linkml.utils.schemaloader import SchemaLoader

    try:
        SchemaLoader(str(path)).resolve()
        return "ok"
    except Exception as exc:  # the loader raises bare ValueErrors
        return f"FAIL: {str(exc).splitlines()[-1][:110]}"


def induced(view: SchemaView) -> str:
    out = []
    for cls in ("Root", "Shared", "Scoped"):
        for name in ("id", "local_id"):
            if name in view.class_slots(cls):
                s = view.induced_slot(name, cls)
                out.append(f"{cls}.{name}: identifier={bool(s.identifier)} key={bool(s.key)}")
    return "; ".join(out)


def generators(path: Path) -> str:
    failed, missing = [], []
    bindir = str(Path(sys.executable).parent)
    for gen in GENERATORS:
        exe = shutil.which(gen, path=bindir)
        if exe is None:
            missing.append(gen)
            continue
        r = subprocess.run([exe, str(path)], capture_output=True, text=True)
        if r.returncode != 0:
            failed.append(gen)
    out = "all ran" if not failed else "failed: " + ", ".join(failed)
    return out if not missing else f"{out} (not installed: {', '.join(missing)})"


def validates(path: Path, scoped_id: str) -> str:
    try:
        report = validate(data_for(scoped_id), str(path), "Root")
    except Exception as exc:
        return f"error: {str(exc).splitlines()[-1][:110]}"
    if not report.results:
        return "valid"
    return "invalid: " + " | ".join(r.message[:110] for r in report.results)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        for case, (body, scoped_id) in CASES.items():
            path = Path(tmp) / "schema.yaml"
            path.write_text(HEADER + body.replace("{collections}", COLLECTIONS))
            print(f"== {case}")
            print(f"   SchemaLoader: {loads(path)}")
            try:
                print(f"   SchemaView:   {induced(SchemaView(str(path)))}")
            except Exception as exc:
                print(f"   SchemaView:   FAIL: {exc}")
            print(f"   generators:   {generators(path)}")
            print(f"   validate:     {validates(path, scoped_id)}")


if __name__ == "__main__":
    main()
