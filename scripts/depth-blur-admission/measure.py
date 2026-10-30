#!/usr/bin/env python3
"""The Depth Blur admission drive, on a built Release app.

Filter > Neural Filters > Depth Blur (`depth_blur`) runs in two admitted stages: the CAPTURE
(the layer's 8-bit display, its CGImage and the Core ML depth model, admitted at the catalogue
entry's `inference_bytes` plus `DepthBlurRun.captureBytesPerPixel` (8) per canvas pixel) and
the FILTER (`depth_blur.rs`, admitted by the core's plan through the 8-bit
`filter_op_scratch_bytes`, the 16-bit `selected_edit_plan` or the 32-bit `float_filter_plan`).
This drive measures what each costs on a 6000x4000 layer and holds the plans against it. The
gates are FIXED here before the drive runs; a miss is recorded in `receipt.json` and fails its
gate, and nothing is widened after the fact. A miss raises `inference_bytes` (capture) or
`depth_blur::FIELD_BYTES_PER_PIXEL` (filter) only.

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with
the same document, then `resource_usage` instead of the measured call), `--runs` times. The
store is the shared fixture store ~/devel/doctored-fixtures/models (DR_MODELS_DIR); the drive
FAILS, with no SKIP, when that store holds no ready model. The document is an MCP
`new_document`, 6000x4000, white, one layer, filled by `apply_filter clouds` in two colours
(#203040 and #e0c0a0), at 8, 16 or 32 bits. E is `ri_interval_max_phys_footprint` - pre around
the call, read once the footprint has been still for 2 s, so the commit and the first projection
are inside the interval.

- **inference** (8 bits): `depth_blur` blur_strength 0, focus_subject false — an exact bypass,
  so the call is the capture alone. P = inference_bytes + 8n.
- **blur-8 / blur-16 / blur-32**: `depth_blur` blur_strength 100, focal_distance 0,
  focus_subject false (the field, the disc and the commit). P = max(capture, filter), the filter
  plan by arithmetic from the core's formulas (n = 24,000,000, k = 3, a 2000 x 1333 field of
  F = 2,666,000 px, R = 100):
  K = 16n + ring + 2 MiB + F (FIELD_BYTES_PER_PIXEL + 64) + n (the reduced guide's channel) +
  2 MiB, ring = 201 x 6201 x 40 + 101 x 201 x 4;
  8 bits: (max(K, 20n) + 4 MiB) x 5/4 rounded up; 16 bits: 6 MiB + 8n + max(K, 24n) + 2n;
  32 bits: 6 MiB + max(K + ceil(K/4), 40n) + ceil(2n x 5/4).
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok and changed as expected.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/depth-blur-admission/measure.py [--app <Doctored.app>]
[--output <folder outside the repository>] [--runs 3]; `--app` defaults to `DR_VERIFY_APP`.
Exit 0 only when every gate passes. It imports `owned`'s pieces, `call` and `footprint` from
`scripts/host-still-admission/verify.py`; memory, not wall-clock.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
from verify import call, footprint  # noqa: E402
import support  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
WIDTH, HEIGHT = 6000, 4000
N = WIDTH * HEIGHT
MIB = 1024 * 1024
STORE = Path.home() / 'devel/doctored-fixtures/models'
MODEL_ID = 'apple/coreml-depth-anything-v2-small'
CAPTURE_BYTES_PER_PIXEL = 8
FIELD = 2000 * 1333
RING = 201 * 6201 * 40 + 101 * 201 * 4
EDGE_REFINE = 64


def swift_constant(pattern, path):
    match = re.search(pattern, (ROOT / path).read_text())
    assert match, (pattern, path)
    return int(match.group(1))


INFERENCE = swift_constant(r'"inference_bytes":\s*(\d+)', 'app/Sources/ModelCatalog.swift')
FIELD_BYTES = swift_constant(r'const FIELD_BYTES_PER_PIXEL: u64 = (\d+);', 'core/src/depth_blur.rs')
CAPTURE = INFERENCE + CAPTURE_BYTES_PER_PIXEL * N
KERNEL = 16 * N + RING + 2 * MIB + FIELD * (FIELD_BYTES + EDGE_REFINE) + N + 2 * MIB


def filter_plan(depth):
    if depth == 8:
        base = max(KERNEL, 20 * N) + 4 * MIB
        return base + math.ceil(base / 4)
    if depth == 16:
        return 6 * MIB + 8 * N + max(KERNEL, 24 * N) + 2 * N
    return 6 * MIB + max(KERNEL + math.ceil(KERNEL / 4), 40 * N) + 2 * N + math.ceil(2 * N / 4)


# case: (depth, depth_blur arguments, plan, expect a change)
CASES = {
    'inference': (8, dict(blur_strength=0, focus_subject=False), CAPTURE, False),
    'blur-8': (8, dict(blur_strength=100, focal_distance=0, focus_subject=False), max(CAPTURE, filter_plan(8)), True),
    'blur-16': (16, dict(blur_strength=100, focal_distance=0, focus_subject=False), max(CAPTURE, filter_plan(16)), True),
    'blur-32': (32, dict(blur_strength=100, focal_distance=0, focus_subject=False), max(CAPTURE, filter_plan(32)), True),
}


class StoreApp(support.HostApp):
    """`HostApp` with DR_MODELS_DIR left at the fixture store, every other library private."""
    PRESET_DIRECTORIES = {name: leaf for name, leaf in support.HostApp.PRESET_DIRECTORIES.items()
                          if name != 'DR_MODELS_DIR'}


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def settle(pid, timeout=240):
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
    depth, arguments, _, _ = CASES[case]
    record = dict(case=case, label=label, baseline=baseline, depth=depth)
    app = StoreApp(output / f'app-{label}', output / f'recovery-{label}', executable)
    with app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        ok, listed = call(app, 'list_models')
        record['model_state'] = next((m['state'] for m in listed['models'] if m['id'] == MODEL_ID), None) \
            if ok else listed
        document = make_document(app, depth)
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            ok, reply = call(app, 'depth_blur', document_id=document, layer=0, **arguments)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            record['changed'] = reply.get('changed') if ok else None
            if not ok:
                record['refusal'] = reply[:800]
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ['DR_VERIFY_APP']) if os.environ.get('DR_VERIFY_APP')
                        else None)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    arguments = parser.parse_args()
    if arguments.app is None:
        parser.error('name the Release app with --app or DR_VERIFY_APP')
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    marker = STORE / MODEL_ID.replace('/', '--')
    if not any((version / 'verified.json').is_file() for version in marker.glob('*')):
        print(f'FAIL: no ready model in the fixture store {STORE}; run scripts/fill_model_fixtures.py')
        return 1
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-depth-blur-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    os.environ['DR_MODELS_DIR'] = str(STORE)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=MINIMUM_MAC_LIMIT,
                  plans=dict(inference_bytes=INFERENCE, capture=CAPTURE, field_bytes_per_pixel=FIELD_BYTES,
                             kernel=KERNEL, filter_8=filter_plan(8), filter_16=filter_plan(16),
                             filter_32=filter_plan(32)),
                  records=[], gates={})
    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "call"}-{run}'
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
        _, _, plan, changes = CASES[case]
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('changed') is changes
                                and r.get('model_state') == 'ready' for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins], plan=plan,
                   need=need, ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][case] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:9} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline {row["baseline_E"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
