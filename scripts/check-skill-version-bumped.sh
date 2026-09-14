#!/usr/bin/env bash
#
# Fail when the shipped skill or the plugin manifest changes without a
# version bump.
#
# The skill reaches consumers through two channels that version differently.
# `/plugin update` gates on plugin.json's version: until it moves, an update
# reports "already at the latest version", so a skill edit reaches nobody.
# The Agent Skills CLI has no version concept and re-pulls, so the
# frontmatter's `metadata.version` is the only version that channel's reader
# can compare. The two have to agree or the channels disagree about what is
# installed.
#
# The skill versions on its own cadence, not the crate's: the crate version
# on the default branch is the last release, while the skill there describes
# the next one, so pinning them made the skill claim a release whose binary
# it did not match.
#
# The rule: whenever content under `skills/` or `.claude-plugin/` differs
# from a base revision, the manifest's version must differ from it too, and
# the skill's frontmatter must carry that same version. The pre-commit hook
# applies it per commit, index against HEAD. CI applies it with
# `--base <ref>` to everything since the merge base with that ref — the
# aggregate that actually lands — because the hook is per-clone and
# `--no-verify` skips it. The two can disagree on one commit (a bump in one
# commit and an unbumped edit in the next passes CI and fails the hook on
# the second), and that is the intent: the hook keeps history honest, CI
# keeps what lands honest.
#
# A ref that does not resolve or shares no history with HEAD is an error
# here. A caller that would rather skip in that case checks before calling.
#
# The frontmatter check duplicates a test in this repo on purpose: the
# script is what other repos copy, and they have no such test.
set -euo pipefail

usage() {
  echo "usage: $(basename "$0") [--base <ref>]" >&2
  exit 2
}

base=HEAD
while [ $# -gt 0 ]; do
  case "$1" in
    --base)
      [ $# -ge 2 ] || usage
      base="$2"
      shift 2
      ;;
    *) usage ;;
  esac
done

manifest=.claude-plugin/plugin.json
skill=skills/panschema-development/SKILL.md

# Initial commit: nothing to compare against.
git rev-parse -q --verify HEAD >/dev/null 2>&1 || exit 0

# Everything below compares the index against `$base`. That serves both
# callers with one code path: the hook wants exactly the index, and a fresh
# CI checkout leaves the index equal to HEAD.
if [ "$base" != HEAD ]; then
  base=$(git merge-base "$base" HEAD 2>/dev/null) || {
    echo "skill version guard: --base does not resolve or shares no history with HEAD." >&2
    exit 2
  }
fi

# Nothing under the guarded paths moved, so there is nothing to guard.
# pre-commit's `files:` filter already implies this on a real commit, but
# `run --all-files` runs every hook regardless of what changed. Without this
# the guard fails a clean full-tree run with "content changed but the version
# did not" — and the obvious response to that message is a version bump
# describing no change, so the false positive has a plausible wrong fix.
# `--quiet` exits 1 for "changed"; anything else is git failing.
changed=0
git diff --cached --quiet "$base" -- skills/ .claude-plugin/ || changed=$?
case "$changed" in
  0) exit 0 ;;
  1) ;;
  *)
    echo "skill version guard: git diff against $base failed (exit $changed)." >&2
    exit 2
    ;;
esac

# A manifest missing at a revision reads as "no version" rather than a
# traceback: on the commit that first adds it, `git show` for the old
# revision produces nothing. One that is present but not a JSON object with
# a version string says so and reads as "no version" too.
read_manifest_version() {
  python3 -c '
import json, sys
text = sys.stdin.read().strip()
if not text:
    print("")
    raise SystemExit(0)
try:
    version = json.loads(text).get("version")
except (ValueError, AttributeError):
    print("skill version guard: manifest is not a JSON object", file=sys.stderr)
    print("")
    raise SystemExit(0)
print("" if version is None else str(version).strip())
'
}

old=$(git show "$base:$manifest" 2>/dev/null | read_manifest_version) || old=""
new=$(git show ":$manifest" 2>/dev/null | read_manifest_version) || new=""

if [ -z "$new" ]; then
  echo "skill version guard: $manifest has no readable version field." >&2
  echo "Add one -- it is the update gate for installed consumers." >&2
  exit 1
fi
if [ "$old" = "$new" ]; then
  echo "skill version guard: skill or plugin content changed but $manifest is still $new." >&2
  echo "Bump the version so installed consumers see the update." >&2
  exit 1
fi

# A bare YAML number prints the way the Rust test in this repo prints it
# (`1.0` as `1`), so the two gates cannot disagree about the same file.
skill_version=$(git show ":$skill" 2>/dev/null | python3 -c '
import re, sys
try:
    import yaml
except ModuleNotFoundError:
    yaml = None
text = sys.stdin.read()
m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
if not m:
    sys.exit("no frontmatter")
front = m.group(1)
if yaml is not None:
    meta = (yaml.safe_load(front) or {}).get("metadata") or {}
    v = meta.get("version")
    if isinstance(v, float) and v.is_integer():
        v = int(v)
    print("" if v is None else str(v).strip())
else:
    m = re.search(r"^metadata:\s*\n(?:[ \t]+.*\n)*?[ \t]+version:[ \t]*\"?([^\"\n]+)\"?", front, re.M) \
        or re.search(r"^metadata:\s*\{[^}]*version:[ \t]*\"?([^\",}]+)\"?", front, re.M)
    print(m.group(1).strip() if m else "")
') || {
  echo "skill version guard: could not read frontmatter from $skill." >&2
  exit 1
}

if [ -z "$skill_version" ]; then
  echo "skill version guard: $skill has no metadata.version." >&2
  echo "Add one matching $manifest ($new) -- it is the only version a" >&2
  echo "consumer installing outside /plugin can see." >&2
  exit 1
fi
if [ "$skill_version" != "$new" ]; then
  echo "skill version guard: $manifest is $new but $skill says $skill_version." >&2
  echo "Move them together." >&2
  exit 1
fi
