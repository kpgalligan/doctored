#!/usr/bin/env python3
"""The Auto-Blend admission drive, on a built Release app.

`AutoBlendWork` admits the core's plan for the blend (`dr_doc_auto_blend_plan`
/ `dr_precision_doc_auto_blend_plan`: the kernel's working set, the new mask
planes and the merged layer) before the job runs, and `auto_blend_layers`
reports the largest charge it admitted as `working_memory_plan_bytes`. This
drive measures what the call really costs and pins the plan against it. The
gate is FIXED here before the drive runs; a miss is recorded in `receipt.json`
and fails the gate, and nothing is widened after the fact. A miss is repaired
by raising `focus_stack.rs`'s `BLEND_BYTES_PER_PIXEL` (or
`WINNERS_BYTES_PER_PIXEL`), never by a new constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with the
same setup, then `resource_usage` instead of the measured call), `--runs`
times. Fixtures are `jpeg8` at 6000x4000, 4032x3024 and 2000x1500, authored into this
run's private folder by `scripts/host-still-admission/author.swift` (a colour
ramp with +-12 codes of noise per channel). E is
`ri_interval_max_phys_footprint` - pre around the one `auto_blend_layers`
call, read once the footprint has been still for 2 s, so the blend, the
publication and the new projection are all inside the interval. The setup ends
with `resource_usage` `evict_caches: true`: the blend's admission evicts the
reusable caches anyway, and with them still held at the start of the interval
the footprint would fall by more than the blend raises it and E would read 0.

THE SETUP makes the blend allocate. A stack of one file is N identical layers,
which one frame would win everywhere, and the kernel answers a one-winner stack
without building a pyramid. So layer k is box-blurred (radius 3) everywhere but
its own vertical strip k of N: frame k then wins strip k, every frame is a
member of the blend, and the reply's shares are each about 1/N. A run where
any share is under half of 1/N fails the gate.

Cases:
- `blend-8-24mp`: jpeg8 x 8 at 6000x4000, an 8-bit stack;
- `blend-16-24mp`: the same stack after `convert_precision` to 16 bits;
- `blend-float-12mp`: `core/tests/fixtures/float_11f/scene.tiff` (48x32,
  32-bit float) then jpeg8 x 4 at 4032x3024, a float stack; the measured call
  blends its four converted layers, 1 to 4;
- `masks-8-24mp`, `masks-16-24mp`, `masks-float-12mp`: the three stacks above
  with `seamless` off, so the call only scores the frames and sets the masks.
  The plan is then the kernel's `WINNERS_BYTES_PER_PIXEL` and the masks, with
  no blend charge above it.
- `blend-8-32x`, `blend-16-32x`, `blend-float-32x` (`seamless` on) and
  `masks-16-32x` (off): 32 layers of jpeg8 at 2000x1500 (the float stack the
  scene then 32 of them, blending 1 to 32), so the mask planes, charged at 5/4
  each (`doc_auto_blend::mask_charge`), are most of the plan: the margin at a
  frame count past where a plane charged only its own bytes fell under 1.25.

- `panorama-8-4x24mp`, `panorama-16-4x24mp` (`seamless` on) and
  `panorama-masks-8-4x24mp`, `panorama-masks-16-4x24mp` (off): Panorama
  over a 19200x4000 strip of jpeg8 x 4 at 6000x4000, 8-bit or after
  `convert_precision` to 16 bits; `panorama-float-4x12mp` (on): the
  scene then jpeg8 x 4 at 4032x3024 on a 12528x3024 strip, blending 1 to 4.
  THE PANORAMA SETUP is its own: `load_files_into_stack`, `canvas_size` to the
  strip (anchor top-left), then `move_layers` moves the k-th blended layer by
  k steps (4400 or 2832 px, so neighbours overlap by 1600 or 1200 px), no blur:
  a panorama's frames are not focus-stacked, the seams and the blend allocate
  on any overlap. The plan is the larger of the gains, the seams and
  `WINNERS_BYTES_PER_PIXEL` (with `seamless`, the blend's too), the masks, and
  with `seamless` the frames' written-back rasters (`doc_auto_blend::
  kernel_plan`); a masks-only miss raises `WINNERS_BYTES_PER_PIXEL` (then the
  `masks-` cases are re-run), a seamless one `BLEND_BYTES_PER_PIXEL`.

- **M-blend** for every case, the reply's `working_memory_plan_bytes` >=
  ceil(1.25 x max E) over its runs, `seamless` on in the `blend-` and
  `panorama-` cases and off in the `masks-` and `panorama-masks-` cases, and
  every frame's share at least half of 1/N.

Usage: python3 -B scripts/auto-blend-admission/measure.py --app <Doctored.app>
[--output <folder outside the repository>] [--runs 3] [--cases a,b]. Exit 0
only when the gate passes.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
AUTHOR = ROOT / 'scripts/host-still-admission/author.swift'
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
from verify import call, footprint, owned  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
LARGE = (6000, 4000)
SMALL = (4032, 3024)
MID = (2000, 1500)
FLOAT_SCENE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
BLUR_RADIUS = 3
STRIP = (19200, 4000)
STRIP_STEP = 4400
FLOAT_STRIP = (12528, 3024)
FLOAT_STEP = 2832
# Panorama cases: name -> (the frames' canvas as loaded, the step between frames).
PANORAMA = {
    'panorama-8-4x24mp': (LARGE, STRIP_STEP),
    'panorama-16-4x24mp': (LARGE, STRIP_STEP),
    'panorama-masks-8-4x24mp': (LARGE, STRIP_STEP),
    'panorama-masks-16-4x24mp': (LARGE, STRIP_STEP),
    'panorama-float-4x12mp': (SMALL, FLOAT_STEP),
}
# name: (the stack's files, the layers blended, convert to 16 bits first, the canvas, seamless)
CASES = {
    'blend-8-24mp': (['jpeg8-24'] * 8, list(range(8)), False, LARGE, True),
    'blend-16-24mp': (['jpeg8-24'] * 8, list(range(8)), True, LARGE, True),
    'blend-float-12mp': (['scene'] + ['jpeg8-12'] * 4, [1, 2, 3, 4], False, SMALL, True),
    'masks-8-24mp': (['jpeg8-24'] * 8, list(range(8)), False, LARGE, False),
    'masks-16-24mp': (['jpeg8-24'] * 8, list(range(8)), True, LARGE, False),
    'masks-float-12mp': (['scene'] + ['jpeg8-12'] * 4, [1, 2, 3, 4], False, SMALL, False),
    'blend-8-32x': (['jpeg8-3'] * 32, list(range(32)), False, MID, True),
    'blend-16-32x': (['jpeg8-3'] * 32, list(range(32)), True, MID, True),
    'blend-float-32x': (['scene'] + ['jpeg8-3'] * 32, list(range(1, 33)), False, MID, True),
    'masks-16-32x': (['jpeg8-3'] * 32, list(range(32)), True, MID, False),
    'panorama-8-4x24mp': (['jpeg8-24'] * 4, list(range(4)), False, STRIP, True),
    'panorama-16-4x24mp': (['jpeg8-24'] * 4, list(range(4)), True, STRIP, True),
    'panorama-masks-8-4x24mp': (['jpeg8-24'] * 4, list(range(4)), False, STRIP, False),
    'panorama-masks-16-4x24mp': (['jpeg8-24'] * 4, list(range(4)), True, STRIP, False),
    'panorama-float-4x12mp': (['scene'] + ['jpeg8-12'] * 4, [1, 2, 3, 4], False, FLOAT_STRIP,
                              True),
}


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def author(output):
    """Build the author, then write the fixtures this drive loads."""
    folder = output / 'fixtures'
    made = {'jpeg8-24': (folder / 'jpeg8-24.jpg', LARGE), 'jpeg8-12': (folder / 'jpeg8-12.jpg', SMALL),
            'jpeg8-3': (folder / 'jpeg8-3.jpg', MID)}
    if not all(path.is_file() for path, _ in made.values()):
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        folder.mkdir(parents=True, exist_ok=True)
        for kind, (path, size) in made.items():
            subprocess.run([str(binary), kind.split('-')[0], str(size[0]), str(size[1]), str(path)],
                           check=True, capture_output=True, timeout=600)
    return dict({kind: path for kind, (path, _) in made.items()}, scene=FLOAT_SCENE)


def settle(pid, timeout=120):
    """Waits until the footprint has been still for 2 s; answers it."""
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def must(app, tool, **arguments):
    ok, reply = call(app, tool, **arguments)
    assert ok, (tool, reply)
    return reply


def setup(app, fixtures, case):
    """The stack, each blended layer blurred everywhere but its own strip; a
    Panorama case's frames stepped along the strip instead."""
    files, layers, convert, canvas, _ = CASES[case]
    stack = must(app, 'load_files_into_stack', paths=[str(fixtures[name]) for name in files])
    document = stack['document']['id']
    loaded = PANORAMA[case][0] if case in PANORAMA else canvas
    assert stack['canvas'] == dict(width=loaded[0], height=loaded[1]), stack['canvas']
    if convert:
        must(app, 'convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if case in PANORAMA:
        must(app, 'canvas_size', document_id=document, width=canvas[0], height=canvas[1],
             anchor='top-left')
        step = PANORAMA[case][1]
        for position, layer in enumerate(layers):
            if position:
                must(app, 'move_layers', document_id=document, layers=[layer],
                     dx=position * step, dy=0)
        must(app, 'resource_usage', evict_caches=True)
        return document
    count = len(layers)
    for position, layer in enumerate(layers):
        left = position * canvas[0] // count
        right = (position + 1) * canvas[0] // count
        must(app, 'select_all', document_id=document)
        must(app, 'select_rect', document_id=document, x=left, y=0, width=right - left,
             height=canvas[1], mode='subtract')
        must(app, 'apply_filter', document_id=document, layer=layer, filter='box_blur',
             radius=BLUR_RADIUS)
    must(app, 'deselect', document_id=document)
    # The blend's own admission evicts the reusable caches the setup left
    # behind (the layers' contiguous views), which would hide its growth
    # inside the interval. Evict them first, so the interval starts where the
    # blend does.
    must(app, 'resource_usage', evict_caches=True)
    return document


def measure_case(output, executable, fixtures, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    layers, seamless = CASES[case][1], CASES[case][4]
    record = dict(case=case, label=label, baseline=baseline, seamless=seamless)
    with owned(output, label, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        document = setup(app, fixtures, case)
        record['precision'] = must(app, 'get_precision', document_id=document).get(
            'bits_per_channel')
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        started = time.monotonic()
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            method = 'panorama' if case in PANORAMA else 'stack_images'
            ok, reply = call(app, 'auto_blend_layers', document_id=document, layers=layers,
                             seamless=seamless, method=method)
        record['seconds'] = round(time.monotonic() - started, 2)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if not ok:
                record['refusal'] = reply[:800]
            else:
                shares = [entry.get('share') for entry in reply.get('layers', [])]
                record.update(plan=reply.get('working_memory_plan_bytes'), shares=shares,
                              merged_layer=reply.get('merged_layer'),
                              every_frame_won=(len(shares) == len(layers) and all(
                                  share >= 0.5 / len(layers) for share in shares)))
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-auto-blend-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    fixtures = author(output)
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), author_sha256=digest(AUTHOR),
                  runs=arguments.runs, working_limit_bytes=MINIMUM_MAC_LIMIT,
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size,
                                       sha256=digest(path)) for name, path in fixtures.items()},
                  records=[], gates={})

    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                try:
                    record = measure_case(output, executable, fixtures, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)

    table = {}
    everything = True
    for case in selected:
        measured = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = (bool(measured) and all(r.get('ok') and r.get('every_frame_won') for r in measured)
              and all(r.get('ok') for r in baselines))
        plans = {r.get('plan') for r in measured}
        max_e = max((r.get('E', 0) for r in measured), default=0)
        plan = next(iter(plans)) if len(plans) == 1 else None
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in measured], baseline_E=[r.get('E') for r in baselines],
                   seconds=[r.get('seconds') for r in measured], plan=plan, need=need,
                   ratio=(plan / max_e) if plan and max_e else None,
                   passed=ok and plan is not None and plan >= need)
        everything &= row['passed']
        table[case] = row
    report['table'] = table
    report['gates']['M-blend'] = dict(passed=everything)
    report['passed'] = everything
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:20} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(f'gate M-blend: {"PASS" if everything else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if everything else 1)


if __name__ == '__main__':
    main()
