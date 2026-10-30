#!/usr/bin/env python3
"""Qualify local RAW development with independent samples and isolated owners.

Prepare fixtures/reference using scripts/raw-local-probe/README.md, then run
--mode host or --mode mcp after the corresponding implementation/build freeze.
All outputs remain in private scratch for review. --require-current is the final
qualification gate; an exploratory snapshot may report changed checkout inputs.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
PROBES = ROOT / 'scripts/raw-local-probe'
sys.path.insert(0, str(PROBES))
import app_under_test
from support import HostApp
from verify import verify_host_exports
from verification_isolation import preset_environment
# The shared cached probe build, loaded by path so `scripts/` never shadows this probe's own modules.
_build_spec = importlib.util.spec_from_file_location('probe_build', ROOT / 'scripts/probe_build.py')
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def dependencies(mode):
    paths = [*(ROOT / 'app/Sources').glob('*.swift'), *(ROOT / 'app/Bridging').glob('*.h'),
             *(ROOT / 'core/src').rglob('*.rs'), *(ROOT / 'core/tests').rglob('*.rs'),
             *(ROOT / 'core/include').glob('*.h'), *PROBES.glob('*.swift'), *PROBES.glob('*.py'), *PROBES.glob('*.c'),
             ROOT / 'core/Cargo.toml', ROOT / 'core/Cargo.lock', ROOT / 'Makefile', ROOT / 'project.yml',
             ROOT / 'app/Info.plist', Path(__file__).resolve(), ROOT / 'scripts/phase5_support.py',
             ROOT / 'scripts/verification_isolation.py',
             ROOT / 'scripts/app_under_test.py',
             ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
             ROOT / 'scripts/host-source-probe/native.py', ROOT / 'scripts/host-source-probe/support.py',
             ROOT / 'scripts/raw-probe/native.py', ROOT / 'scripts/precision-probe/retained_fixtures.py',
             ROOT / 'scripts/precision-probe/retained_linked_fixtures.py', ROOT / 'scripts/precision-probe/fixtures.py',
             ROOT / 'scripts/probe_build.py']
    # The standalone external MCP scenario is neither loaded nor linked when
    # qualifying the host binary. Its separate Release-app run records it.
    if mode == 'host':
        paths = [path for path in paths if path != PROBES / 'mcp.py']
    return {str(path.relative_to(ROOT)): digest(path) for path in sorted(set(paths))}


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def run(command, log, environment, *, timeout=1200):
    with Path(log).open('w') as stream:
        with subprocess.Popen(list(map(str, command)), cwd=ROOT, env=environment,
                              stdout=stream, stderr=subprocess.STDOUT) as process:
            try:
                code = process.wait(timeout=timeout)
            except BaseException:
                process.kill(); process.wait(); raise
    if code:
        raise RuntimeError(f'Command exited {code}; see {log}')
    return process.pid


def host(folder, fixtures, reference, library, reuse=None):
    folder.mkdir()
    sources = probe_build.app_sources()
    probes = probe_build.probe_sources('raw-local-probe')
    compiled = sources + probes + [ROOT / 'app/Bridging/Doctored-Bridging-Header.h', ROOT / 'core/include/doctored_core.h']
    hashes = {str(path.relative_to(ROOT)): digest(path) for path in compiled}
    snapshot = folder / 'source'
    for path in compiled:
        destination = snapshot / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
        assert digest(path) == digest(destination) == hashes[str(path.relative_to(ROOT))]
    archive = folder / 'libdoctored_core.a'
    archive_hash = digest(library)
    shutil.copy2(library, archive)
    assert digest(archive) == digest(library) == archive_hash
    info = plistlib.loads((ROOT / 'app/Info.plist').read_bytes())
    info.update(CFBundleIdentifier='com.kgalligan.doctored.raw-local-probe', CFBundleName='Host Source Probe')
    (folder / 'Info.plist').write_bytes(plistlib.dumps(info))
    private_fixtures = folder / 'fixtures'
    shutil.copytree(fixtures, private_fixtures)
    results = folder / 'results'
    results.mkdir()
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='15.0', ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'),
        DR_RAW_LOCAL_FIXTURES=str(private_fixtures), DR_RAW_LOCAL_RESULTS=str(results),
        DR_ACTIONS_DIR=str(folder / 'actions'), DR_RECOVERY_DIR=str(folder / 'recovery'),
        CLANG_MODULE_CACHE_PATH=str(folder / 'module-cache'), **preset_environment(folder))
    binary = folder / 'probe'
    write(folder / 'compiled-provenance.json', {'files': hashes, 'archive_sha256': archive_hash})
    if reuse is None:
        print('Compiling immutable host snapshot:', folder, flush=True)
        probe_build.build(
            sources=[snapshot / p.relative_to(ROOT) for p in sources + probes],
            bridging_header=snapshot / 'app/Bridging/Doctored-Bridging-Header.h',
            library=folder / 'libdoctored_core.a', info_plist=folder / 'Info.plist',
            output=binary, log=folder / 'build.log', env=environment, timeout=1200)
    else:
        prior = json.loads((reuse / 'compiled-provenance.json').read_text())
        assert prior == {'files': hashes, 'archive_sha256': archive_hash}, 'Reuse requires identical compiled inputs and archive'
        assert digest(reuse / 'libdoctored_core.a') == archive_hash
        assert digest(reuse / 'Info.plist') == digest(folder / 'Info.plist')
        shutil.copy2(reuse / 'probe', binary)
        assert digest(binary) == digest(reuse / 'probe')
        (folder / 'build.log').write_text(f'Reused identical immutable binary from {reuse}\nSHA256 {digest(binary)}\n')
        print('Reusing exact qualified host binary:', reuse, flush=True)
    process = run([binary, '-ApplePersistenceIgnoreState', 'YES',
                   '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'], folder / 'run.log', environment)
    result = json.loads((results / 'host-report.json').read_text())
    assert result['passed']
    required = {'registered-vignette-crop', 'registered-vignette-pad',
                'registered-vignette-exif', 'registered-vignette-affine-chain',
                'registered-vignette-ui-replay-reopen'}
    for depth in (8, 16):
        matching = [group for group in result.get('geometry', [])
                    if any(row.get('depth') == depth for row in group.get('groups', []))]
        assert len(matching) == 1, f'Missing geometry report at depth {depth}'
        group = matching[0]
        names = {row['name'] for row in group['groups']}
        ui = group.get('ui_replay_reopen', {})
        if ui.get('passed') and ui.get('depth') == depth:
            names.add(ui.get('name'))
        assert names == required, f'Missing mandatory registered-vignette reports at depth {depth}: {required - names}'
        assert group.get('optical_reedit', {}).get('passed') is True
        assert group['optical_reedit']['depth'] == depth
        assert group.get('optical_ui', {}).get('passed') is True
        assert group['optical_ui']['depth'] == depth
        assert group['optical_ui']['cancel_preserved_owner'] is True
        assert group.get('migration_map_sheet', {}).get('passed') is True
        assert group['migration_map_sheet']['depth'] == depth
    numeric = verify_host_exports(result, reference, folder)
    current = {str(path.relative_to(ROOT)): digest(path) for path in compiled}
    return {'passed': True, 'result': result, 'independent_source_checks': numeric,
            'exact_master_components': sum(row['exact_source_components'] for row in numeric['native']),
            'group_count': result['group_count'],
            'compiled_input_count': len(compiled), 'compiled_source_hashes': hashes,
            'compiled_checkout_changes': [name for name, value in hashes.items() if current.get(name) != value],
            'archive_sha256': archive_hash, 'binary_sha256': digest(binary),
            'process_id': process, 'owned_process_exited': True, 'scratch': str(folder),
            'reused_binary_from': str(reuse) if reuse is not None else None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['host', 'mcp', 'all'], default='all')
    parser.add_argument('--fixtures', type=Path, default=Path('/tmp/rz-phase8g-fixtures'))
    parser.add_argument('--reference', type=Path, default=Path('/tmp/rz-phase8g-reference-output'))
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_executable())
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    parser.add_argument('--require-current', action='store_true')
    parser.add_argument('--reuse-host', type=Path, help='Prior host scratch directory; every compiled input, archive and embedded Info.plist must match')
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    fixtures, reference = args.fixtures.resolve(), args.reference.resolve()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase8g-workflow-'))
    print('RAW-local qualification scratch:', folder, flush=True)
    before = dependencies(args.mode)
    write(folder / 'source-provenance.json', before)
    fixture_hashes = {str(path.relative_to(fixtures)): digest(path) for path in sorted(fixtures.rglob('*')) if path.is_file()}
    reference_hashes = {str(path.relative_to(reference)): digest(path) for path in sorted(reference.rglob('*')) if path.is_file()}
    report = {'passed': False, 'scratch': str(folder), 'mode': args.mode,
              'fixture_hashes': fixture_hashes, 'reference_hashes': reference_hashes,
              'archive_sha256': digest(args.library)}
    if args.mode in ('mcp', 'all'):
        report['app_sha256'] = digest(args.app)
    try:
        if args.mode in ('host', 'all'):
            report['host'] = host(folder / 'host', fixtures, reference, args.library, args.reuse_host)
            print('Host groups passed:', report['host']['group_count'], flush=True)
        if args.mode in ('mcp', 'all'):
            spec = importlib.util.spec_from_file_location('host_source_mcp', PROBES / 'mcp.py')
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            report['mcp'] = module.run(fixtures, reference, folder / 'mcp', args.app.resolve(),
                                       report.get('host', {}).get('result'))
            assert report['mcp']['passed']
            required = {'registered-vignette-crop', 'registered-vignette-pad',
                        'registered-vignette-exif', 'registered-vignette-affine-chain',
                        'registered-vignette-ui-replay-reopen'}
            if args.mode == 'all' or args.require_current:
                for scenario in report['mcp']['scenarios']:
                    assert required <= set(scenario['groups']), (scenario['document_depth'],
                        required - set(scenario['groups']))
            assert digest(args.app) == report['app_sha256'], 'Release app changed during qualification'
        after = dependencies(args.mode)
        changed = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
        report['source_check'] = {'files': len(before), 'file_set_exact': before.keys() == after.keys(), 'changes': changed}
        current_fixtures = {str(path.relative_to(fixtures)): digest(path)
                            for path in sorted(fixtures.rglob('*')) if path.is_file()}
        current_reference = {str(path.relative_to(reference)): digest(path)
                             for path in sorted(reference.rglob('*')) if path.is_file()}
        report['fixture_file_set_and_hashes_exact'] = current_fixtures == fixture_hashes
        report['reference_file_set_and_hashes_exact'] = current_reference == reference_hashes
        assert current_fixtures == fixture_hashes, 'Shared fixture file set or samples changed'
        assert current_reference == reference_hashes, 'Independent reference file set or samples changed'
        assert digest(args.library) == report['archive_sha256'], 'Core archive changed during qualification'
        if args.require_current:
            assert not changed and report['source_check']['file_set_exact'], changed
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['owned_app_pids'] = [owner.process.pid for owner in HostApp.owners if owner.process is not None]
        report['all_owned_apps_exited'] = all(owner.process is None or owner.process.poll() is not None for owner in HostApp.owners)
        write(folder / 'report.json', report)
        write(args.report, report)


if __name__ == '__main__':
    main()
