#!/usr/bin/env python3
"""Replay preserved pre-9H byte and native paint outputs and old Actions.

Expected samples come from immutable earlier Release apps, never the candidate.
Both direct legacy arguments and imported legacy Action steps must agree exactly.
"""
import argparse
import base64
import json
from pathlib import Path
import runpy
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
f = runpy.run_path(str(ROOT/'scripts/verify_phase9f.py'))
probe, shared = f['probe'], f['shared']
BASELINES = {
    8: '0495bc7007f1822d40cd41445e1062476fe5f045e37d6e669241c8f90611a28b',
    16: '9ac0804e5a457b05cc995bd5590a7e95b6156bda675aac5f99987e1dd1527fdd',
}


def dependencies():
    result = f['dependencies']()
    result[str(Path(__file__).relative_to(ROOT))] = f['digest'](__file__)
    return result


def stored(path):
    native = probe.read_native(path)
    doc = native['document']
    images = [(i, probe.png_samples(data)) for i, data in sorted(doc['fallbacks'].items())]
    return dict(width=doc['width'], height=doc['height'], depth=doc['depth'],
                profile=doc['profile'], layers=doc['layers'], channels=doc['channels'],
                pixels=[(i, p['width'], p['height'], p['depth'], tuple(p['samples'])) for i, p in images])


def capture(call, path):
    call('save_copy', path=str(path))
    return stored(path)


def selection(call):
    samples = call('get_selection', points=[dict(x=x, y=y) for y in range(f['HEIGHT'])
                                            for x in range(f['WIDTH'])])['samples']
    return tuple(p['coverage16'] for p in samples)


def byte_selection(call, baseline):
    records = [json.loads(line) for line in (baseline/'calls.jsonl').read_text().splitlines()]
    sample = next(r['result']['samples'] for r in records if r['tool'] == 'get_selection')
    values = bytes(p['coverage'] for p in sample)
    assert len(values) == f['WIDTH']*f['HEIGHT']
    runs = b''.join(struct.pack('<IBB', 1, v, 255) for v in values)
    call('set_quick_mask', active=True)
    call('quick_mask_stroke', width=f['WIDTH'], height=f['HEIGHT'],
         coverage_rle=base64.b64encode(runs).decode())
    call('set_quick_mask', active=False)
    actual = call('get_selection', points=[dict(x=p['x'], y=p['y']) for p in sample])['samples']
    assert bytes(p['coverage'] for p in actual) == values


def run_depth(folder, executable, baseline, depth):
    folder.mkdir(parents=True)
    original_report = json.loads((baseline/'report.json').read_text())
    assert original_report['passed'] and original_report['app_sha256'] == BASELINES[depth]
    fixture = baseline/f'fixture-{depth}-alpha.png'
    assert f['digest'](fixture) == original_report['fixture_sha256']
    expected_original = stored(baseline/'original.rz')
    rows = []
    baseline_files = [baseline/'report.json', baseline/'original.rz', fixture,
                      *(Path(c['native']) for c in original_report['cases'])]
    if depth == 16:
        baseline_files += list((baseline/'input.drsnapshot').rglob('*'))
    else:
        baseline_files.append(baseline/'calls.jsonl')
    hashes = {str(p): f['digest'](p) for p in baseline_files if p.is_file()}
    with probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        call = probe.opened(app, fixture, depth, folder/'calls.jsonl')
        if depth == 16:
            identifier = call('load_snapshot', path=str(baseline/'input.drsnapshot'))['snapshot_id']
            call('restore_snapshot', snapshot_id=identifier)
        else:
            byte_selection(call, baseline)
        assert capture(call, folder/'original.rz') == expected_original
        original_selection = selection(call)
        for index, case in enumerate(original_report['cases']):
            expected = stored(Path(case['native']))
            changed = expected != expected_original
            assert changed == case['changed']
            before = probe.history(call)
            call(case['tool'], **case['arguments'])
            assert capture(call, folder/f'direct-{index}.rz') == expected, (depth, index, case['tool'])
            assert selection(call) == original_selection
            assert probe.history(call)[1] == before[1] + int(changed)
            if changed:
                call('undo'); assert capture(call, folder/f'undo-{index}.rz') == expected_original
                call('redo'); assert capture(call, folder/f'redo-{index}.rz') == expected
                call('undo')
            # The stored pre-9H recording is preferred; the older byte receipt
            # preserves exactly the same original unversioned tool arguments.
            steps = case.get('action_steps', [dict(tool=case['tool'], arguments=case['arguments'])])
            name = f'Historical {depth} paint {index}'
            app.call('save_action', name=name, steps=steps)
            before = probe.history(call)
            call('run_action', name=name)
            assert capture(call, folder/f'action-{index}.rz') == expected, (depth, index, 'Action')
            assert selection(call) == original_selection
            assert probe.history(call)[1] == before[1] + int(changed)
            if changed:
                call('undo'); assert capture(call, folder/f'action-undo-{index}.rz') == expected_original
            rows.append(dict(tool=case['tool'], arguments=case['arguments'], changed=changed,
                             exact_direct=True, exact_legacy_action=True, exact_undo_redo=True))
    assert hashes == {p: f['digest'](p) for p in hashes}
    return dict(depth=depth, cases=rows, baseline_sha256=BASELINES[depth],
                baseline_file_hashes=hashes, original_samples_properties_and_selection_preserved=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_executable())
    parser.add_argument('--byte-baseline', type=Path, required=True)
    parser.add_argument('--native-baseline', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    args = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9h-compat-'))
    before = dependencies()
    report = dict(passed=False, scratch=str(folder), app_sha256=f['digest'](args.app))
    try:
        report['depths'] = [run_depth(folder/str(depth), args.app.resolve(), baseline, depth)
                            for depth, baseline in [(8, args.byte_baseline), (16, args.native_baseline)]]
        after = dependencies()
        report['source_check'] = dict(changes=sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k)),
                                      files=len(before), file_set_exact=before.keys()==after.keys())
        if args.require_current:
            assert not report['source_check']['changes'], report['source_check']
        assert f['digest'](args.app) == report['app_sha256']
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['all_owned_apps_exited'] = all(a.process is None or a.process.poll() is not None for a in shared.HostApp.owners)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True)+'\n')
        print(args.report, flush=True)


if __name__ == '__main__':
    main()
