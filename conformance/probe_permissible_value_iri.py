"""Ask the reference implementation what IRI each schema element gets.

Run with `uv run --project conformance conformance/probe_permissible_value_iri.py`.
A schema whose class, slot, and enum names are not already in LinkML's
conventional casing, and whose enum values contain a space, a slash, a
non-ASCII letter, or a `meaning:`, goes through `gen-owl`; the probe prints
the IRI minted for each element. It runs twice: once under a `/`-ending
default prefix and once under a `#`-ending one. The measured rules:

- a class or enum without a declared URI is `{default_prefix}:{camelcase(name)}`,
  a slot `{default_prefix}:{underscore(name)}`; a declared `enum_uri` wins;
- a value without `meaning` is `{enum IRI}#{quote(text, safe="")}`, and
  `meaning` wins;
- under a `#` namespace that value IRI carries a second `#`, which is not a
  valid IRI (RFC 3987 allows none in a fragment).

`gen-shacl` lists enum values in `sh:in` as literals, and `rdflib_dumper`
writes an enum-valued instance slot as a literal; both are printed for
reference. panschema's element-identity functions
(`panschema-model/src/linkml_resolve.rs`) mint these IRIs, except that a
value under an enum IRI that already has a fragment takes `/` instead of a
second `#`.
"""

import tempfile
from pathlib import Path

from linkml.generators.owlgen import OwlSchemaGenerator
from linkml.generators.pythongen import PythonGenerator
from linkml.generators.shaclgen import ShaclGenerator
from linkml_runtime.dumpers import rdflib_dumper
from linkml_runtime.utils.schemaview import SchemaView
from rdflib import Graph
from rdflib.namespace import OWL, RDF, RDFS, SH

SCHEMA = """id: https://example.org/t
name: t
prefixes:
  linkml: https://w3id.org/linkml/
  ex: {namespace}
  skos: http://www.w3.org/2004/02/skos/core#
imports: [linkml:types]
default_prefix: ex
default_range: string
enums:
  item_status:
    permissible_values:
      planned: {{}}
      in progress: {{}}
      a/b: {{}}
      café: {{}}
      done:
        meaning: skos:Concept
  declared:
    enum_uri: skos:DeclaredScheme
    permissible_values:
      only: {{}}
classes:
  person_record:
    slots: [id, has owner, in-stock, status, declared]
slots:
  id: {{identifier: true}}
  has owner: {{}}
  in-stock: {{}}
  status: {{range: item_status}}
  declared: {{range: declared}}
"""


def write_schema(tmp: str, namespace: str) -> Path:
    path = Path(tmp) / "t.yaml"
    path.write_text(SCHEMA.format(namespace=namespace))
    return path


def main() -> None:
    for namespace in ["https://example.org/t/", "https://example.org/t#"]:
        with tempfile.TemporaryDirectory() as tmp:
            path = write_schema(tmp, namespace)
            owl = Graph().parse(data=OwlSchemaGenerator(str(path)).serialize(), format="turtle")
            print(f"gen-owl under default prefix {namespace}")
            for kind in [OWL.Class, OWL.ObjectProperty, OWL.DatatypeProperty]:
                for subject in sorted(owl.subjects(RDF.type, kind)):
                    if str(subject).startswith("https://example.org/") or "Declared" in str(subject):
                        label = owl.value(subject, RDFS.label)
                        print(f"  {kind.n3(owl.namespace_manager):22} {subject}  label={label}")

    with tempfile.TemporaryDirectory() as tmp:
        path = write_schema(tmp, "https://example.org/t/")
        shacl = Graph().parse(data=ShaclGenerator(str(path)).serialize(), format="turtle")
        print("gen-shacl: sh:in members on the enum-ranged slots")
        for node in shacl.objects(None, SH["in"]):
            print(f"  {[str(member) for member in shacl.items(node)]}")

        view = SchemaView(str(path))
        module = PythonGenerator(str(path)).compile_module()
        item = module.PersonRecord(id="ex:item1", status="in progress")
        data = Graph().parse(data=rdflib_dumper.dumps(item, view), format="turtle")
        print("rdflib_dumper: the status object for a record whose status is `in progress`")
        for _, predicate, obj in data:
            if str(predicate).endswith("status"):
                print(f"  {obj!r}")


if __name__ == "__main__":
    main()
