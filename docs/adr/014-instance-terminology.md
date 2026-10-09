# ADR-014: "instance" is the user-facing term for a member of a dataset

## Status

Accepted (2026-10-09)

## Context

The Instances block of the HTML page named one thing four ways. The block
heading said **Instances**, the cards section and its sidebar entry said
**Individuals**, each card's badge said **Individual**, and the dataset's
metadata card counted **Records**. The instance graph's legend and hover
said **Individual** as well. A reader moving down the block met four words
and had no way to tell they meant the same thing.

Each word is correct somewhere. LinkML calls data that conforms to a schema
*instance data*, and each object in it an *instance* of its class. OWL
calls a member of the A-box an *individual*, and panschema's RDF output
writes `owl:NamedIndividual`. *Record* is no metamodel's term; it is the
everyday word for an entry in a data file.

Two other leftovers from the OWL side read wrong on a LinkML page. Every
schema section said "defined in this ontology", whatever format the schema
came from, and the instance graph's caption called its edges
"object-property assertions", the OWL term ADR-006 replaced with "slot".

## Decision

**Use "instance" for a member of a dataset everywhere a reader sees it**,
matching LinkML and panschema's instance model (`Instance`, `InstanceSet`).
That covers the card badge, the metadata card's count, and the instance
graph's legend and hover card. The block heading was already "Instances";
the cards section and its sidebar entry under it read "All instances", so
the child does not repeat its group's label.

The rest of the vocabulary, for reference:

- **schema** is what panschema documents, whatever format it was read
  from. "Ontology" is for OWL input in prose that is about OWL ("generate
  documentation from an OWL ontology"), not page copy.
- **individual** stays where the output is OWL: the RDF writers emit
  `owl:NamedIndividual`, and the OWL reader reads individuals.
- **record** is fine in prose about a data file's entries, but it is not a
  label on the page.
- **dataset** is one data file's instances, or the individuals embedded in
  an OWL schema.
- **T-box** and **A-box** stay as glosses on the Schema and Instances
  blocks. They name which half of the page is which; they do not describe
  `verify`, which checks a closed world where description logic assumes an
  open one.
- **edges** in the instance graph are slot assertions: a slot whose value
  is another instance.

**Identifiers keep "individual"**, which departs from ADR-006:

- The section anchors `#individuals` and `#instance-individuals` and the
  card anchors `#ind-<id>` are published deep links, which feature 51
  deliberately kept.
- The instance graph JSON writes `node_type: "individual"` and node ids
  `individual:<id>`, and `--format instance-graph-json` exports that
  document for other tools to read.
- Renaming the HTML writer's internals (`IndividualData`,
  `individual_card.html`, `.individual-badge`, `--color-individual`) would
  not remove the second word from the code, since the anchors and the wire
  format keep it. It would only move the boundary. Inside the code
  "individual" reads as the A-box term it is.

## Consequences

- The page and the graph use one word for a member of a dataset, and one
  word for the thing being documented.
- No deep link breaks and no exported format changes. Tests that match the
  page's copy change with it.
- A contributor sees "Instance" in the output and "individual" in the
  HTML writer and the graph JSON. This ADR is the map between the two.
