# Feature 40: An Agent-Facing Skill That Cannot Drift

**Feature:** Ship a Claude Code skill with panschema, and hold it true with
tests rather than discipline — the documented examples are executed by CI,
and the code's own enumerations assert their own coverage in the reference.

**User Story:** As someone starting a coding-agent session in a repo that
*consumes* panschema, I want the agent to reach for panschema correctly on
the first try — right subcommand, valid manifest, correct key names —
instead of guessing from a README it may not read or source it cannot see.

**Related:** the fleet practice this implements is the "ship a skill with
each tool" convention; `mdbook-listings` is the reference implementation and
`playwright-rust` has adopted it. panschema is the outstanding one.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why this feature exists (the evidence)

Every item below is a real failure from a consumer session or from
panschema's own docs, not a hypothetical:

| What happened | What would have caught it |
|---|---|
| A consumer wrote `[schemas.x] source = "path:."` and got "unrecognized source protocol." `source` is the *remote* field; `path` is a separate one | a manifest reference with a complete working example |
| A `panschema.toml` was proposed with `[generate.x]` and no `[schemas.x]`. The generate loop iterates `manifest.schemas.keys()`, so it silently printed nothing and exited 0 | an **executable** example — a fenced block CI actually runs |
| `CLAUDE.md` described `mdbook-panschema` as an mdBook preprocessor; it has one `install` subcommand and implements none of the protocol | enumeration coverage over the real CLI surface |
| `layout.rs` marked four working layout algorithms "Planned implementation" | (nothing automatic — see *Honest limits*) |
| A sibling tool shipped a feature and never documented it; a consumer session had to read the code to find it | enumeration coverage over the feature list |

The pattern: **a doc that merely exists goes stale; a doc the build checks
stays true.**

## Honest limits — what this does *not* solve

Stated up front so the feature isn't oversold:

- **Executable examples catch example rot only.** An example that still
  works while the prose around it is wrong passes happily.
- **Enumeration coverage catches "undocumented thing," not "wrongly
  described thing."** It asserts a format/subcommand/key is *mentioned*,
  not that what is said about it is accurate.
- **Prose accuracy still needs the maintenance habit.** The `layout.rs`
  staleness would survive every check in this feature.

That residue is acceptable: the two mechanised checks cover the failures
that actually reached consumers, and they cost nothing per change.

---

## Vertical Slices

### Slice 1: The skill, with its examples executed and its coverage asserted

**Status:** Complete

**Priority:** Must Have

**User Value:** An agent in a consuming repo gets a correct, current
reference for panschema's CLI, manifest, and output formats — and the
reference cannot silently rot, because CI runs its examples and checks it
mentions everything the code offers.

**Acceptance Criteria:**
- [x] A skill ships in-repo with a description written as *triggers* (when
  to reach for panschema), not as a description of the tool — discovery is
  the harder half.
- [x] Reference material is split so depth loads on demand rather than
  bloating the agent's context: at minimum the CLI surface, the manifest
  schema, and the output formats.
- [x] **The manifest reference's example is executed by a test.** The test
  extracts the fenced TOML from the reference file itself — not a copy —
  builds a package around it, runs generation, and asserts the expected
  artifacts appear. A documented example that stops working fails CI.
- [x] **Every output format the registry knows is mentioned in the formats
  reference**, asserted against `FormatRegistry`, so a new writer cannot
  ship undocumented.
- [x] **Every CLI subcommand is mentioned in the CLI reference**, asserted
  against the parser.
- [x] **Every `[generate.<name>]` key is mentioned in the manifest
  reference**, asserted against the config type's own field names.
- [x] The reference states the traps that have actually bitten consumers:
  `path` versus `source`; `[generate]` needing a matching `[schemas]`;
  `postgres` (not `sql`), `json_schema` (underscore) versus the hyphenated
  CLI flag, `graph-json`/`instance-graph-json` (hyphens); and that
  `--strict` covers unmodeled constructs, dangling references, and
  instance-data violations only.

**Notes:**
- The enumeration tests assert *mention*, deliberately. Asserting prose
  correctness is not mechanisable, and a test that tried would either be
  vacuous or a maintenance tax.
- Extracting the example from the reference file is the whole point; a
  fixture that merely resembles the doc can drift from it.

---

### Slice 2: Packaging and consumer adoption

**Status:** Not started

**Priority:** Should Have

**Depends on:** Slice 1.

**User Value:** Consuming repos can install the skill in one step and get
updates with the tool, rather than copying a snapshot that rots.

**Acceptance Criteria:**
- [ ] The skill is installable by consumers through the same mechanism the
  sibling tools use, so enabling every tool is one step.
- [ ] Installation is documented where a consumer will look.
- [ ] A consumer that has installed it can be pointed at a released version
  rather than a copied file.

---

### Slice 3: `panschema guide` — recipes at the CLI

**Status:** Not started

**Priority:** Could Have

**Depends on:** Slice 1.

**User Value:** An agent that never loads the skill still finds the
recipes, because agents run `--help` reflexively.

**Acceptance Criteria:**
- [ ] A subcommand prints task-shaped recipes (generate docs for a schema;
  wire a manifest; publish versioned docs; validate instance data).
- [ ] Its content is drawn from the same reference material the skill uses,
  so the two cannot disagree.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: skill + executed examples + coverage | Must Have | — | Not started |
| Slice 2: packaging and adoption | Should Have | Slice 1 | Not started |
| Slice 3: `panschema guide` | Could Have | Slice 1 | Not started |

---

## Things to Watch

- **The skill is one more artifact that can go stale.** That is the whole
  reason the checks are part of slice 1 rather than a follow-up — shipping
  the doc without them just moves the staleness.
- **Keep the description trigger-shaped.** The most common failure is a
  description that describes the tool instead of naming when to reach for
  it; if only one thing is right, make it that.
- **Frictions belong in the skill, not just in a fix.** When a consumer
  session reports confusion, the response includes a reference update. That
  habit is what the mechanised checks cannot supply.
