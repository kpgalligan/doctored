#!/usr/bin/env python3
"""The M3a Remove tool Model-mode admission drive, on a built Release app.

`remove_stroke` in mode "model" runs ONE admitted job (`RemoveModelRun.start` through
`DocumentJobs.start`): the core's native input (`dr_*_remove_model_input`, the sampling plate
included), the MI-GAN prediction and the core's composite (`dr_*_remove_model_apply`), at
P = the reply's `planned_bytes` = the core's plan (`dr_*_remove_model_plan`) + the entry's
`inference_bytes` + `RemoveModelRun.hostBytesPerPixel` x n. The gates are FIXED here before the
drive runs; a miss is recorded in `receipt.json` and fails its gate, and nothing is widened after
the fact. A miss raises the entry's `inference_bytes` (to the least MiB multiple that clears it,
the record on `ModelCatalog`'s comment) and then `hostBytesPerPixel` only.

Every case runs in its own fresh owned app, on its own scratch MCP socket, at
`DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with
the same document, then `resource_usage` instead of the measured call), `--runs` times. The
document is the phase-6 sky.jpg opened with `open_document` and resized with `image_size`
(catmull-rom) to 6000 x 4000; `rgba16` cases convert it with `convert_precision`. E is
`ri_interval_max_phys_footprint` - pre around the one call, read once the footprint has been
still for 2 s. The stroke: size 400 from (1500, 1500) to (3500, 2500).

- **rgba8-all** / **rgba16-all**: layer 0, sample all.
- **rgba8-current** / **rgba16-current**: layer 0, sample current.
- **rgba8-below** / **rgba16-below**: the layer duplicated (`duplicate_layer`), target layer 1,
  sample current_and_below.
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok, completed, mode model.
The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/remove-model-admission/measure.py --app <Doctored.app>
--output <folder outside the repository> [--runs 3] [--cases rgba8-all,...]. Exit 0 only when
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
MODEL = 'doctored/migan-512-places2'
WIDTH, HEIGHT = 6000, 4000
STROKE = dict(points=[[1500, 1500], [3500, 2500]], size=400)
# case: (bits, sample, duplicated)
CASES = {
    'rgba8-all': (8, 'all', False), 'rgba16-all': (16, 'all', False),
    'rgba8-current': (8, 'current', False), 'rgba16-current': (16, 'current', False),
    'rgba8-below': (8, 'current_and_below', True), 'rgba16-below': (16, 'current_and_below', True),
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
            bits, sample, duplicated = CASES[case]
            if bits == 16:
                ok, converted = call(app, 'convert_precision', document_id=document, mode='rgba16',
                                     structure='preserve')
                assert ok, converted
            layer = 0
            if duplicated:
                ok, copied = call(app, 'duplicate_layer', document_id=document, index=0)
                assert ok, copied
                layer = 1
            pid = app.process.pid
            settle(pid)
            pre, _ = footprint(pid, reset=True)
            if baseline:
                ok, reply = call(app, 'resource_usage')
            else:
                ok, reply = call(app, 'remove_stroke', document_id=document, layer=layer,
                                 sample=sample, **STROKE)
            settle(pid)
            post, peak = footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if ok:
                    for key in ('state', 'mode', 'model', 'planned_bytes', 'crop', 'covered_pixels'):
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
    folder = model_store_paths.model_folder(STORE, MODEL)
    if not any((version / 'verified.json').is_file() for version in folder.glob('*')):
        print(f'FAIL: no ready {MODEL} in the fixture store {STORE}; run scripts/fill_model_fixtures.py')
        return 1
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-remove-model-admission-')))
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
                  working_limit_bytes=MINIMUM_MAC_LIMIT, stroke=STROKE, records=[], gates={})
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
        ok = bool(runs) and all(r.get('ok') and r.get('state') == 'completed' and r.get('mode') == 'model'
                                and str(r.get('model', '')).startswith(MODEL + ' @ ')
                                and isinstance(r.get('planned_bytes'), int) for r in runs)
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
        print(f'{case:16} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline {row["baseline_E"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
