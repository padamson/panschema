"""Ask the reference implementation what it does with a declared metaslot.

Run with `uv run --project conformance conformance/probe_metamodel_extension.py`.
`gen-owl` and `gen-shacl` fetch from `w3id.org`, so run it with network access.
`conformance/README.md` gives the command that runs it against LinkML's `main`.

One declaration, a count bound on `budget` naming the slot it counts, is
spelled each way LinkML offers for putting tool-specific meaning on a
slot: the `annotations:` form panschema reads today (bare, under a
declared prefix, and under an undeclared one), `extensions:`, the
documented `instantiates:` form (a metaclass in an imported schema whose
attributes are the annotation tags; the same metaclass not imported; a
metaclass that does not exist), the same with a structured value, and a
bare metaslot key. Each spelling is also given a typo'd or missing
field, and is declared once on the slot and once in a class's
`slot_usage`.

Every tool also runs on a control, the same schema with no declaration,
and each verdict is what the spelling changes against that control:
`SchemaView`'s load and what `induced_slot` hands a subclass, the
`InstantiatesValidationPlugin`'s findings, `linkml-lint`'s problems,
`linkml-validate`'s metamodel check, the triples `gen-owl` and
`gen-shacl --include-annotations` add or remove (or that the output is
not parseable RDF), and the paths `gen-json-schema` changes. Last, it
validates an annotation's value as data against the metaclass, the
check a published schema of the value shapes would give. It is the
evidence behind the spelling of feature 44's schema-declared slot
semantics.
"""

import copy
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import linkml_runtime
import yaml
from jsonasobj2 import as_dict
from linkml.validator.plugins.instantiates_validation_plugin import (
    check_instantiates_constraints,
)
from linkml_runtime.utils.schemaview import SchemaView
from rdflib import BNode, Graph, Literal, URIRef
from rdflib.namespace import SH

META = {
    "id": "https://example.org/pm",
    "name": "pm",
    "prefixes": {"linkml": "https://w3id.org/linkml/", "pm": "https://example.org/pm/"},
    "imports": ["linkml:types"],
    "default_prefix": "pm",
    "default_range": "string",
    "classes": {
        "CountBound": {"attributes": {"counted_slot": {"required": True}}},
        "StructuredCountBound": {
            "attributes": {"bounds_count_of": {"range": "CountBound", "inlined": True, "required": True}}
        },
    },
}

MAIN = {
    "id": "https://example.org/t",
    "name": "t",
    "prefixes": {
        "linkml": "https://w3id.org/linkml/",
        "pm": "https://example.org/pm/",
        "t": "https://example.org/t/",
    },
    "imports": ["linkml:types", "pm"],
    "default_prefix": "t",
    "default_range": "string",
    "slots": {"anchors": {"multivalued": True}, "budget": {"range": "integer"}},
    "classes": {"Benchmark": {"slots": ["anchors", "budget"]}, "Sub": {"is_a": "Benchmark"}},
}


def declared(key: str, tag: str, field: str = "counted_slot") -> dict:
    return {key: {tag: {"value": {field: "anchors"}}}}


def instantiating(metaclass: str, annotations: dict | None = None) -> dict:
    body = {"instantiates": [metaclass]}
    if annotations:
        body["annotations"] = annotations
    return body


# name -> (body on budget, run the generators, schema imports pm); the
# generators read no field, so a typo'd or missing one is not run through them
SPELLINGS = {
    "annotations": (declared("annotations", "bounds_count_of"), True, True),
    "annotations, typo": (declared("annotations", "bounds_count_of", "countd_slot"), False, True),
    "annotations, declared prefix": (declared("annotations", "pm:bounds_count_of"), True, True),
    "annotations, undeclared prefix": (declared("annotations", "px:bounds_count_of"), True, True),
    "extensions": (declared("extensions", "bounds_count_of"), True, True),
    "instantiates": (instantiating("pm:CountBound", {"counted_slot": "anchors"}), True, True),
    "instantiates, typo": (instantiating("pm:CountBound", {"countd_slot": "anchors"}), False, True),
    "instantiates, missing": (instantiating("pm:CountBound"), False, True),
    "instantiates, not imported": (instantiating("pm:CountBound", {"counted_slot": "anchors"}), True, False),
    "instantiates, undefined metaclass": (instantiating("pm:Undefined", {"counted_slot": "anchors"}), True, True),
    "instantiates, structured": (
        instantiating("pm:StructuredCountBound", declared("annotations", "bounds_count_of")["annotations"]),
        True,
        True,
    ),
    "instantiates, structured typo": (
        instantiating(
            "pm:StructuredCountBound", declared("annotations", "bounds_count_of", "countd_slot")["annotations"]
        ),
        False,
        True,
    ),
    "bare metaslot": ({"bounds_count_of": {"counted_slot": "anchors"}}, True, True),
}

BUDGET = URIRef("https://example.org/t/budget")

GENERATORS = (["gen-owl"], ["gen-shacl", "--include-annotations"], ["gen-json-schema"])


def schema_with(body: dict, where: str, imports_pm: bool = True) -> dict:
    schema = copy.deepcopy(MAIN)
    if not imports_pm:
        schema["imports"] = ["linkml:types"]
    if where == "slot":
        schema["slots"]["budget"].update(body)
    elif where == "slot_usage":
        schema["classes"]["Benchmark"]["slot_usage"] = {"budget": body}
    return schema


def run(bindir: str, args: list[str]) -> subprocess.CompletedProcess:
    exe = shutil.which(args[0], path=bindir)
    if exe is None:
        sys.exit(f"{args[0]} is not installed in {bindir}")
    return subprocess.run([exe, *args[1:]], capture_output=True, text=True)


def tag_values(collection) -> dict:
    return {tag: entry["value"] for tag, entry in as_dict(collection or {}).items()}


def load(path: Path) -> dict:
    """What the loader, `induced_slot` on a subclass, and the plugin make of the schema."""
    try:
        view = SchemaView(str(path))
        slot = view.induced_slot("budget", "Sub")
        return {
            "load": "ok",
            "Sub sees annotations": tag_values(slot.annotations),
            "Sub sees extensions": tag_values(slot.extensions),
            "Sub sees instantiates": [str(i) for i in slot.instantiates or []],
            "instantiates plugin": [r.message for r in check_instantiates_constraints(view)],
        }
    except Exception as e:  # noqa: BLE001 - the error IS the verdict
        return {"load": f"ERROR {type(e).__name__}: {str(e).splitlines()[0][:120]}"}


def lint(bindir: str, path: Path) -> set:
    r = run(bindir, ["linkml-lint", "-f", "json", str(path)])
    try:
        problems = json.loads(r.stdout or "[]")
    except json.JSONDecodeError:
        return {("unparseable output", r.returncode)}
    return {(p["level"], p["rule_name"], p["message"].replace(str(path), "<schema>")) for p in problems}


def validate(bindir: str, path: Path) -> tuple:
    r = run(bindir, ["linkml-validate", str(path)])
    lines = (r.stdout + r.stderr).replace(str(path), "<schema>").splitlines()
    return r.returncode, tuple(line.strip()[:150] for line in lines if line.startswith(("[ERROR]", "[WARN"))), r


def rdf(text: str):
    try:
        return Graph().parse(data=text, format="turtle")
    except Exception as e:  # noqa: BLE001 - unparseable output IS the verdict
        return f"not parseable as Turtle: {type(e).__name__}: {str(e).splitlines()[0][:100]}"


def short(term) -> str:
    if isinstance(term, Literal) and term.datatype is not None:
        return f"{term!s:.60}^^{term.datatype.split('#')[-1]}"
    if isinstance(term, Literal):
        return repr(f"{term!s:.60}")
    text = str(term)
    for base, prefix in (("https://example.org/t/", "t:"), ("https://example.org/pm/", "pm:"), (str(SH), "sh:")):
        if text.startswith(base):
            return prefix + text[len(base) :]
    return text


def described(graph: Graph, node) -> frozenset:
    """A node's triples as (predicate, object) pairs, a blank object replaced by its own description."""
    pairs = set()
    for p, o in graph.predicate_objects(node):
        kind = " (literal predicate)" if isinstance(p, Literal) else ""
        value = described(graph, o) if isinstance(o, BNode) else short(o)
        pairs.add((short(p) + kind, value))
    return frozenset(pairs)


def on_budget(graph: Graph) -> frozenset:
    """What an output says about `budget`: the OWL property, or every SHACL property shape on it."""
    shapes = [s for s in graph.subjects(SH.path, BUDGET)]
    if shapes:
        return frozenset().union(*(described(graph, s) for s in shapes)) - {("sh:path", "t:budget")}
    return described(graph, BUDGET)


def rdf_change(control, graph) -> str:
    if isinstance(graph, str):
        return graph
    base, now = on_budget(control), on_budget(graph)
    lines = [f"+ {p} {o}" for p, o in sorted(now - base, key=str)]
    lines += [f"- {p} {o}" for p, o in sorted(base - now, key=str)]
    return "; ".join(line[:150] for line in lines) or "nothing"


def json_change(control, document, path: str = "") -> list[str]:
    if isinstance(control, dict) and isinstance(document, dict):
        changes = []
        for key in sorted(set(control) | set(document)):
            if key not in document:
                changes.append(f"- {path}/{key}")
            elif key not in control:
                changes.append(f"+ {path}/{key} = {json.dumps(document[key])[:100]}")
            else:
                changes += json_change(control[key], document[key], f"{path}/{key}")
        return changes
    return [] if control == document else [f"~ {path}: {json.dumps(document)[:100]}"]


def generate(bindir: str, path: Path) -> dict:
    out = {}
    for gen in GENERATORS:
        r = run(bindir, [*gen, str(path)])
        if r.returncode != 0:
            out[gen[0]] = f"failed: {r.stderr.strip().splitlines()[-1][:120] if r.stderr.strip() else r.returncode}"
        elif gen[0] == "gen-json-schema":
            out[gen[0]] = json.loads(r.stdout)
        else:
            out[gen[0]] = rdf(r.stdout)
    return out


def report_generators(control: dict, current: dict) -> None:
    for gen in GENERATORS:
        name = gen[0]
        base, now = control[name], current[name]
        if isinstance(now, str) and now.startswith("failed"):
            print(f"   {name}: {now}")
        elif name == "gen-json-schema":
            print(f"   {name}: {'; '.join(json_change(base, now)) or 'nothing'}")
        else:
            print(f"   {name}: {rdf_change(base, now)}")


def write(tmp: Path, schema: dict) -> Path:
    path = tmp / "t.yaml"
    path.write_text(yaml.safe_dump(schema, sort_keys=False))
    return path


def main() -> None:
    print("linkml-runtime", linkml_runtime.__version__)
    bindir = str(Path(sys.executable).parent)
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        (tmp_path / "pm.yaml").write_text(yaml.safe_dump(META, sort_keys=False))
        path = write(tmp_path, MAIN)
        control = {
            "load": load(path),
            "lint": lint(bindir, path),
            "validate": validate(bindir, path)[:2],
            "generators": generate(bindir, path),
        }
        failed = {name: out for name, out in control["generators"].items() if isinstance(out, str)}
        if failed:
            sys.exit(f"the control schema does not generate, so nothing can be compared: {failed}")
        print(f"== control: load {control['load']['load']}, validate exit {control['validate'][0]}")
        for name, (body, run_generators, imports_pm) in SPELLINGS.items():
            for where in ("slot", "slot_usage"):
                path = write(tmp_path, schema_with(body, where, imports_pm))
                print(f"== {name} (on {where})")
                loaded = load(path)
                changed = {k: v for k, v in loaded.items() if control["load"].get(k) != v}
                print(f"   load: {changed or 'as control'}")
                added = lint(bindir, path) - control["lint"]
                print(f"   linkml-lint adds: {sorted(added) or 'nothing'}")
                exit_code, messages, _ = validate(bindir, path)
                print(f"   linkml-validate (metamodel): exit {exit_code}, {list(messages) or 'no messages'}")
                if where == "slot" and run_generators:
                    report_generators(control["generators"], generate(bindir, path))
        print("== annotation value checked as data against the metaclass")
        for name, value in (("well-formed", {"counted_slot": "anchors"}), ("typo", {"countd_slot": "anchors"})):
            data = tmp_path / "value.yaml"
            data.write_text(yaml.safe_dump(value))
            r = run(bindir, ["linkml-validate", "-s", str(tmp_path / "pm.yaml"), "-C", "CountBound", str(data)])
            errors = [line.strip()[:150] for line in r.stdout.splitlines() if line.startswith("[ERROR]")]
            print(f"   {name}: exit {r.returncode}, {errors or 'no errors'}")


if __name__ == "__main__":
    main()
