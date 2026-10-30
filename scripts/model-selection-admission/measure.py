#!/usr/bin/env python3
"""The model-selection admission drive, on a built Release app.

Select > Sky (`select_sky`), Select > Person Parts... (`select_person_parts`) and the Object
Finder (`object_finder_select`) run their model inside ONE admitted job
(`ModelMaskRun.coverage`): the crop, the Core ML prediction, `dr_model_mask_coverage` and the
union, at P = the entry's `inference_bytes` + `ModelMaskRun.captureBytesPerPixel` x n +
`coverageScratch` (2 x 4 x `maximumPlaneSamples` + 2n) + n per clipped frame. The edge stage
that follows is its own self-admitted job, so every case passes `refine: false`, which ends the
call after the inference job: the measure is the inference job alone. The gates are FIXED here
before the drive runs; a miss is recorded in `receipt.json` and fails its gate, and nothing is
widened after the fact. A miss raises the entry's `inference_bytes` or `captureBytesPerPixel`
only.

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with
the same document, then `resource_usage` instead of the measured call), `--runs` times. The
document is a phase-6 photo opened with `open_document` and resized with `image_size` (catmull-rom);
E is `ri_interval_max_phys_footprint` - pre around the measured call, read once the footprint has
been still for 2 s.

- **sky**: sky.jpg at 6000 x 4000, `select_sky`. One frame.
- **parts**: portrait.png at 6000 x 6000, `select_person_parts` all four parts. The call also
  runs Vision's people analysis and each person's clip (their own admitted jobs, earlier in the
  same call), so E is an upper bound of the inference job's; P counts one clip per person found.
- **finder-large**: product.jpg at 6000 x 4000, `object_finder_select` with a box around the
  camera (scaled from the 2640 x 1860 original), SAM 2.1 Large.
- **finder-tiny**: the same through a scratch DR_MODELS_DIR holding only the fixture store's
  Tiny folder (a symlink) and a copy of its acceptances.json, so the Object Finder runs Tiny.
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok with the expected model.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/model-selection-admission/measure.py [--app <Doctored.app>]
[--output <folder outside the repository>] [--runs 3] [--cases sky,parts,...]; `--app` defaults
to `DR_VERIFY_APP`. Exit 0 only when every gate passes. Imports `call` and `footprint` from
`scripts/host-still-admission/verify.py` and `HostApp` from `scripts/host-source-probe/support.py`;
memory, not wall-clock: run it only when no other lane is building.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
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
PHOTOS = Path.home() / 'devel/doctored-fixtures/phase6/photos'
CATALOG = (ROOT / 'app/Sources/ModelCatalog.swift').read_text()
SELECTION = (ROOT / 'app/Sources/ModelSelection.swift').read_text()


def inference_bytes(model_id):
    match = re.search(r'"id": "' + re.escape(model_id) + r'".*?"inference_bytes": (\d+)', CATALOG, re.S)
    assert match, model_id
    return int(match.group(1))


def swift_constant(pattern):
    match = re.search(pattern, SELECTION)
    assert match, pattern
    return int(match.group(1).replace('_', ''))


CAPTURE_BYTES_PER_PIXEL = swift_constant(r'static let captureBytesPerPixel: UInt64 = ([\d_]+)')
PLANE_SAMPLES = 1 << int(swift_constant(r'static let maximumPlaneSamples: UInt64 = 1 << (\d+)'))
SAM_LARGE, SAM_TINY = 'apple/coreml-sam2.1-large', 'apple/coreml-sam2.1-tiny'
# The camera's box in product.jpg's own 2640 x 1860 pixels, by eye.
CAMERA_BOX = (280, 180, 2040, 1490)

# case: (photo, width, height, tool, arguments, model id, clips counted)
CASES = {
    'sky': ('sky.jpg', 6000, 4000, 'select_sky', {}, 'twistlabs/fotometis-sky'),
    'parts': ('portrait.png', 6000, 6000, 'select_person_parts',
              dict(parts=['hair', 'face_skin', 'body_skin', 'clothes']),
              'doctored/mediapipe-selfie-multiclass-256x256'),
    'finder-large': ('product.jpg', 6000, 4000, 'object_finder_select', None, SAM_LARGE),
    'finder-tiny': ('product.jpg', 6000, 4000, 'object_finder_select', None, SAM_TINY),
}


def finder_box(width, height):
    x, y, w, h = CAMERA_BOX
    sx, sy = width / 2640, height / 1860
    return dict(x=round(x * sx), y=round(y * sy), width=round(w * sx), height=round(h * sy))


def plan(case, clips):
    _, width, height, _, _, model = CASES[case]
    n = width * height
    coverage = 2 * 4 * PLANE_SAMPLES + 2 * n
    return inference_bytes(model) + CAPTURE_BYTES_PER_PIXEL * n + coverage + clips * n


class StoreApp(support.HostApp):
    """`HostApp` with DR_MODELS_DIR left at the environment's store, every other library private."""
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


def tiny_store(output):
    """A store holding only SAM 2.1 Tiny: the fixture folder linked, the acceptances copied."""
    store = output / 'tiny-store'
    store.mkdir(parents=True, exist_ok=True)
    link = store / SAM_TINY.replace('/', '--')
    if not link.exists():
        link.symlink_to(STORE / SAM_TINY.replace('/', '--'))
    shutil.copyfile(STORE / 'acceptances.json', store / 'acceptances.json')
    return store


def measure_case(output, executable, label, case, baseline):
    photo, width, height, tool, arguments, model = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    previous = os.environ.get('DR_MODELS_DIR')
    os.environ['DR_MODELS_DIR'] = str(tiny_store(output) if case == 'finder-tiny' else STORE)
    try:
        app = StoreApp(output / f'app-{label}', output / f'recovery-{label}', executable)
        with app:
            ok, usage = call(app, 'resource_usage')
            record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
            copy = output / f'{label}-{photo}'
            shutil.copyfile(PHOTOS / photo, copy)
            ok, opened = call(app, 'open_document', path=str(copy))
            assert ok, opened
            document = opened['document']['id']
            ok, resized = call(app, 'image_size', document_id=document, width=width, height=height,
                               filter='catmull-rom')
            assert ok, resized
            pid = app.process.pid
            settle(pid)
            pre, _ = footprint(pid, reset=True)
            if baseline:
                ok, reply = call(app, 'resource_usage')
            else:
                extra = dict(arguments) if arguments is not None else dict(box=finder_box(width, height))
                ok, reply = call(app, tool, document_id=document, refine=False, **extra)
            settle(pid)
            post, peak = footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if ok:
                    record['model'] = reply.get('model', {}).get('id')
                    record['person_count'] = reply.get('person_count')
                    record['selection_empty'] = reply.get('selection_empty')
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
    for case in selected:
        folder = model_store_paths.model_folder(STORE, CASES[case][5])
        if not any((version / 'verified.json').is_file() for version in folder.glob('*')):
            print(f'FAIL: no ready {CASES[case][5]} in the fixture store {STORE}; '
                  'run scripts/fill_model_fixtures.py')
            return 1
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-model-selection-admission-')))
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
                  constants=dict(capture_bytes_per_pixel=CAPTURE_BYTES_PER_PIXEL,
                                 maximum_plane_samples=PLANE_SAMPLES,
                                 inference_bytes={CASES[c][5]: inference_bytes(CASES[c][5]) for c in CASES}),
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
        model = CASES[case][5]
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('model') == model for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        clips = max((r.get('person_count') or 0 for r in runs), default=0) if case == 'parts' else 0
        case_plan = plan(case, clips)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins], plan=case_plan,
                   clips=clips, need=need, ratio=(case_plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and case_plan >= need)
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
