#!/usr/bin/env python3
"""Phase 8C host and genuine-camera external MCP qualification.

The camera fixture is CC0 and downloaded separately; its manifest records URL,
size, camera sensor depth and SHA256. No fixture rescaling is permitted. Every
app/probe/settings/recovery owner is private, with dummy credentials. Core Image
requires macOS graphics-service access; sandbox all-zero rendering must fail.
"""

import argparse
from array import array
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
HERE = ROOT / 'scripts/raw-probe'
sys.path.insert(0, str(HERE))
from native import digest, fallback_pngs, incompatible_decoder, png_samples, read_native, tiff_samples
from phase3_fixtures import synthetic_dng
from phase5_support import App, write_report
from verify_phase1 import APP
from verify_phase8b import source_call
from verification_isolation import preset_environment
sys.path.insert(0, str(ROOT / 'scripts/precision-probe'))
from retained_fixtures import encode_png
# The shared cached probe build, loaded by path so `scripts/` never shadows a probe's own modules.
_build_spec = importlib.util.spec_from_file_location('probe_build', ROOT / 'scripts/probe_build.py')
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)


def file_hash(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def run_logged(command, destination, environment, timeout=900):
    print('Running:', Path(str(command[0])).name, '→', destination.name, flush=True)
    with destination.open('w') as log:
        child = subprocess.Popen(list(map(str, command)), cwd=ROOT, env=environment,
                                 stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            status = child.wait(timeout=timeout)
        except BaseException:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            raise
        if status:
            raise RuntimeError(f"{Path(str(command[0])).name} exited {status}; inspect {destination}")
    if 'build' not in destination.name:
        print(destination.read_text(), end='', flush=True)


def host(scratch, library):
    files = scratch / 'host-files'
    files.mkdir()
    synthetic_dng(files / 'synthetic.dng', width=512, height=384)
    routing_pixels = [(1 + i * 1031, 256 + i * 619, 65000 - i * 1801, 65535) for i in range(12)]
    (files / 'ordinary16.png').write_bytes(encode_png(4, 3, 16, routing_pixels,
        Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc').read_bytes()))
    private_library = scratch / 'libdoctored_core.a'
    before = file_hash(library)
    shutil.copy2(library, private_library)
    assert before == file_hash(library) == file_hash(private_library)
    info = plistlib.loads((ROOT / 'app/Info.plist').read_bytes())
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.raw-probe'
    (scratch / 'Info.plist').write_bytes(plistlib.dumps(info))
    environment = dict(os.environ, ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'),
        DR_ACTIONS_DIR=str(scratch / 'actions'), DR_RECOVERY_DIR=str(scratch / 'recovery'),
        DR_RAW_PROBE_DIR=str(files),
        CLANG_MODULE_CACHE_PATH=str(scratch / 'clang-cache'), **preset_environment(scratch))
    sources = probe_build.app_sources()
    probes = probe_build.probe_sources('raw-probe')
    measured = [*sources, HERE / 'main.swift', HERE / 'native.py', Path(__file__).resolve(),
                ROOT / 'core/include/doctored_core.h', ROOT / 'app/Bridging/Doctored-Bridging-Header.h']
    provenance = {str(path.relative_to(ROOT)): file_hash(path) for path in measured}
    write_report(scratch / 'host-source-provenance.json', provenance)
    snapshot = scratch / 'source'
    for path in measured:
        copied = snapshot / path.relative_to(ROOT)
        copied.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, copied)
        assert file_hash(copied) == provenance[str(path.relative_to(ROOT))] == file_hash(path)
    binary = scratch / 'host-probe'
    probe_build.build(
        app_sources=[snapshot / p.relative_to(ROOT) for p in sources],
        sources=[snapshot / p.relative_to(ROOT) for p in probes],
        bridging_header=snapshot / 'app/Bridging/Doctored-Bridging-Header.h',
        library=scratch / 'libdoctored_core.a', info_plist=scratch / 'Info.plist',
        output=binary, log=scratch / 'host-build.log', env=environment, timeout=900)
    run_logged(['/usr/bin/time', '-l', binary, '-ApplePersistenceIgnoreState', 'YES',
        '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
        scratch / 'host-run.log', environment, 600)
    current = {str(path.relative_to(ROOT)): file_hash(path) for path in measured}
    assert current == provenance, 'Host sources changed during qualification'
    report = json.loads((files / 'host-report.json').read_text())
    assert report['passed'] and {case['depth'] for case in report['cases']} == {8, 16}
    for case in report['cases']:
        for name in ['native', 'package']:
            parsed = read_native(case[name], pixels=False)
            assert len(parsed['payloads']) == 1
            assert parsed['payloads'][0] == (files / 'synthetic.dng').read_bytes()
    return {**report, 'source_hashes_match': True, 'source_count': len(provenance),
            'library_sha256': before, 'binary_sha256': file_hash(binary)}


def compact_source(info):
    return {key: info[key] for key in ['source_id', 'revision_id', 'depth', 'fallback_depth', 'raw']}


def camera_scenario(folder, fixture, depth, references):
    folder.mkdir()
    original = folder / 'camera.NEF'
    shutil.copy2(fixture, original)
    initial_path, developed_path = folder / 'initial.rz', folder / 'developed.rz'
    package, output = folder / 'portable.rz', folder / 'developed.png'
    groups, pids = [], []
    with App(folder / 'before', folder / 'recovery-before', interval=600) as app:
        pids.append(app.process.pid)
        catalog = {tool['name'] for tool in app.client.rpc('tools/list', {})['tools']}
        assert {'get_raw_source', 'redevelop_raw'} <= catalog
        opened = app.call('open_document', path=str(original), precision='rgba8' if depth == 8 else 'preserve')
        document = opened['document']['id']
        call = lambda tool, **arguments: source_call(app, document, tool, **arguments)
        state = call('get_document')
        assert (state['width'], state['height']) == (3008, 2000)
        first = call('get_raw_source', layer=0)['source']
        assert first['depth'] == 16 and first['fallback_depth'] == depth
        assert first['raw']['content_sha256'] == file_hash(fixture)
        assert first['decoder_status'] == 'captured_implementation_available'
        call('set_active_layer', index=0)
        if depth == 16:
            call('add_shape_layer', kind='rect', x=100, y=100, w=80, h=60, fill='#00000000')
            call('clone_stamp', layer=1, source_layer=0, source_x=500, source_y=400,
                 points=[[130, 130], [145, 138]], size=9, sample_all_layers=False)
        else:
            call('new_layer', name='Camera retouch')
            call('brush_stroke', layer=1, points=[[130, 130], [145, 138]], size=9, color='#4379ae', opacity=0.63)
        retouch_info = call('get_document')['layers'][1]
        retouch_offset = retouch_info['offset'] if depth == 16 else [retouch_info['offset_x'], retouch_info['offset_y']]
        call('save_copy', path=str(initial_path))
        initial_native = read_native(initial_path, pixels=False)
        initial_fallbacks = fallback_pngs(initial_path)
        assert len(initial_fallbacks) == 2
        retouch_hash = digest(initial_fallbacks[1])
        call('redevelop_raw', layer=0, raw={'temperature': 7000, 'tint': 17, 'exposure': 0.35})
        developed = call('get_raw_source', layer=0)['source']
        assert developed['source_id'] == first['source_id'] and developed['revision_id'] != first['revision_id']
        assert developed['raw']['content_sha256'] == first['raw']['content_sha256']
        call('undo')
        assert compact_source(call('get_raw_source', layer=0)['source']) == compact_source(first)
        call('redo')
        assert compact_source(call('get_raw_source', layer=0)['source']) == compact_source(developed)
        call('save_copy', path=str(developed_path))
        call('save_copy', path=str(package))
        call('save_copy', path=str(output))
        if depth == 16:
            call('save_copy', path=str(folder / 'developed.tiff'))
        groups += ['genuine_camera_native_open', 'localized_retouch_above_raw', 'white_balance_exposure_redevelopment',
                   'source_identity_revision_and_one_step_undo', 'native_and_flat_output']
    original.unlink()
    unavailable = folder / 'unavailable-decoder.rz'
    incompatible_decoder(developed_path, unavailable)
    with App(folder / 'after', folder / 'recovery-after', interval=600) as app:
        pids.append(app.process.pid)
        document = app.open(unavailable)
        call = lambda tool, **arguments: source_call(app, document, tool, **arguments)
        missing = call('get_raw_source', layer=0)['source']
        assert missing['decoder_status'] == 'unavailable' and missing['saved_rendering_available']
        call('save_copy', path=str(folder / 'unavailable.png'))
        failure = call('redevelop_raw', layer=0, raw={'exposure': 0.6}, fail=True)
        assert 'decoder' in failure.lower() and 'unavailable' in failure.lower()
        call('save_copy', path=str(folder / 'after-refusal.png'))
        assert file_hash(output) == file_hash(folder / 'unavailable.png') == file_hash(folder / 'after-refusal.png')
        # The original camera path has gone. Native/package rendering and explicit
        # redevelopment must use only the payload inside these documents.
        native_document = app.open(package)
        packed = lambda tool, **arguments: source_call(app, native_document, tool, **arguments)
        assert compact_source(packed('get_raw_source', layer=0)['source']) == compact_source(developed)
        packed('save_copy', path=str(folder / 'reopened.png'))
        assert file_hash(output) == file_hash(folder / 'reopened.png')
        packed('redevelop_raw', layer=0, raw={'exposure': 0.6})
        packed('save_copy', path=str(folder / 'reopened-redeveloped.rz'))
        packed('redevelop_raw', layer=0, raw={'exposure': 0.35})
        packed('save_copy', path=str(folder / 'reopened-roundtrip.png'))
        assert file_hash(output) == file_hash(folder / 'reopened-roundtrip.png'), 'Exposure roundtrip must preserve every exported code'
        before_unchanged = compact_source(packed('get_raw_source', layer=0)['source'])
        packed('redevelop_raw', layer=0, raw={'temperature': 7000, 'tint': 17, 'exposure': 0.35})
        assert compact_source(packed('get_raw_source', layer=0)['source']) == before_unchanged, 'Repeated identical development request must be a no-op'
        groups += ['fresh_process_original_deleted_portability', 'unavailable_decoder_preserves_saved_rendering',
                   'unavailable_decoder_refuses_redevelopment', 'embedded_original_redevelops_after_reopen', 'exact_exposure_roundtrip_and_repeated_recipe_noop']
    assert digest(fallback_pngs(developed_path)[1]) == retouch_hash
    assert digest(fallback_pngs(folder / 'reopened-redeveloped.rz')[1]) == retouch_hash
    checks = []
    for variant, path in [('initial', initial_path), ('redeveloped', developed_path)]:
        native = read_native(path)
        assert len(native['payloads']) == 1 and digest(native['payloads'][0]) == file_hash(fixture)
        master = next(asset for asset in native['assets'] if asset['descriptor'] is not None)
        assert (master['width'], master['height'], master['depth']) == (3008, 2000, 16)
        expected = array('H')
        expected.frombytes((references / f'{variant}-rgba16le.bin').read_bytes())
        actual = master['samples']
        assert len(actual) == len(expected)
        maximum = max(abs(a - b) for a, b in zip(actual, expected))
        assert maximum <= 3, (variant, depth, 'independent direct-CI maximum code error', maximum)
        assert all(actual[i] == 65535 for i in range(3, len(actual), 4))
        nonbyte = sum(value % 257 != 0 for index, value in enumerate(actual) if index % 4 != 3)
        assert nonbyte > 3008 * 2000
        checks.append({'variant': variant, 'all_master_samples': len(actual), 'nonbyte_rgb': nonbyte,
                       'independent_reference_maximum_code_error': maximum,
                       'original_sha256': digest(native['payloads'][0]), 'retouch_unchanged': True,
                       'master_profile_sha256': master['profile_sha256']})
    # Independent source-over oracle checks every exported pixel, including
    # the retouch footprint and the unchanged underlying RAW elsewhere.
    export = png_samples(output.read_bytes())
    retouch = png_samples(fallback_pngs(developed_path)[1])
    native = read_native(developed_path)
    master = next(asset for asset in native['assets'] if asset['descriptor'] is not None)
    expected = master['samples']
    clone_checks = 0
    if depth == 16:
        initial_master = next(asset for asset in read_native(initial_path)['assets'] if asset['descriptor'] is not None)['samples']
        ox, oy = retouch_offset
        for y in range(retouch['height']):
            for x in range(retouch['width']):
                i = (y * retouch['width'] + x) * 4
                if retouch['samples'][i + 3] == 65535:
                    j = ((y + oy + 270) * 3008 + x + ox + 370) * 4
                    assert retouch['samples'][i:i + 3] == initial_master[j:j + 3], ('clone source codes', x, y)
                    clone_checks += 1
        assert clone_checks > 30
        assert sum(v % 257 != 0 for i, v in enumerate(retouch['samples']) if i % 4 != 3) > 90
        del initial_master
    maximum = (1 << depth) - 1
    worst = 0
    ox, oy = retouch_offset
    assert (export['width'], export['height'], export['depth']) == (3008, 2000, depth)
    for y in range(2000):
        for x in range(3008):
            i = (y * 3008 + x) * 4
            actual = export['samples'][i:i + 4]
            base = [value if depth == 16 else (value + 128) // 257 for value in expected[i:i + 3]]
            if ox <= x < ox + retouch['width'] and oy <= y < oy + retouch['height']:
                j = ((y - oy) * retouch['width'] + x - ox) * 4
                over = retouch['samples'][j:j + 4]
                alpha = over[3] / maximum
                wanted = [round(under * (1 - alpha) + foreground * alpha) for under, foreground in zip(base, over[:3])]
            else:
                wanted = base
            worst = max(worst, *(abs(a - b) for a, b in zip(actual[:3], wanted)))
            assert actual[3] == maximum
    assert worst <= 2, ('independent exported composite', depth, worst)
    assert digest(export['profile']) == master['profile_sha256']
    if depth == 16:
        tiff = tiff_samples(folder / 'developed.tiff')
        assert tiff['samples'] == export['samples'] and tiff['profile'] == export['profile']
    groups.append('independent_all_pixel_composite_png_and_tiff')
    result = {'passed': True, 'fallback_depth': depth, 'native_width': 3008, 'native_height': 2000,
            'resized': False, 'groups': groups, 'catalog_count': len(catalog), 'process_ids': pids,
            'all_owned_processes_exited': True, 'native_checks': checks,
            'composite_maximum_error_codes': worst, 'composite_pixel_count': 3008 * 2000,
            'exact_genuine_u16_clone_pixels': clone_checks}
    write_report(folder / 'case-report.json', result)
    print(f'PASS: genuine camera {depth}-bit fallback, {len(groups)} external/independent groups', flush=True)
    return result


def camera(scratch, fixture):
    manifest = json.loads((HERE / 'camera-fixture.json').read_text())
    assert fixture.stat().st_size == manifest['encoded_bytes'] and file_hash(fixture) == manifest['sha256']
    environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(scratch / 'reference-cache'))
    binary = scratch / 'reference-probe'
    references = scratch / 'reference-files'
    run_logged(['swiftc', '-O', HERE / 'reference.swift', '-o', binary], scratch / 'reference-build.log', environment)
    run_logged([binary, fixture, references], scratch / 'reference-run.log', environment, 120)
    return {'fixture': manifest, 'reference': json.loads((references / 'reference-report.json').read_text()),
            'cases': [camera_scenario(scratch / f'camera-{depth}', fixture, depth, references) for depth in (8, 16)]}


def dependencies():
    paths = [*(ROOT / 'app/Sources').glob('*.swift'), *(ROOT / 'core/src').rglob('*.rs'),
             *(ROOT / 'core/include').glob('*.h'), *(ROOT / 'app/Bridging').glob('*.h'),
             *HERE.glob('*.swift'), *HERE.glob('*.json'), *HERE.glob('*.py'),
             Path(__file__).resolve(), ROOT / 'scripts/phase3_fixtures.py',
             ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/verification_isolation.py',
             ROOT / 'scripts/app_under_test.py',
             ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py',
             ROOT / 'scripts/verify_phase8b.py', ROOT / 'scripts/precision-probe/retained_fixtures.py',
             ROOT / 'scripts/precision-probe/fixtures.py',
             ROOT / 'scripts/precision-probe/retained_linked_fixtures.py', ROOT / 'scripts/probe_build.py']
    return {str(path.relative_to(ROOT)): file_hash(path) for path in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--camera', type=Path, help='Downloaded exact CC0 Nikon D70s NEF from camera-fixture.json.')
    parser.add_argument('--host-only', action='store_true')
    parser.add_argument('--mcp-only', action='store_true')
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    parser.add_argument('--keep', action='store_true')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    if args.host_only and args.mcp_only:
        parser.error('Choose one focused mode.')
    if not args.host_only and args.camera is None:
        parser.error('--camera is required for genuine-camera qualification.')
    scratch = Path(tempfile.mkdtemp(prefix='rasterize-phase8c-'))
    print('Verification scratch:', scratch, flush=True)
    provenance = dependencies()
    write_report(scratch / 'source-provenance.json', provenance)
    binary_hash = file_hash(APP) if not args.host_only else None
    report = {'passed': False, 'scratch': str(scratch)}
    try:
        if not args.mcp_only:
            report['host'] = host(scratch, args.library)
        if not args.host_only:
            report['camera'] = camera(scratch, args.camera)
        current = dependencies()
        mismatches = sorted(key for key in provenance.keys() | current.keys() if provenance.get(key) != current.get(key))
        report['source_check'] = {'files': len(provenance), 'mismatches': mismatches, 'file_set_exact': provenance.keys() == current.keys()}
        assert not mismatches, ('Sources changed during qualification', mismatches)
        if binary_hash is not None:
            assert binary_hash == file_hash(APP), 'Built app changed during qualification'
            report['app_sha256'] = binary_hash
        report['passed'] = True
    except BaseException as failure:
        report['error'] = repr(failure)
        print('Failure artifacts retained:', scratch, flush=True)
        raise
    finally:
        write_report(scratch / 'report.json', report)
        if args.report:
            write_report(args.report, report)
    if not args.keep:
        shutil.rmtree(scratch)


if __name__ == '__main__':
    main()
