"""Record what every probe prints, or check the probes against the record.

Run with `uv run --project conformance conformance/snapshot.py` to write
`conformance/snapshots/<probe>.txt`, and with `--check` to compare the
probes' output against those files, printing a unified diff for each probe
whose verdicts changed and exiting 1. The snapshots are the verdicts of the
LinkML pinned in `uv.lock`; a release or a LinkML `main` that changes one
shows up as a diff, which is what the upstream LinkML check reports.

A probe's output is normalized before it is compared: the temporary
directories it writes into become `<tmp>`, and trailing whitespace and
trailing blank lines are stripped, as the repository's hooks strip them from
the committed files. A probe must print the same thing on every run, so one
whose generator varies its output's order sorts it first, and the probes run
under a fixed `PYTHONHASHSEED`. A probe that exits non-zero is recorded with
its exit code and the end of its stderr, with the Python installation and
the checkout it names written as `<python>` and `<repo>`.

A probe that differs is run once more before it is reported, since several
fetch from `w3id.org` and a failed fetch is not a changed verdict. `--check`
exits 1 when a probe differs, and 2 when the check itself cannot run.
"""

import difflib
import os
import re
import subprocess
import sys
import tempfile
import traceback
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
SNAPSHOTS = HERE / "snapshots"
TEMP_ROOTS = {tempfile.gettempdir(), os.path.realpath(tempfile.gettempdir())}
TEMP = re.compile(rf"(?:{'|'.join(re.escape(root) for root in sorted(TEMP_ROOTS, key=len, reverse=True))})/tmp[\w-]+")
# The virtual environment sits inside the checkout, so the Python paths go first.
PLACEHOLDERS = ((sys.prefix, "<python>"), (sys.base_prefix, "<python>"), (str(HERE.parent), "<repo>"))


def normalize(text: str) -> str:
    text = TEMP.sub("<tmp>", text)
    for path, placeholder in PLACEHOLDERS:
        text = text.replace(path, placeholder)
    kept = "".join(line.rstrip() + "\n" for line in text.splitlines())
    return kept.rstrip("\n") + "\n" if kept.strip() else ""


def run_probe(probe: Path) -> str:
    env = {**os.environ, "PYTHONHASHSEED": "0"}
    r = subprocess.run([sys.executable, str(probe)], capture_output=True, text=True, cwd=HERE.parent, env=env)
    out = normalize(r.stdout)
    if r.returncode != 0:
        tail = normalize("\n".join(r.stderr.strip().splitlines()[-20:]))
        out += f"== exit {r.returncode}\n{tail}"
    return out


def snapshot(stem: str) -> Path:
    return SNAPSHOTS / f"{stem}.txt"


def record(outputs: dict[Path, str], stale: set[str]) -> int:
    SNAPSHOTS.mkdir(exist_ok=True)
    for probe, out in outputs.items():
        snapshot(probe.stem).write_text(out)
    for name in stale:
        snapshot(name).unlink()
    return 0


def check(outputs: dict[Path, str], before: dict[Path, str], stale: set[str]) -> int:
    changed = 0
    for probe, out in outputs.items():
        if out != before[probe]:
            changed += 1
            name = snapshot(probe.stem).name
            sys.stdout.writelines(
                difflib.unified_diff(
                    before[probe].splitlines(keepends=True),
                    out.splitlines(keepends=True),
                    f"recorded/{name}",
                    f"measured/{name}",
                )
            )
    for name in sorted(stale):
        changed += 1
        print(f"snapshots/{name}.txt has no probe")
    print(f"{changed} of {len(outputs)} probes differ from the recorded verdicts" if changed else "every probe matches")
    return 1 if changed else 0


def main() -> int:
    probes = sorted(HERE.glob("probe_*.py"))
    stale = {s.stem for s in SNAPSHOTS.glob("*.txt")} - {p.stem for p in probes}
    with ThreadPoolExecutor() as pool:
        outputs = dict(zip(probes, pool.map(run_probe, probes)))
        if "--check" not in sys.argv[1:]:
            return record(outputs, stale)
        before = {p: snapshot(p.stem).read_text() if snapshot(p.stem).exists() else "" for p in probes}
        again = [p for p in probes if outputs[p] != before[p]]
        outputs.update(zip(again, pool.map(run_probe, again)))
    return check(outputs, before, stale)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:  # noqa: BLE001 - exit 2 keeps a crash apart from a diff
        traceback.print_exc()
        sys.exit(2)
