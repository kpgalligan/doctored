#!/usr/bin/env python3
"""The Puppet Warp admission drive, on a built Release app.

Edit > Puppet Warp (`puppet_warp`) builds a mesh over the layer (the offset
contour's two f32 distance planes over the layer grown by |e| + 1), solves the
pins on at most 3,000 vertices and resamples the layer through the pose; the
float form resamples the selection's float through the selected-pixels walk.
This drive measures what each costs on a 6000x4000 layer and holds the plan it
is admitted by against it. The gates are FIXED here before the drive runs; a
miss is recorded in `receipt.json` and fails its gate, and nothing is widened
after the fact. A miss raises `puppet_render.rs`'s `MARGIN_BYTES_PER_PIXEL`.

Every case runs in its own fresh owned `HostApp`, on its own scratch MCP
socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline
twin (a fresh app with the same document, then `resource_usage` instead of the
measured call), `--runs` times. The document is an MCP `new_document`,
6000x4000, white, one layer, filled by `apply_filter clouds` in two colours
(#203040 and #e0c0a0), at 8, 16 or 32 bits. Every layer case pins the four
points (300, 300), (5700, 300), (300, 3700) and (5700, 3700) in place and
moves a fifth from (3000, 2000) to (3060, 2040); the float case selects
(1000, 800, 4000, 2400) first and moves (3000, 2000) to (3060, 2040) with
(1200, 1000) held. E is `ri_interval_max_phys_footprint` - pre around that
call, read once the footprint has been still for 2 s, so the commit and the
first projection are inside the interval.

The plans, by arithmetic from the core's formulas (n = 24,000,000, e = 2):
- MESH, `puppet_mesh::mesh_plan_bytes` over a w x h rect: with W = w + 6 and
  H = h + 6, 9WH (the u8 mask and the two f32 distance planes) +
  24(W/4 + 2)(H/4 + 2) (the kept cells and corners) + 24 max(W, H) (the
  distance transform's lines) + 1 MiB.
- COMMON, `doc_puppet::common_scratch`: MESH(6000, 4000) + `SOLVER_BYTES`
  (8 x 3000 x 3001 / 2 + 4 MiB) + `MAP_BYTES` (20,000,000 + 2 MiB; 1 MiB when
  measured, before Perspective Warp widened it) + 2 MiB.
- Layer form, the core plan's `scratch_bytes`, with D the warped layer's area
  (read back from `get_document`) and M = `MARGIN_BYTES_PER_PIXEL` = 4:
  8 bits COMMON + 4n + D(4 + M); 16 bits COMMON + 8n + D(8 + M); 32 bits
  COMMON + 16n + D(16 + M) + 16D + 64 per 256-px tile of D. The host admits
  that plus the 24n projection (`MeshWarpWork.prepareLayer`): P = scratch + 24n.
- Float form at 8 bits, the selected-pixels plan the host admits
  (`MoveSelectedPixels.prepared`, `bounded_scratch` with B the selection):
  P = 16|B| + 4n + 16n + MESH(B) + `SOLVER_BYTES` + `MAP_BYTES` + 4 MiB.

- **L8**, **L16**, **L32**: P >= ceil(1.25 x max E) over that depth's layer runs.
- **F8**: P >= ceil(1.25 x max E) over the 8-bit float runs.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/puppet-warp-admission/measure.py --app <Doctored.app>
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
MARGIN = 4
TILE = 256
SOLVER = 8 * (3000 * 3001 // 2) + 4 * MIB
MAP = 20 * 1_000_000 + 2 * MIB
SELECTION = (1000, 800, 4000, 2400)
HELD = [(300, 300), (5700, 300), (300, 3700), (5700, 3700)]
MOVED = ((3000, 2000), (3060, 2040))
# case: (depth, float form)
CASES = {'layer-8': (8, False), 'layer-16': (16, False), 'layer-32': (32, False),
         'float-8': (8, True)}


def mesh_plan(width, height, expansion=2):
    grow = abs(expansion) + 1
    w, h = width + 2 * grow, height + 2 * grow
    plane = w * h
    distances = plane * 8 if expansion else 0
    return plane + distances + (w // 4 + 2) * (h // 4 + 2) * 24 + max(w, h) * 24 + MIB


COMMON = mesh_plan(WIDTH, HEIGHT) + SOLVER + MAP + 2 * MIB
FLOAT_PLAN = (16 * SELECTION[2] * SELECTION[3] + 4 * N + 16 * N
              + mesh_plan(SELECTION[2], SELECTION[3]) + SOLVER + MAP + 4 * MIB)
assert (SOLVER, MAP) == (40206304, 22097152)


def layer_plan(depth, dest):
    """The host's admitted scratch for the layer form over a D = dest area."""
    w, h = dest
    d = w * h
    if depth == 8:
        core = COMMON + 4 * N + d * (4 + MARGIN)
    elif depth == 16:
        core = COMMON + 8 * N + d * (8 + MARGIN)
    else:
        tiles = math.ceil(w / TILE) * math.ceil(h / TILE)
        core = COMMON + 16 * N + d * (16 + MARGIN) + 16 * d + 64 * tiles
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


def pins(float_form):
    held = [(1200, 1000)] if float_form else HELD
    out = [{'from': list(p), 'to': list(p)} for p in held]
    out.append({'from': list(MOVED[0]), 'to': list(MOVED[1])})
    return out


def measure_case(output, executable, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    depth, float_form = CASES[case]
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
            ok, reply = call(app, 'puppet_warp', document_id=document, pins=pins(float_form))
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
                record['vertices'] = reply.get('vertices')
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-puppet-warp-admission-')))
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
                  plans=dict(common=COMMON, solver=SOLVER, map=MAP, float_8=FLOAT_PLAN),
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
        depth, float_form = CASES[case]
        if float_form:
            plan = FLOAT_PLAN
        else:
            dests = {tuple(r.get('dest') or ()) for r in runs}
            dest = min(dests) if len(dests) == 1 and all(dests) else None
            plan = layer_plan(depth, dest) if dest and None not in dest else 0
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   dest=[r.get('dest') for r in runs], plan=plan, need=need,
                   ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][f'{"F" if float_form else "L"}{depth}'] = dict(passed=row['passed'])
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
