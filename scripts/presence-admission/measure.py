#!/usr/bin/env python3
"""The Presence admission drive, on a built Release app.

Filters > Presence (`apply_filter` `presence`) runs four guided passes and
keeps a plane from each, so its kernel plan (`Presence::plan` in
`core/src/sampling_presence.rs`) is the largest of the spatial filters'. This
drive measures what the filter really costs at 24 MP and holds the plan each
depth is admitted by against it. The gates are FIXED here before the drive
runs; a miss is recorded in `receipt.json` and fails its gate, and nothing is
widened after the fact. A miss raises `sampling_presence.rs`'s
`PLANE_BYTES_PER_PIXEL`.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with the
same document, then `resource_usage` instead of the filter), `--runs` times.
The document is an MCP `new_document`, 6000x4000, one layer, filled by
`apply_filter clouds` in two colours (#203040 and #e0c0a0), at 8 or 16 bits.
The measured call is `apply_filter presence {texture: 50, clarity: 50,
dehaze: 50}`, which runs all four guided passes (the veil, the fine, mid and
coarse bases). E is `ri_interval_max_phys_footprint` - pre around that call,
read once the footprint has been still for 2 s, so the commit and the first
projection are inside the interval.

The plans, by arithmetic from the core's formulas (n = 24,000,000):
- KERNEL, `Presence::plan`'s scratch: the coarse pass (radius 117: the n x 16
  correction plane, five box rings of 237 rows and a 120-row delay and
  regression ring, 62,640,000, four bookkeeping MiB, and the n x 4 delta) with
  the three planes kept before it and two bookkeeping MiB: 836,931,456.
- 8 bits: `filter_op_scratch_bytes` = (max(KERNEL, 20n) + 4 MiB) x 1.25 =
  1,051,407,200 (`core/tests/filter_presence.rs` pins it as `PLAN_24MP`), and
  the host admits the larger of it and `ByteFilterAdmission.commitStageBytes`
  = 8n + 24n + ceil(7 x 24n / 16) = 1,020,000,000, beside the document's own
  retained bytes. What the edit leaves retained (the growth of
  `unique_retained_bytes` across the call, the smallest of the runs) is inside
  E and is admitted as retained, so it is added: P8 = max(plan, commit) + dR,
  `ProjectionMargin`'s rule, as in `sampling_lens.rs`'s record.
- 16 bits: `selected_edit_plan`'s `scratch_bytes` = 6 MiB + 8n + max(KERNEL,
  16n + 8n) + 2n = 1,083,222,912 (pinned as `PLAN_24MP_WIDE`): P16.

- **M8**: P8 >= ceil(1.25 x max E) over the 8-bit runs.
- **M16**: P16 >= ceil(1.25 x max E) over the 16-bit runs.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/presence-admission/measure.py --app <Doctored.app>
[--output <folder outside the repository>] [--runs 3]. Exit 0 only when every
gate passes. It imports `owned`, `call` and `footprint` from
`scripts/host-still-admission/verify.py`; memory, not wall-clock.
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
WIDTH, HEIGHT = 6000, 4000
N = WIDTH * HEIGHT
MIB = 1024 * 1024
PARAMS = dict(texture=50, clarity=50, dehaze=50)
COARSE_PASS = 16 * N + (5 * 237 + 118 + 2) * WIDTH * 8 + 4 * MIB + 4 * N
KERNEL = COARSE_PASS + 3 * 4 * N + 2 * MIB
PLAN_8 = (max(KERNEL, 20 * N) + 4 * MIB) + math.ceil((max(KERNEL, 20 * N) + 4 * MIB) / 4)
COMMIT_8 = 8 * N + 24 * N + math.ceil(7 * 24 * N / 16)
PLAN_16 = 6 * MIB + 8 * N + max(KERNEL, 24 * N) + 2 * N
assert (KERNEL, PLAN_8, COMMIT_8, PLAN_16) == (836931456, 1051407200, 1020000000, 1083222912)
CASES = {'presence-8': 8, 'presence-16': 16}


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


def make_document(app, depth):
    ok, made = call(app, 'new_document', width=WIDTH, height=HEIGHT, depth=depth, profile='srgb',
                    background='white')
    assert ok, made
    document = made['document']['id']
    ok, clouds = call(app, 'apply_filter', document_id=document, filter='clouds', seed=7,
                      foreground_color='#203040', background_color='#e0c0a0')
    assert ok and clouds.get('changed') is True, clouds
    return document


def retained(app):
    ok, usage = call(app, 'resource_usage')
    assert ok, usage
    return usage['unique_retained_bytes']


def measure_case(output, executable, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    depth = CASES[case]
    record = dict(case=case, label=label, baseline=baseline, depth=depth)
    with owned(output, label, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        document = make_document(app, depth)
        pid = app.process.pid
        settle(pid)
        before = retained(app)
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            ok, reply = call(app, 'apply_filter', document_id=document, filter='presence',
                             **PARAMS)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre,
                      retained_before=before, retained_after=retained(app))
        record['retained_growth'] = record['retained_after'] - before
        if not baseline:
            record['changed'] = reply.get('changed') if ok else None
            if not ok:
                record['refusal'] = reply[:800]
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES),
                        help='comma list of cases, from ' + ','.join(CASES))
    arguments = parser.parse_args()
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-presence-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=MINIMUM_MAC_LIMIT, params=PARAMS,
                  plans=dict(kernel=KERNEL, plan_8=PLAN_8, commit_8=COMMIT_8, plan_16=PLAN_16),
                  records=[], gates={})

    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "filter"}-{run}'
                try:
                    record = measure_case(output, executable, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)

    table = {}
    passed = True
    for case in selected:
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('changed') is True for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        if CASES[case] == 8:
            growth = min((r.get('retained_growth', 0) for r in runs), default=0)
            plan = max(PLAN_8, COMMIT_8) + growth
        else:
            growth = None
            plan = PLAN_16
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   retained_growth=[r.get('retained_growth') for r in runs], added_growth=growth,
                   plan=plan, need=need, ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][f'M{CASES[case]}'] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:12} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline '
              f'{row["baseline_E"]} growth {row["retained_growth"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    for name, gate in report['gates'].items():
        print(f'gate {name}: {"PASS" if gate["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
