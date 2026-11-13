#!/usr/bin/env python3
"""Qualify actual retouch canvas events, source overlays, history, and recorded Actions."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import runpy
import shutil
import subprocess
import tempfile
import sys
sys.dont_write_bytecode = True
from verification_isolation import preset_environment

ROOT = Path(__file__).resolve().parents[1]
HERE = ROOT / "scripts/retouch-probe"
# The shared cached probe build, loaded by path so `scripts/` never shadows a probe's own modules.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
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
    parser.add_argument('--fixtures', type=Path, default=Path('/tmp/rz-phase9d-fixtures'))
    parser.add_argument('--require-current', action='store_true')
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    args = parser.parse_args()
    fixture = runpy.run_path(str(HERE / 'fixtures.py'))
    fixture['build'](args.fixtures)
    for depth in [8, 16]:
        pixels = fixture['photograph'](depth)
        maximum = (1 << depth) - 1
        for y in range(48):
            for x in range(64):
                if (x - 30)**2 + (y - 22)**2 < 16:
                    pixels[y * 64 + x] = (maximum, maximum // 10, maximum // 20, maximum)
        (args.fixtures / f'red-eye-{depth}.png').write_bytes(
            fixture['png'](64, 48, depth, pixels, fixture['srgb_profile']()))

    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9d-ui-'))
    print('Actual app-source retouch UI probe:', folder, flush=True)
    sources = [p for p in sorted((ROOT / 'app/Sources').glob('*.swift')) if p.name != 'main.swift']
    probes = [HERE / 'main.swift']
    compiled = sources + probes + [ROOT / 'app/Bridging/Doctored-Bridging-Header.h', ROOT / 'core/include/doctored_core.h']
    inputs = compiled + [Path(__file__).resolve(), ROOT / 'app/Info.plist', HERE / 'fixtures.py',
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
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.retouch-ui-probe'
    info['CFBundleName'] = 'Retouch UI Probe'
    (folder / 'Info.plist').write_bytes(plistlib.dumps(info))
    output = folder / 'results'
    output.mkdir()
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='15.0', ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'),
               DR_RETOUCH_UI_OUTPUT=str(output), DR_RETOUCH_UI_FIXTURES=str(args.fixtures), DR_ACTIONS_DIR=str(folder / 'actions'),
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
            output=binary, log=folder / 'build.log', env=env, timeout=1200)
        process = run([binary, '-ApplePersistenceIgnoreState', 'YES', '-AgentServerEnabled', 'NO',
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
