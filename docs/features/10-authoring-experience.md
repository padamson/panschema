# Feature: Authoring Experience

**Feature:** Schema / ontology authoring quality-of-life

**User Story:** As a LinkML schema author (or as an ontology developer using LinkML to express OWL/BFO-aligned schemas), I want panschema to surface authoring best practices, catch idiomatic-LinkML mistakes early, and accelerate the round-trip between "edit YAML" and "see rendered docs", so that producing a high-quality, OBO-Foundry-compatible schema is incremental rather than ceremonial.

**Related ADR:** None yet (likely needs one once the linter rule set is fixed — "ADR: Authoring rule taxonomy and severity model")

**Approach:** Vertical Slicing with Outside-In TDD. Slice 1 is dogfood-driven friction gathering — explicitly *not* code — because the rule set should come from observed pain, not invented rules. Subsequent slices implement the rules a user actually hit while authoring a real schema.

---

## Context

A LinkML schema is just a YAML file, which makes the barrier to creating a *valid* schema low and the barrier to creating a *good* schema invisible. Authors learn the difference through trial and error against downstream toolchains (`gen-owl`, `gen-shacl`, `linkml-validate`) and against community conventions that aren't enforced anywhere mechanical.

Panschema is already the producer's "first reader" — when a schema author runs `panschema generate`, the rendered HTML is the first artifact they compare against their mental model. That makes panschema an ideal place to surface authoring guidance that's mechanically checkable: undeclared prefixes, slot-range hops that won't compile, non-idiomatic mixin chains, missing `tree_root`s, inheritance cycles that LinkML would silently flatten, etc.

This feature collects the rule set + the UX for surfacing it.

### Reference material

The friction-gathering slice (slice 1) and the rule-design slices that follow should be grounded in established LinkML / ontology authoring conventions. The three highest-signal sources:

1. **LinkML's authoring documentation** — [linkml.io/linkml/schemas](https://linkml.io/linkml/schemas/index.html). The "Best Practices" subsection and the worked examples are the single most LinkML-specific reference; any gap between idiomatic LinkML and what panschema currently renders / accepts is a candidate rule.
2. **OBO Foundry principles** — [obofoundry.org/principles/fp-000-summary](https://obofoundry.org/principles/fp-000-summary.html). The bio-ontology community's distilled discipline (open IRIs, versioning, modularity, controlled-vocabulary term deprecation, etc.). Companion text: *Building Ontologies with Basic Formal Ontology* (Arp, Smith, Spear, 2015) for the BFO-aligned methodological lens.
3. **Ontology Development 101** — [https://protege.stanford.edu/publications/ontology_development/ontology101.pdf](https://protege.stanford.edu/publications/ontology_development/ontology101.pdf). Noy & McGuinness, Stanford 2001 (free PDF). Outdated on syntax, evergreen on methodology: when is something a class vs a property vs an instance, naming-convention discipline, scope discipline. The canonical free intro.

The intent is *not* to wrap any of these into panschema verbatim — it's to use them as the source of common "this is a known good pattern" / "this is a known anti-pattern" candidates for the rule set.

---

## Vertical Slices

### Slice 1: Friction-gathering pass (no code)

**Status:** Not Started

**User Value:** A documented, prioritized backlog of authoring frictions — the rules that would have caught real mistakes made while authoring a real schema. Without this, every rule we implement is the rule the implementer found memorable, not the rule the author would benefit from.

**Acceptance Criteria:**
- [ ] At least one full authoring pass over [scimantic-schema](https://github.com/padamson/scimantic-schema) (or another real LinkML schema of comparable size) with panschema in the loop. Each "loop iteration" = make a schema edit, run `panschema generate`, view rendered output, capture friction.
- [ ] Each friction logged as a short entry: what the author did, what they expected, what they got, what would have caught it earlier. Captured in a new `docs/authoring-frictions.md` (or a sub-directory) and tagged with severity (annoyance / dead-end / silent-correctness-bug).
- [ ] Cross-reference each friction to its source (LinkML docs / OBO Foundry / Ontology Development 101) — if a friction maps to a documented best practice somewhere, that's the strongest signal for promoting it to a rule.
- [ ] At least 10 frictions logged; the friction list is the input to slice 2's rule design.

**Notes:**
- Doing this without writing code first is a deliberate guard against the implementer-bias failure mode. The temptation will be to skip straight to "implement the validator." Don't.
- Pairs naturally with [feature 07 (Schema Validation)](07-schema-validation.md), which targets the strictly-mechanical metaschema-conformance checks. Feature 10 covers the softer "this isn't *wrong* per the metaschema but it's not what an experienced author would do" layer.

---

### Slice 2: Rule taxonomy and severity model (design, ADR)

**Status:** Not Started

**User Value:** A clear contract for what kinds of rule panschema can express, what severity levels exist (note / warn / error), how rules are categorized (style / correctness / portability / OBO-Foundry-compliance / …), and how they're configurable per-schema via `panschema.toml`.

**Acceptance Criteria:**
- [ ] An ADR (`docs/adr/00X-authoring-rule-taxonomy.md`) capturing the rule categories surfaced by slice 1, the severity model, and the per-rule configuration shape (probably `[authoring.rules.<category>.<id>]` blocks in `panschema.toml`).
- [ ] A first-pass rule registry: each friction from slice 1 has an assigned ID, category, default severity, and a sketch of the implementation approach.
- [ ] At least one rule is implementable in the IR (e.g. via `SchemaDefinition` traversal); at least one is implementable as a static-file lint (raw YAML pass before the IR phase); the ADR documents how panschema dispatches between the two.

**Notes:**
- Models to study: `cargo clippy`'s lint categories + allow/warn/deny, `ruff`'s rule namespacing (`E`, `W`, `F`, `B`, `S`), `mdformat`'s plugin-rule shape. Pick what fits.

---

### Slice 3+: Per-rule implementation

**Status:** Not Started

**User Value:** Each subsequent slice ships one (or one tight family of) rules from the registry, with an actionable diagnostic + the per-rule configuration surface + integration into a new `panschema authoring` subcommand (or `panschema generate --authoring=warn`).

The slice list isn't pinned here because slice 1 generates the input. Expected order of magnitude: 5–15 slices, one rule-family each.

---

## Out of scope

- **Full LSP server / VS Code integration.** A schema-author IDE experience is its own feature — would consume panschema's rule engine but bring its own protocol concerns. Defer.
- **Auto-fixing.** Rules emit diagnostics; they don't rewrite YAML. Auto-fix is a follow-up once the rule set is stable.
- **Schema diff / compatibility hints.** Already on the roadmap as a future v0.5+ feature (`panschema diff`); the authoring-experience feature doesn't try to absorb it.
- **The metaschema-conformance check itself.** That's [feature 07](07-schema-validation.md)'s domain. Feature 10 covers the "stylistic and methodological" layer that sits on top of feature 07's "is this even valid LinkML" layer.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Friction-gathering pass | Must Have | None (real schema to author against) | Not Started |
| Slice 2: Rule taxonomy + ADR | Must Have | Slice 1 | Not Started |
| Slice 3+: Per-rule implementation | Should Have | Slice 2 | Not Started |

**Prerequisite:** A real schema to dogfood. Scimantic-schema is the working assumption; any LinkML schema of comparable size would do.
