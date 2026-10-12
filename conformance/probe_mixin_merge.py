"""Ask the reference implementation which ancestor's slot refinements win.

Run with `uv run --project conformance conformance/probe_mixin_merge.py`.
Each case gives `Thing` a slot from more than one ancestor and prints the
fields `SchemaView.induced_slot` reports beside `class_ancestors`, so the
rule is read off the output: the base is the first ancestor's attribute,
else the top-level slot, and each field takes the value of the first
ancestor in `class_ancestors` order whose `slot_usage` sets it. That order
lists a class's mixins before its `is_a` parent, in discovery order. panschema's
`class_ancestors` and `resolve_effective_slots`
(`panschema-model/src/linkml_resolve.rs`) are checked against this.
"""

from linkml_runtime.utils.schemaview import SchemaView
import linkml_runtime

HEADER = """id: https://example.org/t
name: t
prefixes: {linkml: https://w3id.org/linkml/}
imports: [linkml:types]
default_prefix: https://example.org/t/
default_range: string
slots:
  id: {identifier: true}
  name: {}
  title: {required: true}
"""

CASES = {
    # The measured gap: only the mixin refines the slot.
    "mixin refines, parent silent": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {required: true, pattern: "^[A-Z]"}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # Both set the same field.
    "parent and mixin both set pattern": """
classes:
  Entity:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {pattern: "^parent"}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^mixin", required: true}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # A grandparent sets the field; the direct mixin sets it too.
    "grandparent vs direct mixin": """
classes:
  Base:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {pattern: "^grandparent"}
  Entity: {abstract: true, is_a: Base}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^mixin"}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # The parent's own mixin sets the field; the child's mixin sets it too.
    "parent's mixin vs child's mixin": """
classes:
  ParentMixin:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^parent-mixin"}
  Entity: {abstract: true, slots: [id, name], mixins: [ParentMixin]}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^child-mixin"}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # Two mixins on one class.
    "first mixin vs second mixin": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  First:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^first"}
  Second:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^second", required: true}
  Thing: {is_a: Entity, mixins: [First, Second]}
""",
    # A mixin narrows the range the parent declared.
    "mixin narrows range": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {range: integer}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # The mixin inherits its refinement from its own is_a parent.
    "mixin's own ancestor refines": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  NamedBase:
    mixin: true
    slots: [name]
    slot_usage:
      name: {required: true}
  Named: {mixin: true, is_a: NamedBase}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # The parent and the mixin's own is_a parent both set the field.
    "parent vs mixin's own ancestor": """
classes:
  Entity:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {pattern: "^parent"}
  NamedBase:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^mixin-base"}
  Named: {mixin: true, is_a: NamedBase}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # Each of two mixins brings an ancestor that sets the field.
    "first mixin's ancestor vs second mixin's ancestor": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  FirstBase:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^first-base"}
  First: {mixin: true, is_a: FirstBase}
  SecondBase:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^second-base"}
  Second: {mixin: true, is_a: SecondBase}
  Thing: {is_a: Entity, mixins: [First, Second]}
""",
    # An ancestor's slot_usage above a class that redeclares the slot as an attribute.
    "parent slot_usage over child attribute": """
classes:
  Entity:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {pattern: "^parent", required: true}
  Thing:
    is_a: Entity
    attributes:
      name: {pattern: "^attr"}
""",
    # A slot_usage on a top-level slot no ancestor lists.
    "slot_usage on an unlisted top-level slot": ("""
classes:
  Thing:
    slots: [id, name]
    slot_usage:
      title: {pattern: "^usage"}
""", "title"),
    # The class's own slot_usage outranks both.
    "own slot_usage vs parent and mixin": """
classes:
  Entity:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {pattern: "^parent"}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {pattern: "^mixin"}
  Thing:
    is_a: Entity
    mixins: [Named]
    slot_usage:
      name: {pattern: "^own"}
""",
    # The parent states `required: false`; the mixin states `true`.
    "parent required false vs mixin required true": """
classes:
  Entity:
    abstract: true
    slots: [id, name]
    slot_usage:
      name: {required: false}
  Named:
    mixin: true
    slots: [name]
    slot_usage:
      name: {required: true}
  Thing: {is_a: Entity, mixins: [Named]}
""",
    # The mixin redeclares the slot as an attribute.
    "mixin attribute vs parent slot": """
classes:
  Entity: {abstract: true, slots: [id, name]}
  Named:
    mixin: true
    attributes:
      name: {required: true, pattern: "^attr"}
  Thing: {is_a: Entity, mixins: [Named]}
""",
}

FIELDS = ("range", "required", "pattern")


def main() -> None:
    print("linkml-runtime", linkml_runtime.__version__)
    for name, case in CASES.items():
        body, slot_name = case if isinstance(case, tuple) else (case, "name")
        try:
            view = SchemaView(HEADER + body)
            order = view.class_ancestors("Thing")
            slot = view.induced_slot(slot_name, "Thing")
            fields = {f: getattr(slot, f) for f in FIELDS}
            print(f"{name:46s} -> {fields}  ancestors={order}")
        except Exception as e:  # noqa: BLE001 - the error IS the verdict
            first_line = str(e).split("\n")[0][:110]
            print(f"{name:46s} -> ERROR: {type(e).__name__}: {first_line}")


if __name__ == "__main__":
    main()
