#!/usr/bin/env python3
"""Independent archive/coverage qualification across the 9F -> 9G boundary.

The baseline directory is captured from the committed 9F app. Native coverage
is authored here as little-endian scalar codes in the documented v3 package;
neither a product-generated selection nor its display bitmap is the oracle.
"""
import argparse
import base64
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('selection_compat_support', ROOT/'scripts/verify_phase9f.py')
s = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s)
BASELINE_SHA = '0495bc7007f1822d40cd41445e1062476fe5f045e37d6e669241c8f90611a28b'


def dependencies():
    result = s.dependencies()
    result[str(Path(__file__).relative_to(ROOT))] = s.digest(__file__)
    return result


def package(source, target, change=None):
    shutil.copytree(source, target)
    path = target/'state.json'
    manifest = json.loads(path.read_text())
    if change:
        change(manifest, target)
    path.write_text(json.dumps(manifest)+'\n')
    return target


def restore(call, path):
    identifier = call('load_snapshot', path=str(path))['snapshot_id']
    call('restore_snapshot', snapshot_id=identifier)


def stored_channels(path):
    return s.shared.probe.read_native(path)['document']['channels']


def saved_coverage(call, folder, name):
    identifier = call('create_snapshot', name=name)['snapshot_id']
    path = folder/(name+'.drsnapshot')
    call('save_snapshot', snapshot_id=identifier, path=str(path))
    manifest = json.loads((path/'state.json').read_text())
    value = manifest['selection']
    assert value['kind'] == 'mask16', value
    data = (path/value['maskFile']).read_bytes()
    return struct.unpack('<'+'H'*(len(data)//2), data)


def run_depth(folder, executable, baseline, case):
    folder.mkdir(parents=True)
    depth = case['depth']
    source = baseline/str(depth)/'v2.drsnapshot'
    expected_raw = s.raw_file(baseline/str(depth)/'legacy-native.rz')
    points = [dict(x=x, y=y) for y in range(s.HEIGHT) for x in range(s.WIDTH)]
    outcomes = dict(depth=depth, refusals=[])
    with s.probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        call = s.probe.opened(app, source/'image.rz', depth, folder/'calls.jsonl')
        restore(call, source)
        actual = call('get_selection', points=points)
        assert len(actual['samples']) == len(case['selection']['samples'])
        assert actual['samples'] == case['selection']['samples'] or all(
            {key: value for key, value in a.items() if key in ('x', 'y', 'coverage')} == b
            for a, b in zip(actual['samples'], case['selection']['samples']))
        assert s.raw(call, folder, 'v2-restored') == expected_raw
        outcomes['legacy_v2'] = True

        if depth == 8:
            def v1(manifest, _):
                manifest['version'] = 1
                for key in ('storage', 'evaluation', 'exportBitDepth', 'exportDither'):
                    manifest.pop(key, None)
            restore(call, package(source, folder/'legacy-v1.drsnapshot', v1))
            assert s.raw(call, folder, 'v1-restored') == expected_raw
            outcomes['legacy_v1'] = True
            for index, edit in enumerate(case['scalar_outputs']):
                restore(call, source)
                call(edit['tool'], **edit['arguments'])
                assert s.raw(call, folder, f'byte-edit-{index}') == s.raw_file(Path(edit['native']))
                assert stored_channels(folder/f'byte-edit-{index}.rz') == stored_channels(Path(edit['native']))
            outcomes['legacy_scalar_outputs'] = len(case['scalar_outputs'])

        restore(call, source)
        for name, change in [
            ('future-version', lambda m, _: m.update(version=999)),
            ('old-invalid-evaluation', lambda m, _: m.update(evaluation='unknown')),
            ('old-invalid-export', lambda m, _: m.update(exportBitDepth=32)),
            ('old-native-kind', lambda m, _: m['selection'].update(kind='mask16')),
            ('unsafe-mask-name', lambda m, _: m['selection'].update(maskFile='../selection.mask')),
            ('short-mask', lambda _, p: (p/'selection.mask').write_bytes(b'\x01')),
        ]:
            path = package(source, folder/(name+'.drsnapshot'), change)
            history = s.probe.history(call)
            before_selection = call('get_selection', points=points)
            error = call('load_snapshot', path=str(path), fail=True)
            assert s.probe.history(call) == history
            assert call('get_selection', points=points) == before_selection
            assert s.raw(call, folder, name+'-unchanged') == expected_raw
            outcomes['refusals'].append(dict(name=name, error=error))

        if depth == 16:
            codes = [0, 1, 2, 127, 128, 255, 256, 257, 258, 1001, 32767, 32768, 65534, 65535]
            coverage = tuple(codes[(x+3*y) % len(codes)] for y in range(s.HEIGHT) for x in range(s.WIDTH))
            previous = tuple(reversed(coverage))
            def native(manifest, path):
                manifest['version'] = 3
                manifest['target'] = 'channel'
                manifest['targetChannel'] = 0
                manifest['channelView'] = dict(rgb=False, plane=None, mask=False, channels=[0])
                for field, values in [('selection', coverage), ('previousSelection', previous)]:
                    value = manifest[field]
                    value['kind'] = 'mask16'
                    (path/value['maskFile']).write_bytes(struct.pack('<'+'H'*len(values), *values))
            native_path = package(source, folder/'native-input.drsnapshot', native)
            restore(call, native_path)
            assert saved_coverage(call, folder, 'native-roundtrip') == coverage
            saved = json.loads((folder/'native-roundtrip.drsnapshot/state.json').read_text())
            assert saved['target'] == 'channel' and saved['targetChannel'] == 0
            assert saved['channelView'] == dict(rgb=False, mask=False, channels=[0])
            assert tuple(p['coverage16'] for p in call('get_selection', points=points)['samples']) == coverage
            call('deselect')
            call('reselect')
            assert saved_coverage(call, folder, 'native-reselect') == coverage
            call('undo')
            call('undo')
            assert saved_coverage(call, folder, 'native-undo') == coverage
            call('set_quick_mask', active=True)
            runs = struct.pack('<IBB', s.WIDTH*s.HEIGHT, 0, 64)
            call('quick_mask_stroke', width=s.WIDTH, height=s.HEIGHT,
                 coverage_rle=base64.b64encode(runs).decode())
            expected = tuple((value*191+127)//255 for value in coverage)
            assert saved_coverage(call, folder, 'legacy-quickmask-native') == expected
            call('undo')
            assert saved_coverage(call, folder, 'quickmask-undo') == coverage
            outcomes['native_coverage_archive_history_and_legacy_stroke'] = True
            for name, change in [
                ('native-odd-length', lambda _, p: (p/'selection.mask').write_bytes(b'\x01'*767)),
                ('native-overlong', lambda _, p: (p/'selection.mask').write_bytes(b'\x01'*770)),
                ('native-empty', lambda _, p: (p/'selection.mask').write_bytes(b'\x00'*768)),
                ('duplicate-view-channel', lambda m, _: m['channelView'].update(channels=[0,0])),
                ('invalid-view-channel', lambda m, _: m['channelView'].update(channels=[99])),
                ('missing-view-mask', lambda m, _: m['channelView'].update(mask=True)),
                ('unknown-view-plane', lambda m, _: m['channelView'].update(plane='unknown')),
                ('invalid-target-channel', lambda m, _: m.update(targetChannel=99)),
            ]:
                path = package(native_path, folder/(name+'.drsnapshot'), change)
                history = s.probe.history(call)
                error = call('load_snapshot', path=str(path), fail=True)
                assert s.probe.history(call) == history
                outcomes['refusals'].append(dict(name=name, error=error))
    return outcomes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_executable())
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    args = parser.parse_args()
    baseline = json.loads((args.baseline/'report.json').read_text())
    assert baseline['passed'] and baseline['app_sha256'] == BASELINE_SHA
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9g-compat-'))
    before = dependencies()
    report = dict(passed=False, scratch=str(folder), app_sha256=s.digest(args.app), baseline_sha256=BASELINE_SHA)
    try:
        report['cases'] = [run_depth(folder/str(c['depth']), args.app.resolve(), args.baseline, c) for c in baseline['cases']]
        after = dependencies()
        changes = sorted(k for k in before.keys()|after.keys() if before.get(k) != after.get(k))
        report['source_check'] = dict(files=len(before), changes=changes, file_set_exact=before.keys()==after.keys())
        if args.require_current:
            assert not changes, changes
        assert s.digest(args.app) == report['app_sha256']
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['all_owned_apps_exited'] = all(a.process is None or a.process.poll() is not None for a in s.shared.HostApp.owners)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True)+'\n')
        print(args.report, flush=True)


if __name__ == '__main__':
    main()
