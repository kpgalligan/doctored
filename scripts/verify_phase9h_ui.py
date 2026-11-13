#!/usr/bin/env python3
"""Bounded actual-source native paint UI qualification, with isolated app state.

Compiles a snapshot of the real app plus probe-only Swift into one executable.
Uses a caller-supplied current Rust archive; never builds or launches Doctored.app.
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
import tempfile

from verification_isolation import preset_environment

ROOT = Path(__file__).resolve().parents[1]
HERE = ROOT / 'scripts/native-paint-probe'
# The shared cached probe build, loaded by path so `scripts/` never shadows a probe's own modules.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)
# The committed 11F fixture the probe's FLOAT opener reads
# (`openFloatDocument`, main.swift). A probe case that needs it FAILS when it
# is absent; it is never a silently skipped gate.
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
# Cases this runner REQUIRES to have run and passed. Named here so a probe
# file that stops being compiled, or a case that stops being registered,
# cannot turn into a green run over fewer groups (Phase 12E-3b).
REQUIRED_CASES = ('knockout-blending-ui', 'knockout-merge-menu-ui',
                  'stamp-flatten-menu-ui', 'canvas-size-sheet-ui',
                  'image-size-percentage-ui',
                  'float-exact-geometry-menu-ui', 'straighten-crop-tool-ui',
                  'rotate-canvas-sheet-ui', 'geometry-noops-ui',
                  'precision-move-handles-ui', 'precision-transform-handles-ui',
                  'precision-crop-handles-ui', 'precision-geometry-lifecycle-ui',
                  'precision-geometry-float-ui', 'geometry-set-handles-ui',
                  'retained-artwork-geometry-ui', 'persisted-warp-sheet-ui',
                  'byte-brush-record-ui', 'byte-eraser-record-ui',
                  'byte-clone-record-ui', 'byte-heal-record-ui',
                  'byte-spot-heal-record-ui', 'byte-dodge-record-ui',
                  'byte-coverage-record-ui', 'byte-quick-mask-record-ui',
                  'byte-stroke-contracts-ui', 'selection-modifier-feedback-ui',
                  'current-layer-commands-ui', 'shift-click-line-ui',
                  'brush-outline-cursor-ui', 'margin-select-start-ui',
                  'view-items-at-depth-ui', 'canvas-keys-ui',
                  'automation-alerts-ui', 'soft-proof-menu-ui',
                  'export-items-at-depth-ui', 'tool-keys-ui',
                  'rotate-view-stroke-ui', 'rotate-view-gesture-ui',
                  'rotate-view-sheet-ui', 'second-view-ui', 'rotate-view-hover-ui',
                  'workspace-frame-fit-ui', 'selection-marching-ants-ui',
                  'layer-thumbnail-cache-ui', 'broken-library-managers-ui',
                  'new-document-broken-presets-ui', 'workspace-broken-files-ui',
                  'rotated-text-editor-ui')


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
    parser.add_argument('--require-current', action='store_true')
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a')
    args = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9h-ui-'))
    print('Actual app-source native paint UI probe:', folder, flush=True)
    sources = [p for p in sorted((ROOT / 'app/Sources').glob('*.swift')) if p.name != 'main.swift']
    probes = sorted(HERE.glob('*.swift'))
    compiled = sources + probes + [ROOT / 'app/Bridging/Doctored-Bridging-Header.h', ROOT / 'core/include/doctored_core.h']
    inputs = compiled + [Path(__file__).resolve(), ROOT / 'app/Info.plist',
                         ROOT / 'scripts/verification_isolation.py', ROOT / 'scripts/probe_build.py']
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in inputs}
    snapshot = folder / 'source'
    for path in inputs:
        destination = snapshot / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
        assert digest(path) == digest(destination) == hashes[str(path.relative_to(ROOT))], str(path)
    archive = folder / 'libdoctored_core.a'
    archive_hash = digest(args.library)
    shutil.copy2(args.library, archive)
    assert archive_hash == digest(args.library) == digest(archive)
    info = plistlib.loads((ROOT / 'app/Info.plist').read_bytes())
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.native-paint-ui-probe'
    info['CFBundleName'] = 'Native Paint UI Probe'
    (folder / 'Info.plist').write_bytes(plistlib.dumps(info))
    output = folder / 'results'
    output.mkdir()
    second = folder / 'results-pass2'
    second.mkdir()
    # EVERY preference library the probe process can reach is redirected into
    # this run's own folder: the probe shares
    # ~/Library/Application Support/Doctored/* with the real app otherwise,
    # and the shortcut case WRITES one (`preset_environment`).
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='15.0', ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'),
               DR_PAINT_UI_OUTPUT=str(output), DR_ACTIONS_DIR=str(folder / 'actions'),
               DR_RECOVERY_DIR=str(folder / 'recovery'),
               DR_PAINT_UI_FLOAT_FIXTURE=str(FLOAT_FIXTURE),
               CLANG_MODULE_CACHE_PATH=str(folder / 'module-cache'), **preset_environment(folder))
    assert FLOAT_FIXTURE.is_file(), f'The probe float fixture is missing: {FLOAT_FIXTURE}'
    binary = folder / 'probe'
    report = {'passed': False, 'scratch': str(folder), 'source_hashes': hashes,
              'compiled_input_count': len(compiled), 'library_sha256': archive_hash,
              'scope': 'Actual app source, synthetic tablet events, actual controls/timer/cancel button; no hardware tablet claim'}
    try:
        probe_build.build(
            app_sources=[snapshot / p.relative_to(ROOT) for p in sources],
            sources=[snapshot / p.relative_to(ROOT) for p in probes],
            bridging_header=snapshot / 'app/Bridging/Doctored-Bridging-Header.h',
            library=folder / 'libdoctored_core.a', info_plist=folder / 'Info.plist',
            output=binary, log=folder / 'build.log', env=env, timeout=1200)
        arguments = [binary, '-ApplePersistenceIgnoreState', 'YES', '-AgentServerEnabled', 'NO',
                     '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb']
        process = run(arguments, folder / 'run.log', env)
        result = json.loads((output / 'host-report.json').read_text())
        assert result['passed'], result
        # The named cases must have RUN, not merely not-failed: a case that
        # is no longer registered would otherwise leave a green report.
        ran = set(result.get('cases', []))
        reported = {group.get('case') or group.get('name') for group in result.get('groups', [])}
        missing = [name for name in REQUIRED_CASES if name not in ran or name not in reported]
        assert not missing, f'Required probe cases did not run: {missing}'
        # PASS 2: a SECOND process over the same private directories, running
        # only the case that reads what pass 1 left on disk. Its results go in
        # their own folder so neither report can overwrite the other.
        second_env = dict(env, DR_PAINT_UI_OUTPUT=str(second),
                          DR_PAINT_UI_CASES='shortcut-relaunch-ui')
        second_process = run(arguments, folder / 'run-pass2.log', second_env)
        second_result = json.loads((second / 'host-report.json').read_text())
        assert second_result['passed'], second_result
        current = {str(p.relative_to(ROOT)): digest(p) for p in inputs}
        report.update(passed=True, result=result, second_pass=second_result,
                      required_cases=list(REQUIRED_CASES),
                      float_fixture=str(FLOAT_FIXTURE.relative_to(ROOT)),
                      binary_sha256=digest(binary),
                      process_id=process, second_process_id=second_process,
                      owned_process_exited=True,
                      source_changes=sorted(name for name in hashes if hashes[name] != current[name]),
                      source_file_set_exact={p.name for p in sources} == {p.name for p in (ROOT / 'app/Sources').glob('*.swift') if p.name != 'main.swift'})
        if args.require_current:
            assert not report['source_changes'] and report['source_file_set_exact'], 'Checkout changed during qualification'
            assert digest(args.library) == archive_hash, 'Core archive changed during qualification'
    except BaseException as error:
        report['passed'] = False
        report['error'] = repr(error)
        host = output / 'host-report.json'
        if host.exists():
            report['result'] = json.loads(host.read_text())
        host2 = second / 'host-report.json'
        if host2.exists():
            report['second_pass'] = json.loads(host2.read_text())
        raise
    finally:
        encoded = json.dumps(report, indent=2, sort_keys=True) + '\n'
        (folder / 'report.json').write_text(encoded)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded)
    print('Passed groups:', report['result']['group_count'],
          '+ pass 2:', report['second_pass']['group_count'], flush=True)
    print('Changed checkout inputs:', report['source_changes'], flush=True)


if __name__ == '__main__':
    main()
