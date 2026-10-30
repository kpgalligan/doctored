#!/usr/bin/env python3
"""The Gaussian Blur admission drive, on a built Release app.

Filter > Blur > Gaussian Blur runs the fitted kernel from radius 7 up
(`core/src/gaussian_blur_filter.rs`): one 128-row band at a time at 8 and 16
bits, the whole plane on a 32-bit float layer. This drive measures what it
costs at 24 MP and holds the plan each depth is admitted by against it. The
gates are FIXED here before the drive runs; a miss is recorded in
`receipt.json` and fails its gate, and nothing is widened after the fact. A
miss raises the bookkeeping-allowance count of `gaussian_blur_filter::plan`,
except where the publication term binds (M8's commit stage, M16's display
preparation): there it raises that stage's existing margin.

Every case runs in its own fresh owned `HostApp`, on its own scratch MCP
socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline
twin (a fresh app with the same document, then `resource_usage` instead of the
measured call), `--runs` times. The document is an MCP `new_document`,
6000x4000, one layer, filled by `apply_filter clouds` in two colours (#203040
and #e0c0a0), at 8, 16 or 32 bits. The measured call is `apply_filter blur`
at sigma (Radius) 25 or 64. E is `ri_interval_max_phys_footprint` - pre
around that call, read once the footprint has been still for 2 s, so the
commit and the first projection are inside the interval.

The plans (n = 24,000,000), pinned by the core's default tests
(`core/tests/gaussian_blur_photoshop.rs` `PLANS_24MP`,
`core/tests/float_adjust_apply_24mp.rs`):
- 8 bits (P8): `filter_op_scratch_bytes` = 184,357,760 at r25 and
  218,412,160 at r64, and the host admits the larger of it and
  `ByteFilterAdmission.commitStageBytes` = 1,020,000,000, plus what the edit
  leaves retained (the smallest growth of `unique_retained_bytes` across the
  call): P8 = max(plan, commit) + dR, presence-admission's rule.
- 16 bits (P16): `selected_edit_plan`'s `scratch_bytes` = 485,583,360 at r25
  and 512,826,880 at r64, and the host admits the larger of it and the 16-bit
  display preparation `PreparedDocumentEdit` admits after it,
  `projectionScratchEstimate()` = 795,586,399 (`PROJECTION_16`: the one-layer
  structural 578,608,290 plus `ProjectionMargin`'s m16 = 6 sixteenths), plus
  dR: P16 = max(plan, projection) + dR, the same rule as P8.
- 32 bits (P32): the float `selected_edit_plan`'s `scratch_bytes` =
  1,026,291,456 at both (its second stage binds).

- **M8 / M16 / M32** (one gate per depth, both radii): plan >=
  ceil(1.25 x max E) over that depth's runs at each radius.
The baseline twins' E is recorded beside each row, not gated.

M16 was RE-DECLARED after the first run (receipt `admission-1`, kept, M16 FAIL:
E 576.7 MB against the edit plan alone, need 720.8 MB). That run's E is the
whole call, edit plus the 16-bit display preparation, and is the same 576.7 MB
at r6 on the old kernel; the core-level probe `measure_sixteen_bit_blur`
measures the edit plan itself at 1.255x (r25) and 1.330x (r64). M8 already held
the call against what the host admits for it; M16 now does the same.

Usage: python3 -B scripts/gaussian-blur-admission/measure.py --app <Doctored.app>
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
COMMIT_8 = 8 * N + 24 * N + math.ceil(7 * 24 * N / 16)
PLANS = {(8, 25): 184_357_760, (8, 64): 218_412_160, (16, 25): 485_583_360,
         (16, 64): 512_826_880, (32, 25): 1_026_291_456, (32, 64): 1_026_291_456}
assert COMMIT_8 == 1_020_000_000
# The 16-bit display preparation's charge (`RasterDocument.projectionScratchEstimate()`):
# max(render 24n + 1 MiB, output 8n + display 4n + bookkeeping) + page tables (peak / 2048)
# + 39 edge tiles x 32 KiB, the structural 578,608,290 `ProjectionMargin` records for a
# one-layer 6000x4000 16-bit document (H2, PB-16), plus its m16 = 6 sixteenths.
_RENDER_16 = 24 * N + (1 << 20)
STRUCTURAL_16 = (_RENDER_16 + math.ceil(_RENDER_16 / 2048)
                 + ((WIDTH + 255) // 256 + (HEIGHT + 255) // 256 - 1) * 32 * 1024)
PROJECTION_16 = STRUCTURAL_16 + (STRUCTURAL_16 * 6 + 15) // 16
assert STRUCTURAL_16 == 578_608_290
assert PROJECTION_16 == 795_586_399
# case: (depth, radius)
CASES = {f'blur-{d}-r{r}': (d, r) for d in (8, 16, 32) for r in (25, 64)}


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
    depth, radius = CASES[case]
    record = dict(case=case, label=label, baseline=baseline, depth=depth, radius=radius)
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
        else:
            ok, reply = call(app, 'apply_filter', document_id=document, filter='blur',
                             sigma=radius)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre,
                      retained_before=before, retained_after=retained(app))
        record['retained_growth'] = record['retained_after'] - before
        if not baseline:
            record['changed'] = reply.get('changed', reply.get('ok')) if ok else None
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-gaussian-blur-admission-')))
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
                  working_limit_bytes=MINIMUM_MAC_LIMIT,
                  plans={f'{d}-r{r}': p for (d, r), p in PLANS.items()}, commit_8=COMMIT_8,
                  projection_16=PROJECTION_16,
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
    gates = {}
    for case in selected:
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('changed') is True for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        depth, radius = CASES[case]
        growth = None
        if depth in (8, 16):
            growth = min((r.get('retained_growth', 0) for r in runs), default=0)
            publication = COMMIT_8 if depth == 8 else PROJECTION_16
            plan = max(PLANS[(depth, radius)], publication) + growth
        else:
            plan = PLANS[(depth, radius)]
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   retained_growth=[r.get('retained_growth') for r in runs], added_growth=growth,
                   plan=plan, need=need, ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        table[case] = row
        gates.setdefault(f'M{depth}', []).append(row['passed'])
    report['table'] = table
    report['gates'] = {name: dict(passed=all(rows)) for name, rows in gates.items()}
    passed = all(g['passed'] for g in report['gates'].values())
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
