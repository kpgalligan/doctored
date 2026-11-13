#!/usr/bin/env python3
"""Compile an immutable actual-app-source TextKit capture/editor probe snapshot."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

# The one list of private preference directories, loaded by path so
# `scripts/` never shadows this probe's own modules on sys.path.
_isolation_spec = importlib.util.spec_from_file_location(
    'verification_isolation', ROOT / 'scripts/verification_isolation.py')
_isolation = importlib.util.module_from_spec(_isolation_spec)
_isolation_spec.loader.exec_module(_isolation)
preset_environment = _isolation.preset_environment
# The shared cached probe build, loaded by path for the same reason.
_build_spec = importlib.util.spec_from_file_location('probe_build', ROOT / 'scripts/probe_build.py')
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def run(command, log, env):
    with log.open('w') as output:
        with subprocess.Popen(list(map(str, command)), cwd=ROOT, env=env,
                              stdout=output, stderr=subprocess.STDOUT) as process:
            try:
                code = process.wait(timeout=1200)
            except BaseException:
                process.kill()
                process.wait()
                raise
    if code:
        raise RuntimeError(f'Command exited {code}; see {log}')
    return process.pid

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--workflow-ui', action='store_true')
    parser.add_argument('--require-current', action='store_true')
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    args = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase8e-text-host-'))
    print('Actual app-source text probe:', folder, flush=True)
    probe_set = 'artwork-workflow-probe' if args.workflow_ui else 'artwork-text-probe'
    sources = probe_build.app_sources()
    probes = probe_build.probe_sources(probe_set)
    compiled = sources + probes + [ROOT / 'app/Bridging/Doctored-Bridging-Header.h', ROOT / 'core/include/doctored_core.h']
    inputs = compiled + [Path(__file__).resolve(), ROOT / 'app/Info.plist',
                         ROOT / 'scripts/verification_isolation.py', ROOT / 'scripts/probe_build.py']
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in inputs}
    snapshot = folder / 'source'
    for p in inputs:
        destination = snapshot / p.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, destination)
        assert digest(p) == digest(destination) == hashes[str(p.relative_to(ROOT))], str(p)
    archive = folder / 'libdoctored_core.a'
    archive_hash = digest(args.library)
    shutil.copy2(args.library, archive)
    assert archive_hash == digest(args.library) == digest(archive)
    info = plistlib.loads((ROOT / 'app/Info.plist').read_bytes())
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.artwork-text-probe'
    info['CFBundleName'] = 'Artwork Text Probe'
    (folder / 'Info.plist').write_bytes(plistlib.dumps(info))
    output = folder / 'results'
    output.mkdir()
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='15.0', ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'),
               DR_ARTWORK_TEXT_PROBE_DIR=str(output), DR_ACTIONS_DIR=str(folder / 'actions'),
               DR_RECOVERY_DIR=str(folder / 'recovery'),
               CLANG_MODULE_CACHE_PATH=str(folder / 'module-cache'), **preset_environment(folder))
    binary = folder / 'probe'
    report = {'passed': False, 'scratch': str(folder), 'source_count': len(hashes), 'compiled_input_count': len(compiled),
              'source_hashes': hashes, 'library_sha256': archive_hash}
    try:
        probe_build.build(
            app_sources=[snapshot / p.relative_to(ROOT) for p in sources],
            sources=[snapshot / p.relative_to(ROOT) for p in probes],
            bridging_header=snapshot / 'app/Bridging/Doctored-Bridging-Header.h',
            library=folder / 'libdoctored_core.a', info_plist=folder / 'Info.plist',
            defines=probe_build.PROBE_SETS[probe_set].defines,
            output=binary, log=folder / 'build.log', env=env, timeout=1200)
        process = run([binary, '-ApplePersistenceIgnoreState', 'YES',
                       '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'], folder / 'run.log', env)
        result = json.loads((output / 'host-report.json').read_text())
        assert result['passed']
        current = {str(p.relative_to(ROOT)): digest(p) for p in inputs}
        report.update(passed=True, result=result, binary_sha256=digest(binary),
                      process_id=process, owned_process_exited=True,
                      source_changes=sorted(name for name in hashes if hashes[name] != current[name]),
                      source_file_set_exact={p.name for p in sources} == {p.name for p in (ROOT / 'app/Sources').glob('*.swift') if p.name != 'main.swift'})
        if args.require_current:
            assert not report['source_changes'] and report['source_file_set_exact'], 'Checkout changed during qualification'
    except BaseException as error:
        report['passed'] = False
        report['error'] = repr(error)
        raise
    finally:
        (folder / 'report.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print('Passed groups:', report['result']['group_count'], flush=True)
    print('Changed checkout inputs:', report['source_changes'], flush=True)

if __name__ == '__main__':
    main()
