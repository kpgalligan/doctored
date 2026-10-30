#!/usr/bin/env python3
"""The Perspective Warp admission drive, on a built Release app.

Edit > Perspective Warp (`perspective_warp`) resolves the quads, meshes the
planes (two projective triangles a plane, or a 64 x 64 affine sub-grid a quad
for a joined group whose homographies disagree), pins and solves the
extension grid (at most 3,000 vertices) and resamples the layer through the
map; the float form resamples the selection's float through the
selected-pixels walk. This drive measures what each costs on a 6000x4000
layer and holds the plan it is admitted by against it. The gates are FIXED
here before the drive runs; a miss is recorded in `receipt.json` and fails
its gate, and nothing is widened after the fact. A miss raises
`puppet_render.rs`'s `MARGIN_BYTES_PER_PIXEL` only.

Every case runs in its own fresh owned `HostApp`, on its own scratch MCP
socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline
twin (a fresh app with the same document, then `resource_usage` instead of the
measured call), `--runs` times. The document is an MCP `new_document`,
6000x4000, white, one layer, filled by `apply_filter clouds` in two colours
(#203040 and #e0c0a0), at 8, 16 or 32 bits. The layer cases warp one keystone
quad over most of the layer, (300, 300), (5700, 300), (5700, 3700),
(300, 3700) to (500, 300), (5500, 300), (5800, 3700), (200, 3700); the float
case selects (1000, 800, 4000, 2400) first and warps the quad on the
selection's corners, its bottom corners pulled out by 100 px and its top ones
in by 200; the joined case warps two quads that share the edge x = 3000,
each pulling one far corner the other way, so their homographies disagree
along it (an affine group at n = 64). E is `ri_interval_max_phys_footprint`
- pre around that call, read once the footprint has been still for 2 s, so
the commit and the first projection are inside the interval.

The plans, by arithmetic from the core's formulas (n = 24,000,000):
- WARP, `perspective_warp::scratch`: `GRID_BYTES` (1 MiB) + `SOLVER_BYTES`
  (8 x 3000 x 3001 / 2 + 4 MiB) + `MAP_BYTES` (20,000,000 + 2 MiB) +
  `TRIANGLE_BYTES` (256) x the plane triangles (2 for one projective quad,
  2 x 2 x 64 x 64 for the joined pair) + 2 MiB.
- Layer form, the core plan's `scratch_bytes` (`doc_puppet::mesh_warp_plan`),
  with D the warped layer's area (read back from `get_document`) and
  M = `MARGIN_BYTES_PER_PIXEL` = 4: 8 bits WARP + 4n + D(4 + M); 16 bits
  WARP + 8n + D(8 + M); 32 bits WARP + 16n + D(16 + M) + 16D + 64 per 256-px
  tile of D. The host admits that plus the 24n projection
  (`MeshWarpWork.prepareLayer`): P = scratch + 24n.
- Float form at 8 bits, the selected-pixels plan the host admits
  (`MoveSelectedPixels.prepared`, `bounded_scratch` with B the selection and
  the landing the whole canvas): P = 16|B| + 4n + 16n + WARP + 4 MiB.

- **L8**, **L16**, **L32**, **J8**: P >= ceil(1.25 x max E) over that case's
  runs.
- **F8**: P >= ceil(1.25 x max E) over the 8-bit float runs.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/perspective-warp-admission/measure.py [--app <Doctored.app>]
[--output <folder outside the repository>] [--runs 3]; `--app` defaults to
`DR_VERIFY_APP`, resolved against the working directory. Exit 0 only when every
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
MARGIN = 4
TILE = 256
GRID = MIB
SOLVER = 8 * (3000 * 3001 // 2) + 4 * MIB
MAP = 20 * 1_000_000 + 2 * MIB
TRIANGLE = 256
SELECTION = (1000, 800, 4000, 2400)


def flat(points):
    return [v for p in points for v in p]


KEYSTONE = [{'from': flat([(300, 300), (5700, 300), (5700, 3700), (300, 3700)]),
             'to': flat([(500, 300), (5500, 300), (5800, 3700), (200, 3700)])}]
FLOAT_QUAD = [{'from': flat([(1000, 800), (5000, 800), (5000, 3200), (1000, 3200)]),
               'to': flat([(1200, 800), (4800, 800), (5100, 3200), (900, 3200)])}]
JOINED = [{'from': flat([(300, 300), (3000, 300), (3000, 3700), (300, 3700)]),
           'to': flat([(600, 500), (3000, 300), (3000, 3700), (300, 3700)])},
          {'from': flat([(3000, 300), (5700, 300), (5700, 3700), (3000, 3700)]),
           'to': flat([(3000, 300), (5700, 300), (5400, 3500), (3000, 3700)])}]
# case: (depth, float form, quads, plane triangles)
CASES = {'layer-8': (8, False, KEYSTONE, 2), 'layer-16': (16, False, KEYSTONE, 2),
         'layer-32': (32, False, KEYSTONE, 2), 'float-8': (8, True, FLOAT_QUAD, 2),
         'joined-8': (8, False, JOINED, 2 * 2 * 64 * 64)}


def warp_plan(triangles):
    return GRID + SOLVER + MAP + TRIANGLE * triangles + 2 * MIB


FLOAT_PLAN = 16 * SELECTION[2] * SELECTION[3] + 4 * N + 16 * N + warp_plan(2) + 4 * MIB
assert (SOLVER, MAP) == (40206304, 22097152)


def layer_plan(depth, dest, triangles):
    """The host's admitted scratch for the layer form over a D = dest area."""
    w, h = dest
    d = w * h
    common = warp_plan(triangles)
    if depth == 8:
        core = common + 4 * N + d * (4 + MARGIN)
    elif depth == 16:
        core = common + 8 * N + d * (8 + MARGIN)
    else:
        tiles = math.ceil(w / TILE) * math.ceil(h / TILE)
        core = common + 16 * N + d * (16 + MARGIN) + 16 * d + 64 * tiles
    return core + 24 * N


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def settle(pid, timeout=180):
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


def measure_case(output, executable, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    depth, float_form, quads, _ = CASES[case]
    record = dict(case=case, label=label, baseline=baseline, depth=depth, float=float_form)
    app = owned(output, label, executable)
    with app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        document = make_document(app, depth)
        if float_form:
            x, y, w, h = SELECTION
            ok, selected = call(app, 'select_rect', document_id=document, x=x, y=y, width=w,
                                height=h)
            assert ok, selected
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            ok, reply = call(app, 'perspective_warp', document_id=document, quads=quads)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            record['changed'] = reply.get('changed') if ok else None
            if not ok:
                record['refusal'] = reply[:800]
            else:
                ok, state = call(app, 'get_document', document_id=document)
                row = state['layers'][0] if ok else {}
                record['dest'] = [row.get('width'), row.get('height')]
                record['shared_edges'] = len(reply.get('shared_edges') or [])
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ['DR_VERIFY_APP']) if os.environ.get('DR_VERIFY_APP')
                        else None,
                        help='the Release Doctored.app (default: DR_VERIFY_APP)')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES),
                        help='comma list of cases, from ' + ','.join(CASES))
    arguments = parser.parse_args()
    if arguments.app is None:
        parser.error('name the Release app with --app or DR_VERIFY_APP')
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-perspective-warp-admission-')))
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
                  plans=dict(grid=GRID, solver=SOLVER, map=MAP, float_8=FLOAT_PLAN,
                             keystone=warp_plan(2), joined=warp_plan(2 * 2 * 64 * 64)),
                  records=[], gates={})

    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "warp"}-{run}'
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
        depth, float_form, _, triangles = CASES[case]
        if float_form:
            plan = FLOAT_PLAN
        else:
            dests = {tuple(r.get('dest') or ()) for r in runs}
            dest = min(dests) if len(dests) == 1 and all(dests) else None
            plan = layer_plan(depth, dest, triangles) if dest and None not in dest else 0
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   dest=[r.get('dest') for r in runs], plan=plan, need=need,
                   ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        letter = 'F' if float_form else 'J' if case.startswith('joined') else 'L'
        report['gates'][f'{letter}{depth}'] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:9} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline '
              f'{row["baseline_E"]} dest {row["dest"]} {"PASS" if row["passed"] else "FAIL"}',
              flush=True)
    for name, gate in report['gates'].items():
        print(f'gate {name}: {"PASS" if gate["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
