"""Ask the reference implementation what merging a schema's imports does.

Run with `uv run --project conformance conformance/probe_merge_imports.py`.
A main schema imports `lib`, which imports `deep_a` and `deep_b`; the files
collide on purpose. Each line compares three readings: `gen-linkml
--mergeimports` (its YAML, reloaded standalone), `SchemaView.merge_imports`
(in memory), and `SchemaView` over the unmerged files, which is what
`gen-owl` projects. It is the evidence behind how panschema's loader
(`panschema-model/src/import_resolve.rs`) mints IRIs and fills ranges for
imported elements.
"""

import subprocess
import tempfile
from pathlib import Path

import linkml_runtime
import yaml
from linkml_runtime.utils.schemaview import SchemaView

FILES = {
    "main.yaml": """
id: https://example.org/main
name: main
prefixes:
  linkml: https://w3id.org/linkml/
  main: https://example.org/main/
  shared: https://example.org/main-says-shared/
default_prefix: main
default_range: string
imports: [linkml:types, sub/lib]
classes:
  Collide: {description: main's Collide}
enums:
  Color: {permissible_values: {red: {}}}
""",
    "sub/lib.yaml": """
id: https://example.org/lib
name: lib
prefixes:
  linkml: https://w3id.org/linkml/
  lib: https://example.org/lib/
  shared: https://example.org/lib-says-shared/
  onlylib: https://example.org/onlylib/
default_prefix: lib
default_range: integer
imports: [linkml:types, deep_a, deep_b]
subsets:
  extra: {}
classes:
  Collide: {description: lib's Collide, slots: [count]}
  LibThing:
    in_subset: [extra]
    comments: [a comment]
    slots: [count, rangeless]
    any_of: [{slot_conditions: {count: {minimum_value: 1}}}]
slots:
  count: {range: integer}
  rangeless: {}
enums:
  Color: {permissible_values: {blue: {}}}
""",
    "sub/deep_a.yaml": """
id: https://example.org/deep_a
name: deep_a
prefixes: {linkml: https://w3id.org/linkml/, deep_a: https://example.org/deep_a/}
default_prefix: deep_a
imports: [linkml:types]
classes:
  Deep: {description: deep_a's Deep}
""",
    "sub/deep_b.yaml": """
id: https://example.org/deep_b
name: deep_b
prefixes: {linkml: https://w3id.org/linkml/, deep_b: https://example.org/deep_b/}
default_prefix: deep_b
imports: [linkml:types]
classes:
  Deep: {description: deep_b's Deep}
""",
}


def gen_linkml_merged(main: Path) -> str:
    return subprocess.run(
        ["gen-linkml", "--mergeimports", "--no-materialize", "-f", "yaml", str(main)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout


def readings(main: Path, merged_file: Path) -> dict[str, SchemaView]:
    in_memory = SchemaView(str(main))
    in_memory.merge_imports()
    return {
        "unmerged": SchemaView(str(main)),
        "merge_imports": in_memory,
        "gen-linkml": SchemaView(str(merged_file)),
    }


def row(label: str, values: dict[str, object]) -> None:
    cells = "  ".join(f"{k}={v!s:42}" for k, v in values.items())
    print(f"{label:34s} {cells}")


def main() -> None:
    print("linkml-runtime", linkml_runtime.__version__)
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for name, text in FILES.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        main_file = root / "main.yaml"
        merged_text = gen_linkml_merged(main_file)
        merged_file = root / "merged.yaml"
        merged_file.write_text(merged_text)
        merged_doc = yaml.safe_load(merged_text)
        views = readings(main_file, merged_file)

        def each(f):
            out = {}
            for k, v in views.items():
                try:
                    out[k] = f(v)
                except Exception as e:  # noqa: BLE001 - the error IS the verdict
                    out[k] = f"ERROR {type(e).__name__}"
            return out

        print("\n-- collisions")
        row("main vs import: Collide", each(lambda v: v.get_class("Collide").description))
        row("main vs import: Collide slots", each(lambda v: v.get_class("Collide").slots))
        row("main vs import: Color values", each(lambda v: list(v.get_enum("Color").permissible_values)))
        row("import vs import: Deep", each(lambda v: v.get_class("Deep").description))
        row("prefix both bind: shared", each(lambda v: v.schema.prefixes["shared"].prefix_reference))
        row("prefix only import: onlylib", each(lambda v: "onlylib" in v.schema.prefixes))

        print("\n-- provenance")
        row("LibThing from_schema", each(lambda v: v.get_class("LibThing").from_schema))
        row("extra subset from_schema", each(lambda v: v.get_subset("extra").from_schema))
        print("gen-linkml output keys:", [k for k in merged_doc if k not in ("classes", "slots", "enums", "types", "subsets", "prefixes")])

        print("\n-- linkml:types")
        print("gen-linkml output `imports`:", merged_doc.get("imports"))
        print("gen-linkml output defines `string`:", "string" in merged_doc.get("types", {}))

        print("\n-- meaning of an imported element")
        row("LibThing IRI", each(lambda v: v.get_uri("LibThing", expand=True)))
        row("count IRI", each(lambda v: v.get_uri("count", expand=True)))
        row("rangeless slot's range", each(lambda v: v.induced_slot("rangeless", "LibThing").range))

        print("\n-- constructs outside panschema's IR")
        lib_thing = merged_doc["classes"]["LibThing"]
        print("gen-linkml keeps comments/in_subset/any_of:", [k in lib_thing for k in ("comments", "in_subset", "any_of")])
        print("gen-linkml adds per element:", sorted(set(lib_thing) - {"comments", "in_subset", "any_of", "slots"}))


if __name__ == "__main__":
    main()
