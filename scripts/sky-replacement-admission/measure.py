#!/usr/bin/env python3
"""The Sky Replacement admission drive, on a built Release app.

`sky_replacement` runs three admitted stages one after another: the sky model's mask (Select ▸
Sky's own job, `ModelMaskRun.coverage`, planned at the sky entry's `inference_bytes` +
`ModelMaskRun.captureBytesPerPixel` × n + 2 × 4 × `maximumPlaneSamples` + 2n — the M1 plan),
its edge refinement (self-admitted), then the replacement job (`SkyReplacementRun.apply`:
`dr_sky_replace_scratch_bytes`, the reply's `planned_bytes`). The plan of the call is
P = max(M1 plan, planned_bytes): the replacement's own plan must cover its need by itself (M3a's
fix: M2b passed only because the prepared display of the result, `PreparedDocumentEdit`'s own
projection estimate, was charged beside it). That display estimate is still read here, as
`resource_usage`'s `projection_scratch_estimate_bytes` after the call, and recorded in each row
as `display_bytes`, but it is not added to P. The gates are FIXED here before the drive runs; a
miss is recorded in `receipt.json` and fails its gate, and nothing is widened after the fact. A
miss raises `sky_harmonize.rs`'s `SCRATCH_BYTES_PER_PIXEL` (Duplicate Layer) or `sky_group.rs`'s
`NEW_LAYERS_BYTES_8|16` (New Layers) only, and the record goes on that constant's doc comment.

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app
with the same document, then `resource_usage` instead of the measured call), `--runs` times.
The document is the phase-6 sky.jpg opened with `open_document`, resized with `image_size`
(catmull-rom) to 6000 × 4000 and, for `rgba16`, converted with `convert_precision`. E is
`ri_interval_max_phys_footprint` − pre around the one call, read once the footprint has been
still for 2 s.

- **rgba8** / **rgba16** (the defaults): `sky_replacement` with the library's first sky,
  fade_edge 3, lighting 50, color_adjustment 50, edge_lighting 50, decontaminate true (the
  dialog's default, which adds the Foreground Edge layer and keeps the coverage and the 8-bit
  canvas copy alive to the end) and output new_layers (the Sky Replacement Group, M2b). **rgba8-duplicate** / **rgba16-duplicate** (opt-in through
  --cases): the same with output duplicate_layer (M2a's). (32 bits is not a case: the app
  disables the command there.)
- Gate per case: P ≥ ceil(1.25 × max E) over its runs, every run ok and changed.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/sky-replacement-admission/measure.py --app <Doctored.app>
--output <folder outside the repository> [--runs 3] [--cases rgba8,rgba16]. Exit 0 only when
every gate passes. Imports `call` and `footprint` from `scripts/host-still-admission/verify.py`
and `HostApp` from `scripts/host-source-probe/support.py`; memory, not wall-clock: run it only
when no other lane is building.
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
PHOTO = Path.home() / 'devel/doctored-fixtures/phase6/photos/sky.jpg'
CATALOG = (ROOT / 'app/Sources/ModelCatalog.swift').read_text()
SELECTION = (ROOT / 'app/Sources/ModelSelection.swift').read_text()
SKY_MODEL = 'twistlabs/fotometis-sky'
LIBRARY = 'doctored/sky-library'
WIDTH, HEIGHT = 6000, 4000
SETTINGS = dict(fade_edge=3, lighting=50, color_adjustment=50, edge_lighting=50, decontaminate=True)
# case: (bits, output); the first two are the default cases.
CASES = {'rgba8': (8, 'new_layers'), 'rgba16': (16, 'new_layers'),
         'rgba8-duplicate': (8, 'duplicate_layer'), 'rgba16-duplicate': (16, 'duplicate_layer')}
DEFAULT_CASES = ['rgba8', 'rgba16']


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


def mask_plan():
    """The M1 plan of the sky model's job (`scripts/model-selection-admission/measure.py`'s)."""
    n = WIDTH * HEIGHT
    return inference_bytes(SKY_MODEL) + CAPTURE_BYTES_PER_PIXEL * n + 2 * 4 * PLANE_SAMPLES + 2 * n


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


def measure_case(output, executable, label, case, baseline):
    record = dict(case=case, label=label, baseline=baseline)
    previous = os.environ.get('DR_MODELS_DIR')
    os.environ['DR_MODELS_DIR'] = str(STORE)
    try:
        app = StoreApp(output / f'app-{label}', output / f'recovery-{label}', executable)
        with app:
            ok, usage = call(app, 'resource_usage')
            record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
            ok, listed = call(app, 'list_skies')
            assert ok and listed.get('skies'), listed
            sky = listed['skies'][0]['id']
            copy = output / f'{label}-sky.jpg'
            shutil.copyfile(PHOTO, copy)
            ok, opened = call(app, 'open_document', path=str(copy))
            assert ok, opened
            document = opened['document']['id']
            ok, resized = call(app, 'image_size', document_id=document, width=WIDTH, height=HEIGHT,
                               filter='catmull-rom')
            assert ok, resized
            bits, output_kind = CASES[case]
            record['output'] = output_kind
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
                ok, reply = call(app, 'sky_replacement', document_id=document, layer=0, sky=sky,
                                 output=output_kind, **SETTINGS)
            settle(pid)
            post, peak = footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if ok:
                    record['changed'] = reply.get('changed')
                    record['planned_bytes'] = reply.get('planned_bytes')
                    record['model'] = reply.get('model', {}).get('id')
                    ok_usage, usage = call(app, 'resource_usage')
                    record['display_bytes'] = usage.get('projection_scratch_estimate_bytes') if ok_usage else None
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
    parser.add_argument('--cases', default=','.join(DEFAULT_CASES))
    arguments = parser.parse_args()
    if arguments.app is None:
        parser.error('name the Release app with --app or DR_VERIFY_APP')
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    for model in (SKY_MODEL, LIBRARY):
        folder = model_store_paths.model_folder(STORE, model)
        if not any((version / 'verified.json').is_file() for version in folder.glob('*')):
            print(f'FAIL: no ready {model} in the fixture store {STORE}; run scripts/fill_model_fixtures.py')
            return 1
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-sky-replacement-admission-')))
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
                  working_limit_bytes=MINIMUM_MAC_LIMIT, mask_plan=mask_plan(),
                  settings=SETTINGS, records=[], gates={})
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
        ok = bool(runs) and all(r.get('ok') and r.get('changed') and r.get('model') == SKY_MODEL
                                and isinstance(r.get('planned_bytes'), int)
                                and isinstance(r.get('display_bytes'), int) for r in runs)
        limit_ok = all(r.get('working_limit_bytes') == MINIMUM_MAC_LIMIT for r in runs + twins)
        replace_plan = max((r.get('planned_bytes', 0) for r in runs if ok), default=0)
        case_plan = max(mask_plan(), replace_plan)
        max_e = max((r.get('E', 0) for r in runs), default=0)
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in runs], baseline_E=[r.get('E') for r in twins],
                   display_bytes=[r.get('display_bytes') for r in runs],
                   mask_plan=mask_plan(), replace_plan=replace_plan, plan=case_plan, need=need,
                   ratio=(case_plan / max_e) if max_e else None,
                   passed=ok and limit_ok and bool(twins) and max_e > 0 and case_plan >= need)
        passed &= row['passed']
        table[case] = row
        report['gates'][case] = dict(passed=row['passed'])
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:16} plan {row["plan"]} (mask {row["mask_plan"]}, replace {row["replace_plan"]}) '
              f'need {row["need"]} ratio {row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
