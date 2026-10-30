#!/usr/bin/env python3
"""The Neural Filters admission drive (M4 phases 1 and 2), on a built Release app.

`super_zoom` runs ONE admitted `BackgroundWork` job (`SuperZoomRun.run`): the layer's canvas
image, FBCNN's stage when remove_jpeg_artifacts is set, Real-ESRGAN's stage (`dr_*_neural_stage`:
Noise Reduction on the source, the tiles, Sharpen on the output) and the new document, which then
opens; `jpeg_artifacts_removal` runs ONE admitted `DocumentJobs` job (`JpegArtifactsRemovalRun`);
`neural_colorize` runs ONE `DocumentJobs` job for a layer output (`ColorizeRun.start`) or ONE
`BackgroundWork` job whose new document then opens (`ColorizeRun.runNewDocument`), its P the core
plan (`dr_neural_colorize_plan`) + DDColor's `inference_bytes` + the host term x the pixels (+ the
canvas source for New Document).
P is the reply's `planned_bytes`: each stage's core plan (`dr_neural_stage_info`) + its model's
`inference_bytes` + `NeuralFilterRun.hostBytesPerOutputPixel` x the output pixels (Super Zoom also
the canvas source and, under the second stage, FBCNN's output). The gates are FIXED here before
the drive runs; a miss is recorded in `receipt.json` and fails its gate, and nothing is widened
after the fact. A miss raises the entry's `inference_bytes` (the least MiB multiple that clears
it, the record on `ModelCatalog`'s comment) or `hostBytesPerOutputPixel` (the least whole value).

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with
the same document, then `resource_usage` instead of the measured call), `--runs` times. The
document is the phase-6 sky.jpg opened with `open_document` and resized with `image_size`
(catmull-rom) to 6000 x 4000 (24 MP); `16` cases convert it with `convert_precision`. E is
`ri_interval_max_phys_footprint` - pre around the one call, read once the footprint has been
still for 2 s.

- **zoom8** / **zoom16**: `super_zoom` zoom 2, remove_jpeg_artifacts true (FBCNN chained before
  Real-ESRGAN), 12000 x 8000 out.
- **sliders8** / **sliders16**: `super_zoom` zoom 2 with noise_reduction 50 and sharpen 50 (the
  stage's Reduce Noise and Unsharp Mask terms).
- **jpeg8** / **jpeg16**: `jpeg_artifacts_removal` strength medium, output layer.
- **colorize8** / **colorize16**: `neural_colorize` output layer with every slider set
  (saturation 20, the three balances 10, -10, 10, color_artifact_reduction 50).
- **colordoc8** / **colordoc16**: `neural_colorize` output new_document.
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/neural-filters-admission/measure.py --app <Doctored.app>
--output <folder outside the repository> [--runs 3] [--cases zoom8,...]. Exit 0 only when every
gate passes. Imports `call` and `footprint` from `scripts/host-still-admission/verify.py` and
`HostApp` from `scripts/host-source-probe/support.py`; memory, not wall-clock: run it only when
no other lane is building.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
from verify import call, footprint  # noqa: E402
import model_store_paths  # noqa: E402
import support  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
STORE = Path.home() / 'devel/doctored-fixtures/models'
PHOTO = Path.home() / 'devel/doctored-fixtures/phase6/photos/sky.jpg'
MODELS = ['doctored/realesrgan-x4plus', 'doctored/fbcnn-color', 'doctored/ddcolor']
WIDTH, HEIGHT = 6000, 4000
# case: (bits, tool, arguments)
ZOOM = dict(zoom=2, remove_jpeg_artifacts=True)
SLIDERS = dict(zoom=2, noise_reduction=50, sharpen=50)
JPEG = dict(strength='medium', output='layer')
COLORIZE = dict(saturation=20, cyan_red=10, magenta_green=-10, yellow_blue=10, color_artifact_reduction=50,
                output='layer')
COLORDOC = dict(output='new_document')
CASES = {
    'zoom8': (8, 'super_zoom', ZOOM), 'zoom16': (16, 'super_zoom', ZOOM),
    'sliders8': (8, 'super_zoom', SLIDERS), 'sliders16': (16, 'super_zoom', SLIDERS),
    'jpeg8': (8, 'jpeg_artifacts_removal', JPEG), 'jpeg16': (16, 'jpeg_artifacts_removal', JPEG),
    'colorize8': (8, 'neural_colorize', COLORIZE), 'colorize16': (16, 'neural_colorize', COLORIZE),
    'colordoc8': (8, 'neural_colorize', COLORDOC), 'colordoc16': (16, 'neural_colorize', COLORDOC),
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


def settle(pid, timeout=600):
    """Waits until the footprint has been still for 2 s; answers it."""
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def measure_case(output, executable, label, case, baseline):
    record = dict(case=case, label=label, baseline=baseline)
    previous = os.environ.get('DR_MODELS_DIR')
    os.environ['DR_MODELS_DIR'] = str(STORE)
    try:
        app = StoreApp(output / f'app-{label}', output / f'recovery-{label}', executable)
        with app:
            ok, usage = call(app, 'resource_usage')
            record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
            copy = output / f'{label}-sky.jpg'
            shutil.copyfile(PHOTO, copy)
            ok, opened = call(app, 'open_document', path=str(copy))
            assert ok, opened
            document = opened['document']['id']
            ok, resized = call(app, 'image_size', document_id=document, width=WIDTH, height=HEIGHT,
                               filter='catmull-rom')
            assert ok, resized
            bits, tool, arguments = CASES[case]
            if bits == 16:
                ok, converted = call(app, 'convert_precision', document_id=document, mode='rgba16',
                                     structure='preserve')
                assert ok, converted
            pid = app.process.pid
            settle(pid)
            pre, _ = footprint(pid, reset=True)
            if baseline:
                ok, reply = call(app, 'resource_usage')
            else:
                ok, reply = call(app, tool, document_id=document, layer=0, **arguments)
            settle(pid)
            post, peak = footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if ok:
                    for key in ('planned_bytes', 'tiles', 'width', 'height', 'changed', 'models', 'model',
                                'output'):
                        record[key] = reply.get(key)
                else:
                    record['refusal'] = reply[:800]
    finally:
        if previous is None:
            os.environ.pop('DR_MODELS_DIR', None)
        else:
            os.environ['DR_MODELS_DIR'] = previous
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
    for model in MODELS:
        folder = model_store_paths.model_folder(STORE, model)
        if not any((version / 'verified.json').is_file() for version in folder.glob('*')):
            print(f'FAIL: no ready {model} in the fixture store {STORE}; run scripts/fill_model_fixtures.py')
            return 1
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-neural-filters-admission-')))
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
                  working_limit_bytes=MINIMUM_MAC_LIMIT, records=[], gates={})
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
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and isinstance(r.get('planned_bytes'), int) for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        plan = min((r.get('planned_bytes', 0) for r in runs), default=0) if ok else 0
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   planned=[r.get('planned_bytes') for r in runs], plan=plan, need=need,
                   ratio=(plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][case] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:12} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline {row["baseline_E"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
