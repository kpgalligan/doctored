#!/usr/bin/env python3
"""The contact-sheet admission drive, on a built Release app.

`ContactSheet.compose` admits `ContactSheetAdmission.composeScratch` (F +
a·sheet px + b·largest cell px) before the compose runs as a job, and the MCP
reply reports it as `working_memory_plan_bytes`. This drive measures what a
contact sheet really costs and pins the plan against it. The gates are FIXED
here before the drive runs; a miss is recorded in `receipt.json` and fails its
gate, and nothing is widened after the fact.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with the
same sources, then `resource_usage` instead of the sheet), `--runs` times.
Sources are MCP `new_document`s (white, 8-bit): S12 4000x3000 sRGB, S48
8000x6000, S96 12000x8000, P48 8000x6000 `display_p3`, and L48 (S48 plus a
`new_layer`). E is `ri_interval_max_phys_footprint` − pre around
`contact_sheet_from_sources {documents}`, read once the footprint has been
still for 2 s, so the delivered document's window and projection are inside
the interval.

Cases (output a new document unless noted):
- `one-S12`, `one-S48`, `one-S96`, `one-P48`, `one-L48`: one cell, the default
  layout;
- `sheet-57`: S12 x196, 14 columns, 512x512 cells;
- `sheet-57-export`: the same, exported as PNG to a private folder;
- `sheet-98`: S12 x196, 14 columns, 680x680 cells.

- **M** for every case, the reply's `working_memory_plan_bytes` ≥
  ⌈1.25 × max E⌉ over its runs.
- **R** in a fresh app at `DR_WORKING_LIMIT_BYTES` 1 GiB, `sheet-98` from S12
  is refused with `RasterResources.admit`'s "…on its own…" sentence, and the
  `list_documents` count is unchanged.

Usage: python3 -B scripts/contact-sheet-admission/measure.py --app
<Doctored.app> [--output <folder outside the repository>] [--runs 3]. Exit 0
only when both gates pass.
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
REFUSAL_LIMIT = 1073741824
SOURCES = {
    'S12': dict(width=4000, height=3000, profile='srgb', layers=1),
    'S48': dict(width=8000, height=6000, profile='srgb', layers=1),
    'S96': dict(width=12000, height=8000, profile='srgb', layers=1),
    'P48': dict(width=8000, height=6000, profile='display_p3', layers=1),
    'L48': dict(width=8000, height=6000, profile='srgb', layers=2),
}
SHEET_57 = dict(columns=14, cell_width=512, cell_height=512)
SHEET_98 = dict(columns=14, cell_width=680, cell_height=680)
# name: (source, cell count, layout or None for the default, export?)
CASES = {
    'one-S12': ('S12', 1, None, False),
    'one-S48': ('S48', 1, None, False),
    'one-S96': ('S96', 1, None, False),
    'one-P48': ('P48', 1, None, False),
    'one-L48': ('L48', 1, None, False),
    'sheet-57': ('S12', 196, SHEET_57, False),
    'sheet-57-export': ('S12', 196, SHEET_57, True),
    'sheet-98': ('S12', 196, SHEET_98, False),
}
REFUSAL_SENTENCE = 'on its own'


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


def make_source(app, name):
    spec = SOURCES[name]
    ok, made = call(app, 'new_document', width=spec['width'], height=spec['height'], depth=8,
                    profile=spec['profile'], background='white')
    assert ok, made
    document = made['document']['id']
    for _ in range(spec['layers'] - 1):
        ok, layer = call(app, 'new_layer', document_id=document)
        assert ok, layer
    return document


def sheet_arguments(document, count, layout, export_folder):
    arguments = dict(documents=[document] * count)
    if layout is not None:
        arguments['layout'] = layout
    if export_folder is not None:
        export_folder.mkdir(parents=True, exist_ok=True)
        arguments['output'] = dict(kind='export',
                                   settings={'format': 'png', 'bit_depth': 8, 'profile': 'srgb'},
                                   destination=str(export_folder), pattern='{name}')
    return arguments


def measure_case(output, executable, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    source, count, layout, export = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    with owned(output, label, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        document = make_source(app, source)
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        else:
            folder = output / f'export-{label}' if export else None
            ok, reply = call(app, 'contact_sheet_from_sources',
                             **sheet_arguments(document, count, layout, folder))
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if ok:
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              width=reply.get('width'), height=reply.get('height'),
                              cells=reply.get('cells'))
            else:
                record['refusal'] = reply[:800]
    return record


def gate_r(output, executable):
    """`sheet-98` from S12 at 1 GiB: refused on its own, no new document."""
    previous = os.environ.get('DR_WORKING_LIMIT_BYTES')
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(REFUSAL_LIMIT)
    record = {}
    try:
        with owned(output, 'gate-r', executable) as app:
            ok, usage = call(app, 'resource_usage')
            record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
            document = make_source(app, 'S12')
            ok, before = call(app, 'list_documents')
            assert ok, before
            _, count, layout, _ = CASES['sheet-98']
            ok, reply = call(app, 'contact_sheet_from_sources',
                             **sheet_arguments(document, count, layout, None))
            ok_after, after = call(app, 'list_documents')
            assert ok_after, after
            record.update(refused=not ok, reply=reply if not ok else None,
                          documents_before=len(before['documents']),
                          documents_after=len(after['documents']))
            record['passed'] = (
                record['working_limit_bytes'] == REFUSAL_LIMIT and not ok
                and REFUSAL_SENTENCE in reply
                and record['documents_before'] == record['documents_after'])
    except Exception as error:                      # Recorded, never repaired.
        record.update(passed=False, error=f'{type(error).__name__}: {error}'[:800])
    finally:
        if previous is None:
            os.environ.pop('DR_WORKING_LIMIT_BYTES', None)
        else:
            os.environ['DR_WORKING_LIMIT_BYTES'] = previous
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-contact-sheet-admission-')))
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
                label = f'{case}-{"baseline" if baseline else "sheet"}-{run}'
                try:
                    record = measure_case(output, executable, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)

    table = {}
    passed_m = True
    for case in selected:
        sheets = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(sheets) and all(r.get('ok') for r in sheets)
        plans = {r.get('plan') for r in sheets}
        max_e = max((r.get('E', 0) for r in sheets), default=0)
        plan = next(iter(plans)) if len(plans) == 1 else None
        need = math.ceil(1.25 * max_e)
        row = dict(E=[r.get('E') for r in sheets], baseline_E=[r.get('E') for r in baselines],
                   plan=plan, need=need, ratio=(plan / max_e) if plan and max_e else None,
                   sheet=(sheets[0].get('width'), sheets[0].get('height')) if sheets else None,
                   passed=ok and plan is not None and plan >= need)
        passed_m &= row['passed']
        table[case] = row
    report['table'] = table
    report['gates']['M'] = dict(passed=passed_m)
    report['gates']['R'] = gate_r(output, executable)
    passed = passed_m and report['gates']['R'].get('passed', False)
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        print(f'{case:16} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {"PASS" if row["passed"] else "FAIL"}', flush=True)
    print(f'gate M: {"PASS" if passed_m else "FAIL"}', flush=True)
    print(f'gate R: {"PASS" if report["gates"]["R"].get("passed") else "FAIL"} '
          f'{json.dumps(report["gates"]["R"])[:600]}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
