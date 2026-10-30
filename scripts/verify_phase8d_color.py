#!/usr/bin/env python3
"""Independent native-camera Display P3 qualification for Phase 8D.

Uses the CC0 Nikon fixture recorded in raw-probe/camera-fixture.json. No camera
rescaling or image libraries are used in the native/source sample comparison.
The frozen maximum difference against direct Core Image is 3 u16 codes.
"""
import argparse
from array import array
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/raw-probe'))
from native import read_native, png_samples
from phase5_support import App, write_report
from verify_phase8b import source_call
from verify_phase8c import dependencies, file_hash, run_logged
from verify_phase1 import APP


def source(path):
    native = read_native(path)
    assert len(native['payloads']) == 1
    asset = next(item for item in native['assets'] if item['descriptor'] is not None)
    assert (asset['width'], asset['height'], asset['depth']) == (3008, 2000, 16)
    return asset, native['payloads'][0]


def scenario(folder, fixture, depth, reference):
    folder.mkdir()
    original = folder / 'camera.NEF'
    shutil.copy2(fixture, original)
    files = {name: folder / f'{name}.rz' for name in ['initial', 'redeveloped', 'srgb', 'roundtrip', 'reopened']}
    pids = []
    with App(folder / 'first', folder / 'recovery-first', interval=600) as app:
        pids.append(app.process.pid)
        opened = app.call('open_document', path=str(original),
            precision='rgba8' if depth == 8 else 'preserve', raw={'output_space': 'display_p3'})
        document = opened['document']['id']
        call = lambda tool, **args: source_call(app, document, tool, **args)
        first = call('get_raw_source', layer=0)['source']
        assert first['depth'] == 16 and first['fallback_depth'] == depth
        assert first['raw']['recipe']['outputSpace'] == 'display-p3-rgba16-normalized-v1'
        assert first['raw']['recipe']['version'] == 2
        call('save_copy', path=str(files['initial']))
        call('redevelop_raw', layer=0, raw={'temperature': 7000, 'tint': 17, 'exposure': 0.35})
        call('save_copy', path=str(files['redeveloped']))
        if depth == 16:
            call('save_copy', path=str(folder / 'developed.png'))
        call('redevelop_raw', layer=0, raw={'output_space': 'srgb'})
        srgb = call('get_raw_source', layer=0)['source']
        assert srgb['raw']['recipe']['outputSpace'] == 'srgb-rgba16-normalized-v1'
        call('save_copy', path=str(files['srgb']))
        call('redevelop_raw', layer=0, raw={'output_space': 'display_p3'})
        call('save_copy', path=str(files['roundtrip']))
        stable = call('get_raw_source', layer=0)['source']
        call('redevelop_raw', layer=0, raw={'output_space': 'display_p3'})
        assert call('get_raw_source', layer=0)['source'] == stable
        call('undo')
        assert call('get_raw_source', layer=0)['source']['raw']['recipe']['outputSpace'] == 'srgb-rgba16-normalized-v1'
        call('redo')
        assert call('get_raw_source', layer=0)['source'] == stable
    original.unlink()
    with App(folder / 'second', folder / 'recovery-second', interval=600) as app:
        pids.append(app.process.pid)
        document = app.open(files['roundtrip'])
        call = lambda tool, **args: source_call(app, document, tool, **args)
        call('redevelop_raw', layer=0, raw={'exposure': 0.7})
        call('redevelop_raw', layer=0, raw={'exposure': 0.35})
        call('save_copy', path=str(files['reopened']))
    results = []
    for variant in ['initial', 'redeveloped']:
        actual, original_bytes = source(files[variant])
        assert original_bytes == fixture.read_bytes()
        expected = array('H')
        expected.frombytes((reference / f'{variant}-rgba16le.bin').read_bytes())
        assert len(actual['samples']) == len(expected)
        maximum = max(abs(a - b) for a, b in zip(actual['samples'], expected))
        assert maximum <= 3, (depth, variant, maximum)
        results.append({'variant': variant, 'components': len(expected), 'maximum_error_u16': maximum,
                        'fixed_bound_u16': 3, 'profile_sha256': actual['profile_sha256']})
    developed, _ = source(files['redeveloped'])
    srgb, _ = source(files['srgb'])
    assert srgb['profile_sha256'] != developed['profile_sha256']
    assert srgb['samples'] != developed['samples']
    for name in ['roundtrip', 'reopened']:
        repeated, encoded = source(files[name])
        assert repeated['samples'] == developed['samples'], (depth, name)
        assert repeated['profile_sha256'] == developed['profile_sha256']
        assert repeated['descriptor'] == developed['descriptor']
        assert encoded == fixture.read_bytes()
    if depth == 16:
        exported = png_samples((folder / 'developed.png').read_bytes())
        assert exported['samples'] == developed['samples']
        import hashlib
        assert hashlib.sha256(exported['profile']).hexdigest() == developed['profile_sha256']
    return {'depth': depth, 'passed': True, 'processes_exited': pids, 'comparisons': results,
        'groups': ['native_camera_p3_import', 'p3_redevelopment_matches_independent_reference',
                   'output_space_change_and_exact_roundtrip', 'output_choice_noop_undo_redo',
                   'original_deleted_fresh_process_redevelopment', 'exact_original_and_profile_persistence']
                  + (['png16_exact_source_codes_and_profile'] if depth == 16 else [])}


def provenance():
    result = dependencies()
    for path in [Path(__file__).resolve(), ROOT / 'scripts/raw-workflow-probe/reference.swift',
                 ROOT / 'core/Cargo.toml', ROOT / 'core/Cargo.lock', ROOT / 'project.yml', ROOT / 'Makefile']:
        result[str(path.relative_to(ROOT))] = file_hash(path)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--camera', type=Path, required=True)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'scripts/raw-probe/camera-fixture.json').read_text())
    assert file_hash(args.camera) == manifest['sha256']
    assert args.camera.stat().st_size == manifest['encoded_bytes']
    scratch = Path(tempfile.mkdtemp(prefix='rasterize-phase8d-color-'))
    os.environ['DR_RAW_PRESETS_DIR'] = str(scratch / 'raw-presets')
    print('Verification scratch:', scratch, flush=True)
    before = provenance()
    binary_hash = file_hash(APP)
    write_report(scratch / 'source-provenance.json', before)
    report = {'passed': False, 'scratch': str(scratch), 'fixture': manifest}
    try:
        environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(scratch / 'reference-cache'))
        binary = scratch / 'reference-probe'
        reference = scratch / 'reference-files'
        run_logged(['swiftc', '-O', ROOT / 'scripts/raw-workflow-probe/reference.swift', '-o', binary],
                   scratch / 'reference-build.log', environment)
        run_logged([binary, args.camera, reference], scratch / 'reference-run.log', environment, 180)
        report['reference'] = json.loads((reference / 'reference-report.json').read_text())
        assert all(record['pixels_outside_srgb'] > 1000 for record in report['reference']['records'])
        report['cases'] = [scenario(scratch / f'camera-{depth}', args.camera, depth, reference) for depth in [8, 16]]
        after = provenance()
        mismatches = [key for key in before.keys() | after.keys() if before.get(key) != after.get(key)]
        report['source_check'] = {'files': len(before), 'file_set_exact': before.keys() == after.keys(), 'mismatches': mismatches}
        assert not mismatches, ('Sources changed during verification', mismatches)
        assert file_hash(APP) == binary_hash
        report['app_sha256'] = binary_hash
        report['passed'] = True
        print('PASS: Phase 8D independent native-camera color qualification', flush=True)
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        write_report(scratch / 'report.json', report)
        if args.report:
            write_report(args.report, report)


if __name__ == '__main__':
    main()
