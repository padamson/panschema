"""Ask linkml-runtime how it reads each annotation spelling.

Run with `uv run --project conformance conformance/probe_annotations.py`.
The output is the reference implementation's verdict on each case; when
panschema's reader and this disagree, the reader is presumed wrong (one
deliberate divergence is documented on `Annotations` in linkml.rs:
panschema reads bare scalar values lexically as strings).
"""

from linkml_runtime.utils.schemaview import SchemaView
import linkml_runtime

PREAMBLE = """id: https://example.org/t
name: t
prefixes: {linkml: https://w3id.org/linkml/}
default_range: string
slots:
  s1:
    annotations:
"""

CASES = {
    "compact scalar": "      note: hello\n",
    "bare structured": "      review_status:\n        stage: draft\n        priority: 2\n",
    "expanded map": "      note:\n        tag: note\n        value: hello\n",
    "expanded list": "      - tag: note\n        value: hello\n",
    "value-wrapped struct": "      review_status:\n        value:\n          stage: draft\n",
    "value+sibling keys": "      note:\n        value: something\n        stage: draft\n",
    "nested annotations": "      note:\n        value: hello\n        annotations:\n          provenance: curated\n",
    "tag mismatch": "      note:\n        tag: elsewhere\n        value: hello\n",
    "map missing value": "      note:\n        tag: note\n",
    "list missing value": "      - tag: note\n",
    "bare sequence": "      note:\n        - a\n        - b\n",
    "numeric scalar": "      note: 2024\n",
    "bool scalar": "      note: true\n",
}


def main() -> None:
    print("linkml-runtime", linkml_runtime.__version__)
    for name, body in CASES.items():
        try:
            view = SchemaView(PREAMBLE + body)
            annotations = view.get_slot("s1").annotations
            verdict = {tag: repr(a.value) for tag, a in annotations.items()}
            print(f"{name:22s} -> {verdict}")
        except Exception as e:  # noqa: BLE001 - the error IS the verdict
            first_line = str(e).split("\n")[0][:110]
            print(f"{name:22s} -> ERROR: {type(e).__name__}: {first_line}")


if __name__ == "__main__":
    main()
