# ADR-013: The lockfile records pins only

**Status:** Accepted (2026-09-17)

## Context

`panschema.lock` was written with one entry per `[schemas]` entry — name,
version, source spec, and a checksum of the main file — for `github:` and
`path:` sources alike, and `fetch --check` compared every entry. Feature 05's
Open Question 2 chose that deliberately: checksumming a local path would
detect "schema edited but `generate` not re-run".

In use it did the opposite of what a lockfile is for. A `path:` source is
the working tree: the repo's own package as `path = "."`, or a sibling
checkout during co-development. Every ordinary edit to one of those made
`fetch --check` report *dependency drift* and fail CI before any other gate
ran, so consumers either re-ran `fetch` in every schema commit — churning a
lockfile that recorded nothing durable — or dropped the check, and with it
the verification of their real pins. The skill reference told them to "keep
`fetch --check` for real dependencies", which the tool gave no way to do.

Two things changed since Open Question 2 was answered. `generate --check`
now exists and detects "edited but not regenerated" directly, from the
outputs, with no lockfile involved. And the rest of the tool already treats
path sources as unpinned: the README says they "carry no pin and always
resolve from the working tree", and `publish` gates on `github:` sources
only.

## Decision

A lockfile entry is a pin: a `github:` source at a released version. `fetch`
resolves every `[schemas]` entry — so a missing package or a malformed entry
still fails, as it does in `generate` — and records only the pins.
`fetch --check` means "every pin still holds": it compares only pins, and a
manifest with none needs no lockfile and passes. With nothing to lock,
`fetch` writes no lockfile and removes one that would now be empty.

A lockfile entry is judged by what it records, not by its name. An entry
recording a `path:` source is a leftover from before this decision: `--check`
names it and asks for a `fetch` to drop it, without failing, so an upgrade
does not turn a consumer's CI red once more on the way in. An entry
recording a pin whose manifest entry is now a `path:` source is a
disagreement between the two files, and fails like any other drift.

The distinction is made beside the code that already decides source kinds:
`SchemaSource::is_pinned` for a parsed source, and `spec_is_pinned` for the
spec string the lockfile stores, where only the prefix survives; a test
holds the two to the same answer. There is no filesystem probe: the
manifest's own package is a path source like any other, whatever its
spelling.

## Consequences

- A consumer with a self entry and a real pin verifies the pin in CI and
  edits its own schema freely. The re-`fetch`-on-every-edit workaround and
  the lockfile churn go away.
- A repo with only path sources has no `panschema.lock`. Repos that had one
  see `fetch` remove it, and `--check` pass.
- What is given up: a freeze on `path:` content outside the repo — a sibling
  checkout someone edits without telling you. That is the co-development
  case by definition, and the consumer sees the change the next time it
  generates; a lock entry that reddened on every such edit did not protect
  against it. Inside the repo, git already shows an edited vendored copy.
- "Edited but not regenerated" is caught by `generate --check`, which the
  consumer guide now names for that purpose.
- Feature 05's Slice 2 acceptance criteria and Open Question 2 are amended
  to this decision.
