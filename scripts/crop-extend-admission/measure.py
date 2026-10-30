#!/usr/bin/env python3
"""The crop-past-the-canvas admission drive, on a built Release app.

MCP `crop` with a rectangle past the canvas runs the core's signed straighten (the identity at
angle 0, `RasterDocument.extendedCrop`; turned, `straightenedCrop`'s one canvas geometry job) over
the Background padded with the background colour (`doc_crop_extend`), at P = the reply's
`planned_bytes` = the core's plan of that request (`geometryEditPlan(request:)["scratch_bytes"]`:
the op's plan over the padded document plus the pad). The gates are FIXED here before the drive
runs; a miss is recorded in `receipt.json` and fails its gate, and nothing is widened after the
fact. A miss raises the plan's existing factor, never a new constant.

The generative-fill protocol (`scripts/generative-fill-admission/measure.py`): every case runs in
its own fresh owned app, on its own scratch MCP socket, at `DR_WORKING_LIMIT_BYTES` 8 GiB with
`MallocLargeCache=0`, beside a baseline twin (a fresh app with the same document, then
`resource_usage` instead of the measured call), `--runs` times. The document is `new_document`
4800 x 3200 filled #808080 at the case's depth (an opaque Background), then `apply_filter`
add_noise (amount 1, seed 665) in BOTH the case and its twin. E is
`ri_interval_max_phys_footprint` - pre around the one call, read once the footprint has been
still for 2 s.

- **extend8** / **extend16**: `crop` x -600, y -400, 6000 x 4000 (24 MP), angle 0,
  delete_cropped true (so the Background is padded), background_color #FFFFFF.
- **extendrot8**: the same at angle 5 (sampler bicubic).
- **fillcaf8** / **fillgen8** (the Crop tool's Fill): extend8's document and window
  with `fill` "content_aware" / "generative_expand" (prompt "a red boat", three variations), one
  waited "Crop" job each, against the generative-fill admission's own OpenAI stub (`Stub` and
  `seed_cloud`, imported from `scripts/generative-fill-admission/measure.py`; dummy key, no
  network). P is the reply's `planned_bytes` (`CropFillRun`: the larger of the crop's plan and
  the fill's plan beside the cropped base); the size is read with `get_document` after the call.
  A miss raises the fill's existing margin (`GenerativeFillRun.hostBytesPerPixel`, or the
  Content-Aware term the bisect names), never a new constant.
- Gate per case: P >= ceil(1.25 x max E) over its runs, every run ok, `extended` true and the
  document 6000 x 4000. The baseline twins' E is recorded beside each row, not gated.

Usage: python3 -B scripts/crop-extend-admission/measure.py --app <Doctored.app>
--output <folder outside the repository> [--runs 3] [--cases extend8,extend16,extendrot8].
Exit 0 only when every gate passes. Imports `call` and `footprint` from
`scripts/host-still-admission/verify.py` and `HostApp` from `scripts/host-source-probe/support.py`;
memory, not wall-clock: run it only when no other lane is building.
"""
import argparse
import hashlib
import importlib.util
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
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
from verify import call, footprint  # noqa: E402
import support  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    'generative_fill_admission', ROOT / 'scripts/generative-fill-admission/measure.py')
generative = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(generative)
Stub, seed_cloud = generative.Stub, generative.seed_cloud

MINIMUM_MAC_LIMIT = 8589934592
PICTURE = (4800, 3200)
WINDOW = dict(x=-600, y=-400, width=6000, height=4000)
# case: (depth, angle, fill)
CASES = {'extend8': (8, 0, None), 'extend16': (16, 0, None), 'extendrot8': (8, 5, None),
         'fillcaf8': (8, 0, 'content_aware'), 'fillgen8': (8, 0, 'generative_expand')}
PROMPT = 'a red boat'


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
    depth, angle, fill = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    folder = output / f'app-{label}'
    if fill == 'generative_expand':
        seed_cloud(folder)
    app = support.HostApp(folder, output / f'recovery-{label}', executable)
    with app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        ok, made = call(app, 'new_document', width=PICTURE[0], height=PICTURE[1], depth=depth,
                        background='#808080')
        assert ok, made
        document = made['document']['id'] if 'document' in made else made['id']
        ok, noised = call(app, 'apply_filter', document_id=document, layer=0, filter='add_noise',
                          amount=1, seed=665)
        assert ok, noised
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            extra = dict(fill=fill) if fill else {}
            if fill == 'generative_expand':
                extra['prompt'] = PROMPT
            ok, reply = call(app, 'crop', document_id=document, angle=angle, sampler='bicubic',
                             delete_cropped=True, background_color='#FFFFFF', **WINDOW, **extra)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if ok:
                summary = reply.get('document') or {}
                if fill:
                    record.update(state=reply.get('state'), fill=reply.get('fill'))
                    _, summary = call(app, 'get_document', document_id=document)
                record.update(extended=reply.get('extended'),
                              planned_bytes=reply.get('planned_bytes'),
                              size=[summary.get('width'), summary.get('height')])
            else:
                record['refusal'] = str(reply)[:800]
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-crop-extend-admission-')))
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
                  working_limit_bytes=MINIMUM_MAC_LIMIT, window=WINDOW, records=[], gates={})
    previous = os.environ.get('DR_OPENAI_BASE_URL')
    with Stub() as stub:
        os.environ['DR_OPENAI_BASE_URL'] = stub.base
        try:
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
        finally:
            if previous is None:
                os.environ.pop('DR_OPENAI_BASE_URL', None)
            else:
                os.environ['DR_OPENAI_BASE_URL'] = previous
        report['stub_requests'] = stub.requests
    table = {}
    passed = True
    for case in selected:
        runs = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        twins = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(runs) and all(r.get('ok') and r.get('extended') is True
                                and r.get('size') == [WINDOW['width'], WINDOW['height']]
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
        print(f'{case:10} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} baseline {row["baseline_E"]} '
              f'{"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
