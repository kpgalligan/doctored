#!/usr/bin/env python3
"""The Smart Sharpen admission drive, on a built Release app.

Filter > Sharpen > Smart Sharpen (`apply_filter` `smart_sharpen`) runs Reduce
Noise, two tonal classifiers and the low-pass of the blur to remove, keeping a
plane from each stage before the last (`SmartSharpen::plan` in
`core/src/sampling_smart_sharpen.rs`); the export workspace runs the same
kernel as its Smart Sharpen output sharpening (`core/src/export_sharpen.rs`).
This drive measures what each costs at 24 MP and holds the plan each depth is
admitted by against it. The gates are FIXED here before the drive runs; a miss
is recorded in `receipt.json` and fails its gate, and nothing is widened after
the fact. A miss raises `sampling_smart_sharpen.rs`'s
`STAGE_MARGIN_BYTES_PER_PIXEL`.

Every case runs in its own fresh owned `HostApp`, on its own scratch MCP
socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline
twin (a fresh app with the same document, then `resource_usage` instead of the
measured call), `--runs` times. The document is an MCP `new_document`,
6000x4000, one layer, filled by `apply_filter clouds` in two colours (#203040
and #e0c0a0), at 8 or 16 bits. Every measured call uses one setting: Lens Blur
removal at radius 4, amount 200, Reduce Noise 50, both fades 50 at tonal width
50 and radius 8, so every stage runs. The filter cases call `apply_filter
smart_sharpen`; the export cases call `export_image` at size none, PNG at the
document's depth, with `sharpen {"method": "smart_sharpen", ...}`. E is
`ri_interval_max_phys_footprint` - pre around that call, read once the
footprint has been still for 2 s, so the commit and the first projection are
inside the interval.

The plans, by arithmetic from the core's formulas (n = 24,000,000):
- KERNEL, `SmartSharpen::plan`'s scratch: the disc stage (the n x 16 plane, a
  ring of 9 rows of 6009 five-binary64 sums and 45 span entries, two
  bookkeeping MiB) beside the three kept planes (12n) and the 4n stage margin,
  plus two bookkeeping MiB: 774,357,724.
- 8 bits (P8): `filter_op_scratch_bytes` = (max(KERNEL, 20n) + 4 MiB) x 1.25 =
  973,190,035 (`core/tests/filter_smart_sharpen.rs` pins it as `PLAN_24MP`),
  and the host admits the larger of it and
  `ByteFilterAdmission.commitStageBytes` = 1,020,000,000, plus what the edit
  leaves retained (the smallest growth of `unique_retained_bytes` across the
  call): P8 = max(plan, commit) + dR, presence-admission's rule.
- 16 bits (P16): `selected_edit_plan`'s `scratch_bytes` = 6 MiB + 8n +
  max(KERNEL, 24n) + 2n = 1,020,649,180.
- Export, 8 bits (PE8): `ExportCapture.scratchEstimate`, the host's byte
  figure for an active sharpen of either method: 12n + 52n + 16 MiB =
  1,552,777,216.
- Export, 16 bits (PE16): the core's typed `export_plan` scratch: the source's
  8n, the sharpen term KERNEL + 8n (`OutputSharpen::plan_bytes`), the PNG
  encoder's 48n + 16 x 6000 + 128 MiB and one bookkeeping MiB: 2,445,720,028
  (`core/tests/export_smart_sharpen.rs` pins it).

- **M8**: P8 >= ceil(1.25 x max E) over the 8-bit filter runs.
- **M16**: P16 >= ceil(1.25 x max E) over the 16-bit filter runs.
- **E8**: PE8 >= ceil(1.25 x max E) over the 8-bit export runs.
- **E16**: PE16 >= ceil(1.25 x max E) over the 16-bit export runs.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/smart-sharpen-admission/measure.py --app <Doctored.app>
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
PARAMS = dict(amount=200, radius=4, noise=50, remove='lens', shadow_fade=50,
              shadow_tonal_width=50, shadow_radius=8, highlight_fade=50,
              highlight_tonal_width=50, highlight_radius=8)
DISC = 16 * N + (9 * (WIDTH + 9) * 40 + 5 * 9 * 4) + 2 * MIB
KERNEL = 12 * N + DISC + 4 * N + 2 * MIB
PLAN_8 = (max(KERNEL, 20 * N) + 4 * MIB) + math.ceil((max(KERNEL, 20 * N) + 4 * MIB) / 4)
COMMIT_8 = 8 * N + 24 * N + math.ceil(7 * 24 * N / 16)
PLAN_16 = 6 * MIB + 8 * N + max(KERNEL, 24 * N) + 2 * N
PLAN_E8 = 12 * N + 52 * N + 16 * MIB
PLAN_E16 = 8 * N + (KERNEL + 8 * N) + (48 * N + 16 * WIDTH + 128 * MIB) + MIB
assert (KERNEL, PLAN_8, COMMIT_8, PLAN_16, PLAN_E8, PLAN_E16) == (
    774357724, 973190035, 1020000000, 1020649180, 1552777216, 2445720028)
# case: (depth, export)
CASES = {'smart-8': (8, False), 'smart-16': (16, False),
         'export-8': (8, True), 'export-16': (16, True)}


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
    depth, export = CASES[case]
    record = dict(case=case, label=label, baseline=baseline, depth=depth, export=export)
    app = owned(output, label, executable)
    with app:
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
        elif export:
            settings = dict(format='png', bit_depth=depth,
                            sharpen=dict(PARAMS, method='smart_sharpen'))
            ok, reply = call(app, 'export_image', document_id=document, settings=settings,
                             path=str(output / f'{label}.png'), overwrite=True)
        else:
            ok, reply = call(app, 'apply_filter', document_id=document, filter='smart_sharpen',
                             **PARAMS)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre,
                      retained_before=before, retained_after=retained(app))
        record['retained_growth'] = record['retained_after'] - before
        if not baseline:
            record['changed'] = (reply.get('ok') if export else reply.get('changed')) if ok else None
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-smart-sharpen-admission-')))
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
                  plans=dict(kernel=KERNEL, plan_8=PLAN_8, commit_8=COMMIT_8, plan_16=PLAN_16,
                             plan_e8=PLAN_E8, plan_e16=PLAN_E16),
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
        depth, export = CASES[case]
        growth = None
        if export:
            plan = PLAN_E8 if depth == 8 else PLAN_E16
        elif depth == 8:
            growth = min((r.get('retained_growth', 0) for r in runs), default=0)
            plan = max(PLAN_8, COMMIT_8) + growth
        else:
            plan = PLAN_16
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   retained_growth=[r.get('retained_growth') for r in runs], added_growth=growth,
                   plan=plan, need=need, ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][f'{"E" if export else "M"}{depth}'] = dict(passed=row['passed'])
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
