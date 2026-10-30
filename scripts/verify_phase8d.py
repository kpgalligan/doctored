#!/usr/bin/env python3
"""Isolated Phase 8D host/UI and built-app preset/synchronization qualification.

Use --host-only or --mcp-only for focused work. Camera colour qualification is
separate in verify_phase8d_color.py. This runner uses small independently framed
DNGs, private preference libraries/Actions/recovery, dummy credentials and owned processes.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import struct
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
HERE = ROOT / 'scripts/raw-workflow-probe'
from phase3_fixtures import synthetic_dng
from phase5_support import App, write_report
from verify_phase1 import APP
from verify_phase8b import source_call

_host_spec = importlib.util.spec_from_file_location('phase8d_host_support', ROOT / 'scripts/host-source-probe/support.py')
_host_support = importlib.util.module_from_spec(_host_spec)
_host_spec.loader.exec_module(_host_support)
PRESET_DIRECTORIES = _host_support.HostApp.PRESET_DIRECTORIES
# The shared cached probe build, loaded by path.
_build_spec = importlib.util.spec_from_file_location('probe_build', ROOT / 'scripts/probe_build.py')
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)


def private_libraries(folder):
    return {name: str(folder / sub) for name, sub in PRESET_DIRECTORIES.items()}


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fixtures(folder):
    folder.mkdir(parents=True, exist_ok=True)
    synthetic_dng(folder / 'source.dng')
    for name in ['second.dng', 'inspect.dng', 'ui.dng']:
        shutil.copy2(folder / 'source.dng', folder / name)
    data = bytearray((folder / 'source.dng').read_bytes())
    count = struct.unpack_from('<H', data, 8)[0]
    for index in range(count):
        at = 10 + index * 12
        if struct.unpack_from('<H', data, at)[0] == 50728:
            offset = struct.unpack_from('<I', data, at + 8)[0]
            struct.pack_into('<6I', data, offset, 3, 2, 1, 1, 3, 4)
            break
    else:
        raise AssertionError('Fixture has no AsShotNeutral tag')
    (folder / 'target.dng').write_bytes(data)


def dependencies():
    files = [*(ROOT / 'app/Sources').glob('*.swift'), *(ROOT / 'core/src').rglob('*.rs'),
             *(ROOT / 'core/include').glob('*.h'), *(ROOT / 'app/Bridging').glob('*.h'),
             *HERE.glob('*.swift'), Path(__file__).resolve(), ROOT / 'scripts/phase3_fixtures.py',
             ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/verification_isolation.py',
             ROOT / 'scripts/app_under_test.py',
             ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
             ROOT / 'scripts/raw-probe/native.py', ROOT / 'scripts/host-source-probe/support.py',
             ROOT / 'scripts/precision-probe/fixtures.py', ROOT / 'scripts/precision-probe/retained_fixtures.py',
             ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
             ROOT / 'core/Cargo.toml', ROOT / 'core/Cargo.lock',
             ROOT / 'project.yml', ROOT / 'Makefile', ROOT / 'scripts/probe_build.py']
    return {str(path.relative_to(ROOT)): digest(path) for path in files}


def logged(command, path, environment, timeout=900):
    print('Running', Path(str(command[0])).name, '→', path.name, flush=True)
    with path.open('w') as output:
        process = subprocess.Popen(list(map(str, command)), cwd=ROOT, env=environment,
            stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            raise
    if code:
        raise RuntimeError(f'{command[0]} exited {code}; see {path}')
    if 'build' not in path.name:
        print(path.read_text(), end='', flush=True)


def host(folder, library):
    files = folder / 'files'
    fixtures(files)
    private = folder / 'libdoctored_core.a'
    fingerprint = digest(library)
    shutil.copy2(library, private)
    assert fingerprint == digest(library) == digest(private)
    sources = probe_build.app_sources()
    probes = probe_build.probe_sources('raw-workflow-probe')
    headers = [ROOT / 'core/include/doctored_core.h', ROOT / 'app/Bridging/Doctored-Bridging-Header.h']
    measured = {str(path.relative_to(ROOT)): digest(path) for path in [*sources, *probes, *headers]}
    write_report(folder / 'host-source-provenance.json', measured)
    snapshot = folder / 'source'
    for path in [*sources, *probes, *headers]:
        target = snapshot / path.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        assert digest(target) == measured[str(path.relative_to(ROOT))] == digest(path)
    info = plistlib.loads((ROOT / 'app/Info.plist').read_bytes())
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.raw-workflow-probe'
    (folder / 'Info.plist').write_bytes(plistlib.dumps(info))
    env = dict(os.environ, ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'), **private_libraries(folder),
        DR_ACTIONS_DIR=str(folder / 'actions'), DR_RECOVERY_DIR=str(folder / 'recovery'),
        DR_RAW_WORKFLOW_PROBE_DIR=str(files), CLANG_MODULE_CACHE_PATH=str(folder / 'clang-cache'))
    binary = folder / 'host-probe'
    probe_build.build(
        sources=[snapshot / p.relative_to(ROOT) for p in sources + probes],
        bridging_header=snapshot / 'app/Bridging/Doctored-Bridging-Header.h',
        library=folder / 'libdoctored_core.a', info_plist=folder / 'Info.plist',
        output=binary, log=folder / 'host-build.log', env=env, timeout=900)
    logged(['/usr/bin/time', '-l', binary, '-ApplePersistenceIgnoreState', 'YES',
        '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
        folder / 'host-run.log', env, 600)
    current = {str(path.relative_to(ROOT)): digest(path) for path in [*sources, *probes, *headers]}
    assert measured == current, 'Host sources changed during qualification'
    report = json.loads((files / 'workflow-report.json').read_text())
    assert report['passed']
    return {**report, 'library_sha256': fingerprint, 'binary_sha256': digest(binary),
            'source_count': len(measured), 'source_hashes_exact': True}


def reuse_host(folder, previous, library):
    """Rerun exact compiled inputs after script-only qualification changes."""
    measured = json.loads((previous / 'host-source-provenance.json').read_text())
    sources = [path for path in sorted((ROOT / 'app/Sources').glob('*.swift')) if path.name != 'main.swift']
    expected = {str(path.relative_to(ROOT)) for path in [*sources, HERE / 'main.swift', HERE / 'develop_ui.swift',
        ROOT / 'core/include/doctored_core.h', ROOT / 'app/Bridging/Doctored-Bridging-Header.h']}
    assert set(measured) == expected, 'Compiled host source file set changed'
    assert all(digest(ROOT / name) == value for name, value in measured.items()), 'Compiled host sources changed'
    assert digest(previous / 'libdoctored_core.a') == digest(library), 'Compiled host archive differs from final archive'
    binary = previous / 'host-probe'
    files = folder / 'files'
    fixtures(files)
    env = dict(os.environ, ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'), **private_libraries(folder),
        DR_ACTIONS_DIR=str(folder / 'actions'), DR_RECOVERY_DIR=str(folder / 'recovery'), DR_RAW_WORKFLOW_PROBE_DIR=str(files))
    logged(['/usr/bin/time', '-l', binary, '-ApplePersistenceIgnoreState', 'YES',
        '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
        folder / 'host-run.log', env, 600)
    assert all(digest(ROOT / name) == value for name, value in measured.items()), 'Compiled host inputs changed during run'
    write_report(folder / 'host-source-provenance.json', measured)
    report = json.loads((files / 'workflow-report.json').read_text())
    assert report['passed']
    return {**report, 'library_sha256': digest(library), 'binary_sha256': digest(binary),
            'compiled_artifacts': str(previous), 'source_count': len(measured), 'source_hashes_exact': True}


class RawApp(App):
    def __init__(self, folder, recovery, presets):
        super().__init__(folder, recovery, interval=600)
        self.presets = presets

    def __enter__(self):
        previous = {name: os.environ.get(name) for name in PRESET_DIRECTORIES}
        os.environ.update(private_libraries(self.folder))
        os.environ['DR_RAW_PRESETS_DIR'] = str(self.presets)
        try:
            return super().__enter__()
        finally:
            for name, value in previous.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value


def mcp(folder):
    import base64
    import time

    spec = importlib.util.spec_from_file_location('phase8d_native', ROOT / 'scripts/raw-probe/native.py')
    native_reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native_reader)
    files = folder / 'mcp-files'
    fixtures(files)
    presets = folder / 'mcp-presets'
    groups, processes, owned, descriptor_mismatches = [], [], [], []

    def exact_descriptor(actual, expected, label):
        if actual != expected:
            descriptor_mismatches.append({'case': label, 'actual': actual, 'expected': expected})

    def inspect(app, document, tool='inspect_raw_development', **arguments):
        deadline = time.monotonic() + 15
        while True:
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': {
                'document_id': document, 'layer': 0, **arguments}})
            message = next(block['text'] for block in result['content'] if block['type'] == 'text')
            if result.get('isError') and 'Another background operation is running.' in message and time.monotonic() < deadline:
                time.sleep(0.05)
                continue
            assert not result.get('isError'), message
            images = [base64.b64decode(block['data']) for block in result['content'] if block['type'] == 'image']
            assert len(images) == 2
            return json.loads(message), images

    region = {'x': 3, 'y': 5, 'width': 16, 'height': 12}
    with RawApp(folder / 'app-before', folder / 'recovery-before', presets) as app:
        processes.append(app.process.pid)
        owned.append(app.process)
        catalog = {tool['name'] for tool in app.client.rpc('tools/list', {})['tools']}
        expected = {'list_raw_presets', 'save_raw_preset', 'delete_raw_preset', 'apply_raw_preset',
                    'apply_raw_settings', 'sync_raw_settings', 'inspect_raw_development',
                    'preview_raw_decoder_migration', 'migrate_raw_decoder', 'repair_raw_preset'}
        assert expected <= catalog, (len(catalog), expected - catalog)
        source = app.call('open_document', path=str(files / 'source.dng'), raw={'exposure': 0.5, 'output_space': 'display_p3'})['document']['id']
        target = app.call('open_document', path=str(files / 'target.dng'), precision='rgba8')['document']['id']
        src = lambda tool, **args: source_call(app, source, tool, **args)
        dst = lambda tool, **args: source_call(app, target, tool, **args)
        source_info = src('get_raw_source', layer=0)['source']
        assert 'display-p3' in source_info['raw']['recipe']['outputSpace']
        initial = dst('get_raw_source', layer=0)['source']
        app.call('save_raw_preset', name='Stable exposure', raw={'exposure': 0.5})
        app.call('start_recording')
        result = dst('apply_raw_preset', name='Stable exposure', layer=0)
        assert result['result']['changed_documents'] == 1
        action = app.call('stop_recording', name='Resolved preset')
        assert len(action['steps']) == 1 and action['steps'][0]['tool'] == 'apply_raw_settings'
        changed = dst('get_raw_source', layer=0)['source']
        assert changed['revision_id'] != initial['revision_id']
        repeat = dst('apply_raw_preset', name='Stable exposure', layer=0)
        assert repeat['result']['changed_documents'] == 0
        assert dst('get_raw_source', layer=0)['source']['revision_id'] == changed['revision_id']
        app.call('save_raw_preset', name='Stable exposure', raw={'exposure': 2}, replace=True)
        dst('undo')
        dst('run_action', name='Resolved preset')
        assert dst('get_raw_source', layer=0)['source']['raw']['recipe']['values']['exposure'] == 0.5
        app.call('delete_raw_preset', name='Stable exposure')
        dst('undo')
        dst('run_action', name='Resolved preset')
        assert dst('get_raw_source', layer=0)['source']['raw']['recipe']['values']['exposure'] == 0.5
        groups += ['catalog_and_p3_import', 'persisted_sparse_preset', 'one_undo_and_noop', 'resolved_actions_survive_library_changes']
        src('redevelop_raw', layer=0, raw={'exposure': 0.75, 'temperature': 7100, 'tint': 12})
        src('sync_raw_settings', fields=['exposure', 'white_balance', 'output_space'], targets=[{'document_id': target, 'layer': 0}])
        synchronized = dst('get_raw_source', layer=0)['source']
        assert synchronized['raw']['recipe']['values']['exposure'] == 0.75
        assert synchronized['raw']['recipe']['values']['customWhiteBalance'] is True
        assert 'display-p3' in synchronized['raw']['recipe']['outputSpace']
        for invalid in [None, True, 0.5]:
            src('sync_raw_settings', fields=['exposure'], targets=[{'document_id': target, 'layer': 0}, {'document_id': target, 'layer': invalid}], fail=True)
            assert dst('get_raw_source', layer=0)['source']['revision_id'] == synchronized['revision_id']
        native = files / 'synced.rz'
        dst('save_copy', path=str(native))
        src('save_copy', path=str(files / 'typed.rz'))
        app.call('save_raw_preset', name='Persist across restart', raw={'exposure': 1.25})
        groups += ['cross_depth_sync_and_gamut', 'strict_atomic_target_rejection', 'native_save']
        report, images = inspect(app, target, region=region, raw={'exposure': 1})
        assert len(images) == 2 and report['preview_divisor'] == 1 and report['committed'] is False
        assert report['before_histogram']['sampled_pixels'] == 512 * 384
        assert report['before_histogram']['stride'] == report['after_histogram']['stride'] == 1
        assert all(struct.unpack_from('>II', image, 16) == (16, 12) for image in images)
        assert dst('get_raw_source', layer=0)['source']['revision_id'] == synchronized['revision_id']
        groups.append('readonly_native_detail_and_full_histogram')
        for invalid in [dict(region, x=True), dict(region, y=0.5), dict(region, width=1025), dict(region, x=511)]:
            dst('inspect_raw_development', region=invalid, fail=True)
            assert dst('get_raw_source', layer=0)['source']['revision_id'] == synchronized['revision_id']
        groups.append('strict_inspection_region_refusal')
    for depth, path in [('rgba8', native), ('rgba16', files / 'typed.rz')]:
        native_reader.incompatible_decoder(path, files / f'unavailable-{depth}.rz')
        unchanged = native_reader.read_native(files / f'unavailable-{depth}.rz', pixels=False)
        original = native_reader.read_native(path, pixels=False)
        assert unchanged['payloads'] == original['payloads']
        assert unchanged['assets'][0]['png_sha256'] == original['assets'][0]['png_sha256']
    # All original locations belong to this harness; both native variants must
    # now restore the exact embedded camera payload without their import URLs.
    (files / 'source.dng').unlink()
    (files / 'target.dng').unlink()
    with RawApp(folder / 'app-after', folder / 'recovery-after', presets) as app:
        processes.append(app.process.pid)
        owned.append(app.process)
        entries = app.call('list_raw_presets')['presets']
        assert any(p['name'] == 'Persist across restart' and p['raw']['exposure'] == 1.25 for p in entries)
        reopened = app.open(files / 'synced.rz')
        result = source_call(app, reopened, 'apply_raw_preset', name='Persist across restart', layer=0)
        assert result['result']['changed_layers'] == 1
        assert source_call(app, reopened, 'get_raw_source', layer=0)['source']['raw']['recipe']['values']['exposure'] == 1.25
        groups.append('fresh_process_preset_and_native_reopen')
        for depth in ['rgba8', 'rgba16']:
            document = app.open(files / f'unavailable-{depth}.rz')
            call = lambda tool, **args: source_call(app, document, tool, **args)
            saved = call('get_raw_source', layer=0)['source']
            assert saved['decoder_status'] == 'unavailable' and saved['saved_rendering_available']
            call('redevelop_raw', layer=0, raw={'exposure': 1}, fail=True)
            before_report, before_images = inspect(app, document, region=region)
            assert before_images[0] == before_images[1]
            assert call('get_raw_source', layer=0)['source']['revision_id'] == saved['revision_id']
            call('migrate_raw_decoder', layer=0, fail=True)
            call('migrate_raw_decoder', layer=0, migration={'before': '{}', 'after': '{}'}, fail=True)
            groups.append(f'{depth}_saved_inspection_without_decoder_or_original')

            old, _ = inspect(app, document, 'preview_raw_decoder_migration', region=region)
            latest, latest_images = inspect(app, document, 'preview_raw_decoder_migration', region=region)
            assert old['preview_id'] != latest['preview_id']
            call('migrate_raw_decoder', preview_id=old['preview_id'], fail=True)
            # A selection change invalidates the captured editing state even
            # though the immutable retained source revision has not changed.
            call('select_rect', x=1, y=2, width=12, height=14)
            call('migrate_raw_decoder', preview_id=latest['preview_id'], fail=True)
            assert call('get_raw_source', layer=0)['source']['revision_id'] == saved['revision_id']
            groups.append(f'{depth}_superseded_and_stale_review_refusal')

            review, review_images = inspect(app, document, 'preview_raw_decoder_migration', region=region)
            assert review['committed'] is False and review['expires_after_seconds'] == 300
            assert review_images[0] == before_images[0]
            assert review['before_descriptor']['decoder']['implementation'] == 'unavailable-independent-probe-implementation'
            assert review['after_descriptor']['decoder']['implementation'] != review['before_descriptor']['decoder']['implementation']
            assert call('get_raw_source', layer=0)['source']['revision_id'] == saved['revision_id']
            app.call('start_recording')
            call('migrate_raw_decoder', layer=0, preview_id=review['preview_id'])
            recorded = app.call('stop_recording', name=f'Reviewed migration {depth}')
            assert len(recorded['steps']) == 1 and recorded['steps'][0]['tool'] == 'migrate_raw_decoder'
            migrated = call('get_raw_source', layer=0)['source']
            assert migrated['decoder_status'] == 'captured_implementation_available'
            assert migrated['source_id'] == saved['source_id'] and migrated['revision_id'] != saved['revision_id']
            assert migrated['raw']['content_sha256'] == saved['raw']['content_sha256']
            observed_descriptor = {key: value for key, value in migrated['raw'].items() if key != 'encoded_bytes'}
            write_report(files / f'migration-descriptors-{depth}.json', {'observed': observed_descriptor, 'reviewed': review['after_descriptor']})
            exact_descriptor(observed_descriptor, review['after_descriptor'], f'{depth}_accepted_recipe')
            accepted_report, accepted_images = inspect(app, document, region=region)
            assert accepted_images[0] == accepted_images[1] == review_images[1]
            assert accepted_report['before_histogram'] == review['after_histogram']
            call('migrate_raw_decoder', layer=0, preview_id=review['preview_id'], fail=True)
            call('undo')
            restored = call('get_raw_source', layer=0)['source']
            assert restored['raw'] == saved['raw'] and restored['revision_id'] == saved['revision_id']
            _, undo_images = inspect(app, document, region=region)
            assert undo_images[0] == before_images[0]
            call('run_action', name=f'Reviewed migration {depth}')
            assert call('get_raw_source', layer=0)['source']['raw'] == migrated['raw']
            _, replay_images = inspect(app, document, region=region)
            assert replay_images[0] == review_images[1]
            call('undo')
            assert call('get_raw_source', layer=0)['source']['raw'] == saved['raw']
            call('redo')
            assert call('get_raw_source', layer=0)['source']['raw'] == migrated['raw']
            path = files / f'migrated-{depth}.rz'
            call('save_copy', path=str(path))
            packed = native_reader.read_native(path, pixels=False)
            exact_descriptor(packed['assets'][0]['descriptor'], review['after_descriptor'], f'{depth}_native_recipe')
            assert packed['payloads'] == native_reader.read_native(files / f'unavailable-{depth}.rz', pixels=False)['payloads']
            groups.append(f'{depth}_reviewed_pixels_one_undo_action_replay_and_native_save')
    with RawApp(folder / 'app-migrated', folder / 'recovery-migrated', presets) as app:
        processes.append(app.process.pid)
        owned.append(app.process)
        for depth in ['rgba8', 'rgba16']:
            document = app.open(files / f'migrated-{depth}.rz')
            current = source_call(app, document, 'get_raw_source', layer=0)['source']
            assert current['decoder_status'] == 'captured_implementation_available'
            packed = native_reader.read_native(files / f'migrated-{depth}.rz', pixels=False)
            assert {key: value for key, value in current['raw'].items() if key != 'encoded_bytes'} == packed['assets'][0]['descriptor']
            source_call(app, document, 'preview_raw_decoder_migration', layer=0, fail=True)
            groups.append(f'{depth}_migrated_native_fresh_process_reopen')
    assert all(process.poll() is not None for process in owned)
    write_report(files / 'exact-descriptor-comparisons.json', {'mismatches': descriptor_mismatches})
    assert not descriptor_mismatches, ('Reviewed descriptor changed', [entry['case'] for entry in descriptor_mismatches])
    return {'passed': True, 'groups': groups, 'catalog_count': len(catalog), 'process_ids': processes,
            'all_owned_processes_exited': True, 'app_sha256': digest(APP)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host-only', action='store_true')
    parser.add_argument('--mcp-only', action='store_true')
    parser.add_argument('--keep', action='store_true')
    parser.add_argument('--reuse-host', type=Path, help='Prior scratch with exact current compiled Swift/header/library inputs.')
    parser.add_argument('--report', type=Path)
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    args = parser.parse_args()
    if args.host_only and args.mcp_only:
        parser.error('Choose one focused mode.')
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase8d-workflow-'))
    print('Workflow scratch:', folder, flush=True)
    before = dependencies()
    write_report(folder / 'source-provenance.json', before)
    app_before = digest(APP) if not args.host_only else None
    report = {'passed': False, 'scratch': str(folder)}
    try:
        if not args.mcp_only:
            report['host'] = reuse_host(folder, args.reuse_host, args.library) if args.reuse_host else host(folder, args.library)
        if not args.host_only:
            report['mcp'] = mcp(folder)
        after = dependencies()
        changes = sorted(path for path in before.keys() | after.keys() if before.get(path) != after.get(path))
        report['source_check'] = {'count': len(before), 'file_set_exact': before.keys() == after.keys(), 'changes': changes}
        assert not changes, ('Sources changed during qualification', changes)
        if app_before is not None:
            assert app_before == digest(APP), 'App binary changed during qualification'
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        write_report(folder / 'report.json', report)
        if args.report:
            write_report(args.report, report)
    if not args.keep:
        shutil.rmtree(folder)


if __name__ == '__main__':
    main()
