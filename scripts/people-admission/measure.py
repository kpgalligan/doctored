#!/usr/bin/env python3
"""The people admission drive, on a built Release app.

`inspect_subjects {model: "people"}` runs Vision's person-instance request
beside the accurate person segmenter and builds the canvas-sized owner map
(`PersonInstances.build`). It is admitted by `SubjectTargeting.scratchBytes`,
12 bytes a canvas pixel plus 32 MiB, the same plan the objects picker uses.
This drive measures what the people analysis really costs at the largest
fixture and holds that plan against it. The gate is FIXED here before the
drive runs; a miss is recorded in `receipt.json` and fails, and nothing is
widened after the fact. A miss raises `scratchBytes`' per-pixel 12 to the
smallest integer that passes at the measured size.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin, `--runs` times. The
document is `--fixture` (IMG_0221.jpg, 4032x3024, one of the user's private
photos), opened IN PLACE through `open_document`. After the footprint is
still, the interval is reset and the case calls `inspect_subjects {model:
"people", document_id}`; the twin calls `resource_usage` instead. E is
`ri_interval_max_phys_footprint` - pre, read once the footprint has been
still for 2 s.

- **M**: PLAN = 12n + 32 MiB = 179,867,648 (n = 12,192,768) >=
  ceil(1.25 x max E) over the case runs, and every case reply lists at least
  3 people (the analysis really ran and allocated).
The baseline twins' E is recorded beside the row, not gated.

Usage: python3 -B scripts/people-admission/measure.py --app <Doctored.app>
--fixture <IMG_0221.jpg> [--output <folder outside the repository>]
[--runs 3]. Exit 0 only when the gate passes. It imports `owned`, `call` and
`footprint` from `scripts/host-still-admission/verify.py`; memory, not
wall-clock.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
from verify import call, footprint, owned  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
WIDTH, HEIGHT = 4032, 3024
N = WIDTH * HEIGHT
MIB = 1024 * 1024
PLAN = 12 * N + 32 * MIB
assert (N, PLAN) == (12192768, 179867648)
MINIMUM_PEOPLE = 3


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def settle(pid, timeout=120):
    """Waits until the footprint has been still for 2 s; answers it."""
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def measure_case(output, executable, fixture, label, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    record = dict(label=label, baseline=baseline)
    with owned(output, label, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        ok, opened = call(app, 'open_document', path=str(fixture))
        assert ok, opened
        document = opened['document']
        record['size'] = [document['width'], document['height']]
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            ok, reply = call(app, 'inspect_subjects', document_id=document['id'], model='people')
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            record['people'] = reply.get('instance_count') if ok else None
            if not ok:
                record['refusal'] = reply[:800]
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-people-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    fixture = arguments.fixture.expanduser().resolve()
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), fixture_sha256=digest(fixture),
                  runs=arguments.runs, working_limit_bytes=MINIMUM_MAC_LIMIT, plan=PLAN,
                  records=[])

    for run in range(1, arguments.runs + 1):
        for baseline in (True, False):
            label = f'people-{"baseline" if baseline else "inspect"}-{run}'
            try:
                record = measure_case(output, executable, fixture, label, baseline)
            except Exception as error:          # Recorded, never repaired.
                record = dict(label=label, baseline=baseline, ok=False,
                              error=f'{type(error).__name__}: {error}'[:800])
            record['run'] = run
            report['records'].append(record)
            print(json.dumps(record), flush=True)

    runs = [r for r in report['records'] if not r['baseline']]
    twins = [r for r in report['records'] if r['baseline']]
    ok = bool(runs) and all(r.get('ok') and (r.get('people') or 0) >= MINIMUM_PEOPLE
                            and r.get('size') == [WIDTH, HEIGHT] for r in runs)
    limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
    max_e = max((r.get('E', 0) for r in runs), default=0)
    need = math.ceil(1.25 * max_e)
    row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
               people=[r.get('people') for r in runs], plan=PLAN, need=need,
               ratio=(PLAN / max_e) if max_e else None,
               passed=ok and limit_ok and bool(twins) and max_e > 0 and PLAN >= need)
    report['table'] = row
    report['passed'] = row['passed']
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    print(f'people-inspect plan {PLAN} need {need} ratio {row["ratio"] and round(row["ratio"], 3)} '
          f'E {row["E"]} baseline {row["baseline_E"]} people {row["people"]} '
          f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(f'gate M: {"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if row['passed'] else 1)


if __name__ == '__main__':
    main()
