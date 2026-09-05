# panschema-model

The LinkML model that [panschema](https://github.com/padamson/panschema)
reads and writes, as a library on its own. It carries the schema model
(`SchemaDefinition`, `ClassDefinition`, `SlotDefinition`, and the rest of
the LinkML metamodel panschema covers), the resolution of `is_a`, mixins,
and `slot_usage` into a class's effective slots, the LinkML YAML reader,
and the `Reader` trait a format reader implements.

It depends on no RDF, template, CLI, or async library. A tool that
reasons over schemas takes this crate and skips the rest of panschema.

```rust
use std::path::Path;

use panschema_model::io::Reader;
use panschema_model::linkml_resolve::resolve_effective_slots;
use panschema_model::yaml_reader::YamlReader;

let schema = YamlReader::new().read(Path::new("schema.yaml"))?;
let person = &schema.classes["Person"];
for (name, slot) in resolve_effective_slots(person, &schema) {
    println!("{name}: {:?}", slot.range);
}
```

`panschema` re-exports every module here at its previous path, so code
written against `panschema::linkml` keeps working.
