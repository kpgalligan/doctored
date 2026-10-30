#!/usr/bin/env python3
"""The Develop Filter admission drive, on a built Release app.

Filters > Develop Filter (`apply_filter` `develop_filter`) writes its toned
plane beside the tone guide and then beside Presence's whole working set, so
its kernel plan (`DevelopFilter::plan` in `core/src/sampling_develop_filter.rs`) is
Presence's plus an n x 16 plane. This drive measures what the filter really
costs at 24 MP and holds the plan each depth is admitted by against it. The
gates are FIXED here before the drive runs; a miss is recorded in
`receipt.json` and fails its gate, and nothing is widened after the fact. A
miss raises `sampling_develop_filter.rs`'s `TONED_BYTES_PER_PIXEL`.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with the
same document, then `resource_usage` instead of the filter), `--runs` times.
The document is Presence's drive's: an MCP `new_document`, 6000x4000, one
layer, filled by `apply_filter clouds` in two colours (#203040 and #e0c0a0),
at 8 or 16 bits. The measured call is `apply_filter develop_filter` with ALL (all
thirteen controls non-zero, Texture, Clarity and Dehaze at 50, so every stage
and all four of Presence's guided passes run) or TONE (the same without the
three Presence keys: the guide, one point pass and the result). E is
`ri_interval_max_phys_footprint` - pre around that call, read once the
footprint has been still for 2 s, so the commit and the first projection are
inside the interval.

The plans, by arithmetic from the core's formulas (n = 24,000,000):
- GUIDE, the tone guide at radius 30 x 6000/2048: 750 x 500 cells at factor
  8, a 35-tap kernel: 375,000 x 24 + 35 x 4 + 4096 = 9,004,236.
- PRESENCE, `Presence::plan` with all three amounts: 836,931,456
  (`scripts/presence-admission/measure.py`'s KERNEL).
- KERNEL: 16n + max(GUIDE, PRESENCE) + 2 MiB = 1,223,028,608 for ALL; 16n +
  GUIDE + 2 MiB = 395,101,388 for TONE.
- 8 bits: `filter_op_scratch_bytes` = (max(KERNEL, 20n) + 4 MiB) x 1.25 =
  1,534,028,640 (ALL, `core/tests/filter_develop_filter.rs`'s `PLAN_24MP`) and
  605,242,880 (TONE, `PLAN_24MP_TONE`), and the host admits the larger of it
  and `ByteFilterAdmission.commitStageBytes` = 1,020,000,000, beside the
  document's own retained bytes; what the edit leaves retained (the smallest
  growth of `unique_retained_bytes` across the call) is inside E and is
  admitted as retained, so it is added: P8 = max(plan, commit) + dR,
  `ProjectionMargin`'s rule, as in Presence's record.
- 16 bits: `selected_edit_plan`'s `scratch_bytes` = 6 MiB + 8n + max(KERNEL,
  16n + 8n) + 2n = 1,469,320,064 (ALL, `PLAN_24MP_WIDE`) and 822,291,456
  (TONE, `PLAN_24MP_TONE_WIDE`): P16.

MASK (the filter's masks; `core/src/develop_filter_masks.rs`) runs
`apply_filter develop_filter` with no global control and three masks: a radial
with Exposure and Clarity, a 50-dab brush with feather 50 and Dehaze, and a
shadow luminance range with Shadows (the host adds the document's
`range_profile`). Its kernel, `DevelopFilterMasked::plan`, is pinned by
`core/tests/filter_develop_filter_masks.rs`'s
`the_24_megapixel_masked_plans_are_pinned`: the brush local's 16n + 8n
(`LOCAL_BYTES_PER_PIXEL`) + the 120 px feather's blur 857,411,692 + the
Dehaze `DevelopFilter` plan 910,212,608 + 2 MiB = 2,345,721,452; P8 =
2,937,394,695 and P16 = 2,592,012,908 by the formulas above. A miss raises
`LOCAL_BYTES_PER_PIXEL`.

MASKD (the masks' Sharpness and Noise Reduction;
`core/src/local_detail.rs`) runs one radial over the whole frame (centre 0.5,
0.5, radius 1 x 1, hardness 0.5) with Clarity 50, Sharpness 60 and Noise
Reduction 60. Its kernel, pinned by the same test, is 16n + 8n + the detail
stage beside the n x 16 Clarity value plane (16n + the Smart Sharpen kernel
at amount 90, radius 1 484,866,360 beside the n x 16 denoised plane + 2 MiB,
larger than Clarity's `DevelopFilter` plan) + 2 MiB = 1,833,060,664; P8 =
2,296,568,710 and P16 = 2,079,352,120. A miss raises `LOCAL_BYTES_PER_PIXEL`.

- **M8**: P8 >= ceil(1.25 x max E) over each 8-bit case's runs.
- **M16**: P16 >= ceil(1.25 x max E) over each 16-bit case's runs.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/develop-filter-admission/measure.py --app <Doctored.app>
[--output <folder outside the repository>] [--runs 3] [--cases a,b]. Exit 0
only when every gate passes. It imports `owned`, `call` and `footprint` from
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
TONE = dict(temperature=30, tint=-20, exposure=0.4, contrast=25, highlights=-40, shadows=35,
            whites=15, blacks=-10, vibrance=25, saturation=-10)
ALL = dict(TONE, texture=50, clarity=50, dehaze=50)
MASK = dict(masks=[
    dict(shape=dict(op='radial', center=[0.4, 0.45], radius=[0.25, 0.3], hardness=0.4),
         exposure=0.6, clarity=40),
    dict(shape=dict(op='empty'), feather=50, dehaze=40,
         brush=[dict(mode='add', dabs=[dict(center=[0.2 + 0.012 * k, 0.7], radius=0.04, hardness=0.6,
                                            opacity=1) for k in range(50)])]),
    dict(shape=dict(op='full'), shadows=50,
         range=dict(luminance=dict(low=0, high=0.35, low_falloff=0.1, high_falloff=0.1, invert=False))),
])
MASKD = dict(masks=[
    dict(shape=dict(op='radial', center=[0.5, 0.5], radius=[1.0, 1.0], hardness=0.5),
         clarity=50, sharpness=60, noise_reduction=60),
])
GUIDE = 375_000 * 24 + 35 * 4 + 4096
PRESENCE = 836_931_456
FEATHER_120 = 857_411_692
DEHAZE_KERNEL = 910_212_608
SHARPEN_KERNEL = 484_866_360
KERNEL = dict(all=16 * N + max(GUIDE, PRESENCE) + 2 * MIB, tone=16 * N + GUIDE + 2 * MIB,
              mask=16 * N + 8 * N + FEATHER_120 + DEHAZE_KERNEL + 2 * MIB,
              maskd=16 * N + 8 * N + 16 * N + (16 * N + SHARPEN_KERNEL + 2 * MIB) + 2 * MIB)


def plan_8(kernel):
    base = max(kernel, 20 * N) + 4 * MIB
    return base + math.ceil(base / 4)


COMMIT_8 = 8 * N + 24 * N + math.ceil(7 * 24 * N / 16)
PLAN_8 = {name: plan_8(kernel) for name, kernel in KERNEL.items()}
PLAN_16 = {name: 6 * MIB + 8 * N + max(kernel, 24 * N) + 2 * N for name, kernel in KERNEL.items()}
assert (KERNEL['all'], KERNEL['tone']) == (1223028608, 395101388)
assert (PLAN_8['all'], PLAN_8['tone'], COMMIT_8) == (1534028640, 605242880, 1020000000)
assert (PLAN_16['all'], PLAN_16['tone']) == (1469320064, 822291456)
assert (KERNEL['mask'], PLAN_8['mask'], PLAN_16['mask']) == (2345721452, 2937394695, 2592012908)
assert (KERNEL['maskd'], PLAN_8['maskd'], PLAN_16['maskd']) == (1833060664, 2296568710, 2079352120)
# name: (params set, depth)
CASES = {'all-8-24mp': ('all', 8), 'all-16-24mp': ('all', 16),
         'tone-8-24mp': ('tone', 8), 'tone-16-24mp': ('tone', 16),
         'mask-8-24mp': ('mask', 8), 'mask-16-24mp': ('mask', 16),
         'maskd-8-24mp': ('maskd', 8), 'maskd-16-24mp': ('maskd', 16)}
PARAMS = {'all': ALL, 'tone': TONE, 'mask': MASK, 'maskd': MASKD}


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
    params, depth = CASES[case]
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
            ok, reply = call(app, 'apply_filter', document_id=document, filter='develop_filter',
                             **PARAMS[params])
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-develop-filter-admission-')))
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
    gates = {'M8': True, 'M16': True}
    for case in selected:
        params, depth = CASES[case]
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('changed') is True for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        if depth == 8:
            growth = min((r.get('retained_growth', 0) for r in runs), default=0)
            plan = max(PLAN_8[params], COMMIT_8) + growth
        else:
            growth = None
            plan = PLAN_16[params]
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   retained_growth=[r.get('retained_growth') for r in runs], added_growth=growth,
                   plan=plan, need=need, ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        gates[f'M{depth}'] &= row['passed']
        table[case] = row
    report['gates'] = {name: dict(passed=passed) for name, passed in gates.items()
                       if any(CASES[case][1] == int(name[1:]) for case in selected)}
    report['table'] = table
    report['passed'] = all(gate['passed'] for gate in report['gates'].values())
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:14} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline '
              f'{row["baseline_E"]} growth {row["retained_growth"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    for name, gate in report['gates'].items():
        print(f'gate {name}: {"PASS" if gate["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if report['passed'] else 1)


if __name__ == '__main__':
    main()
