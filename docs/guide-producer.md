# Schema author guide

This guide is for **schema authors** — you have (or are about to write)
a LinkML schema you want other projects to depend on. It covers the
publishing standard panschema uses, the commands that drive the
producer workflow, and what a release looks like end-to-end.

The companion document for **consumers** of your schema (projects that
want to fetch it and run codegen) is [guide-consumer.md](guide-consumer.md).

## What is a panschema package?

A **package** is the unit of "thing you publish" — a directory
containing:

1. `panschema-publish.toml` — declares the package's authoritative
   identity (name, version, target LinkML metamodel version) and
   points at the main schema file.
2. The main schema file itself (typically `schema.yaml` for LinkML,
   but `.ttl` and other formats supported by panschema's readers work
   too).

That's it. Other files (`README.md`, `LICENSE`, generated docs, etc.)
are fine but not required.

A minimal `panschema-publish.toml`:

```toml
[schema]
name = "my-schema"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
```

`[schema].name` is what consumers see in their `panschema.toml`. It
should be unique, stable, and follow whatever naming convention
your ecosystem uses (kebab-case is the LinkML convention).

`[schema].version` is semver — `panschema verify` on the consumer
side enforces that the version in the publish file matches what's
declared in the consumer's manifest.

`[files].main` is a path relative to the publish file's location.

## Scaffolding a new schema package

You don't need to write `panschema-publish.toml` by hand. `panschema
init` covers the common cases:

### Starting from an existing LinkML file

```bash
cd my-schema-repo/
panschema init --from schema.yaml
```

This reads `schema.yaml`, extracts `name` and `version` from its
top-level metadata, and writes a matching `panschema-publish.toml`.
The `--main` field defaults to the path you passed (`schema.yaml`).

### Starting from scratch

```bash
cd my-schema-repo/
panschema init --name my-schema --version 0.1.0 --main schema.yaml
```

Or just `panschema init` with no args — defaults to:

- `name` = the CWD's basename
- `version` = `0.1.0`
- `main` = `schema.yaml`
- `linkml` = `1.7.0`

### If a publish file already exists

`panschema init` refuses to overwrite. Pass `--force` to clobber an
existing file (rare; you'd normally just hand-edit instead).

### Post-write validation

After writing, `panschema init` parses the file back and tries to
read the configured main file. If the main file doesn't exist or
doesn't parse, it prints a warning — but still writes the publish
file. The validation is informational; you can run `init` while
you're still authoring the schema.

## Cutting a release

Once you've got a schema and a publish file, releases are version
bumps + git tags. `panschema release` handles both.

### Bump-only (default)

```bash
panschema release --level patch     # 0.1.3 → 0.1.4
panschema release --level minor     # 0.1.4 → 0.2.0
panschema release --level major     # 0.2.0 → 1.0.0
panschema release --version 1.0.0-rc1   # exact version (e.g. pre-release)
```

The default doesn't touch git. It updates `panschema-publish.toml`
and prints the commands you'd run to complete the release:

```
Bumped panschema-publish.toml: 0.1.3 → 0.1.4
Suggested next steps:
    git commit -am 'release: v0.1.4'
    git tag v0.1.4
    git push --follow-tags
```

This makes the command safe to script and easy to integrate into
release workflows that handle git themselves.

### One-shot release with `--git`

```bash
panschema release --level patch --git           # bump + commit + tag
panschema release --level patch --git --push    # bump + commit + tag + push
```

`--git` commits `panschema-publish.toml` with message `release:
v<version>` and tags `v<version>`. `--push` (requires `--git`) also
runs `git push --follow-tags`.

Safety checks before any git operation:

- Refuses if the working tree is dirty (other uncommitted changes
  in the repo). Commit or stash them first.
- Refuses if the target tag already exists.
- Refuses if not in a git repo.

### Dry-run

```bash
panschema release --level patch --dry-run
```

Prints the plan (old → new, what would be committed/tagged/pushed)
without writing any file or running any git command. Useful as a
sanity check.

### Pre-1.0 semantics

`panschema release` follows literal semver: `0.x.y --level major`
goes to `1.0.0`, not `0.(x+1).0`. If you want the informal pre-1.0
"minor-as-breaking" convention, pass `--version 0.(x+1).0`
explicitly.

`--level` bumps drop any pre-release / build metadata (so
`0.2.0-rc1 --level patch` produces `0.2.1`, not `0.2.1-rc1`). For
arbitrary pre-release strings, use `--version`.

## What gets shipped

Consumers fetch your package by anonymous git-archive tarball from
`codeload.github.com`. The tarball includes everything in your
repo, but only `panschema-publish.toml` and the file at
`[files].main` are *read* by panschema. The shared cache extracts
the whole tarball, so symlinks etc. need to stay inside the
package; panschema enforces this on the consumer side.

The lockfile on the consumer records the commit SHA of the tag —
which is recovered from the tarball's top-level directory name
`<owner>-<repo>-<sha>`. No GitHub API call is made, so your
schema's `fetch` doesn't count against the anonymous 60/hr rate
limit.

## Recommended repo layout

A schema repo is just a git repo. Typical layout:

```
my-schema/
├── panschema-publish.toml
├── schema.yaml
├── README.md
├── LICENSE
└── .gitignore
```

Nothing in particular about that is mandatory — `panschema-publish.toml`
needs to be at the repo root (codeload tarballs serve from there) and
`[files].main` needs to resolve to a parseable schema file.

## Producer workflow checklist

When you're getting ready to publish a new schema:

1. `cd` into the repo root.
2. `panschema init --from <existing>.yaml` (or `--name X --version 0.1.0 --main schema.yaml` if starting from scratch).
3. Commit the publish file. Push.
4. When ready to ship a version: `panschema release --level <patch|minor|major> --git --push`.

When cutting subsequent releases:

1. Make changes to your schema.
2. Commit them.
3. `panschema release --level <patch|minor|major> --git --push` to bump
   the version and tag.

That's it. Consumers re-running `panschema fetch` pick up the new
version after they update their `panschema.toml` (or run
`panschema add <repo>@<new-version>`).
