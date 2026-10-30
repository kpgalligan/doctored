#!/usr/bin/env python3
"""Build the internal typed host probe; all executable/data/settings stay private.

The linked probe is built by `scripts/probe_build.py` and cached under
`build/probe-cache`; the copy that runs is in the private scratch directory.

Run after the coordinated release Rust library is current. --large adds a fresh
process for native24MP resource qualification. Its report marks every refusal as
a failed size target; a successful runner exit alone does not mean24MP passed.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from fixtures import create, create_large, create_large_color_reference, create_tone, verify

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
assert Path(sys.modules['fixtures'].__file__).resolve().parent == HERE, sys.modules['fixtures'].__file__

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


def measured_sources():
    return [*sorted(p for p in (ROOT / 'app/Sources').glob('*.swift') if p.name != 'main.swift'),
            *sorted(HERE.glob('*.swift')), *sorted(HERE.glob('*.py')),
            ROOT / 'core/include/doctored_core.h', ROOT / 'app/Bridging/Doctored-Bridging-Header.h',
            ROOT / 'scripts/verification_isolation.py', ROOT / 'scripts/probe_build.py']


def logged(command, path, environment, timeout):
    with path.open('w') as output:
        print('$ ' + shlex.join(map(str, command)), file=output, flush=True)
        child = subprocess.Popen(list(map(str, command)), cwd=ROOT, env=environment,
                                 stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            status = child.wait(timeout=timeout)
        except BaseException:
            # The session contains only this runner's compiler/probe and the
            # time wrapper. Never leave the probe running when its parent times out.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            raise
        if status:
            raise subprocess.CalledProcessError(status, command)


def run(scratch, large, large_color, large_pixels, large_public, library_source):
    if not library_source.is_file():
        raise RuntimeError('Build the coordinated release Rust library before running this probe.')
    # Xcode can rebuild its top-level archive from identical source while a
    # separate source probe compiles. Link one immutable, verified private copy.
    library_dir = scratch / 'lib'
    library_dir.mkdir()
    library = library_dir / 'libdoctored_core.a'
    digest = lambda path: hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
    before = digest(library_source)
    shutil.copy2(library_source, library)
    copied, after = digest(library), digest(library_source)
    if before != copied or copied != after:
        raise RuntimeError('The release archive changed during its private copy; retry after the build finishes.')
    (scratch / 'library-provenance.json').write_text(json.dumps({
        'source': str(library_source), 'private_copy': str(library),
        'before_sha256': before, 'copy_sha256': copied, 'after_sha256': after}, indent=2))
    files = scratch / 'files'
    files.mkdir()
    profile = Path('/System/Library/ColorSync/Profiles/Display P3.icc').read_bytes()
    create(files, profile)
    destination_profile = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc').read_bytes()
    create_tone(files, profile, destination_profile)
    environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(scratch / 'clang-cache'),
        ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=os.environ.get('DR_WORKING_LIMIT_BYTES', '2147483648'), DR_ACTIONS_DIR=str(scratch / 'actions'),
        DR_RECOVERY_DIR=str(scratch / 'recovery'), DR_PRECISION_PROBE_DIR=str(files),
        **preset_environment(scratch))
    environment.pop('DR_PRECISION_LARGE', None)
    environment.pop('DR_PRECISION_LARGE_COLOR', None)
    environment.pop('DR_PRECISION_LARGE_PIXELS', None)
    environment.pop('DR_PRECISION_LARGE_PUBLIC', None)
    with (ROOT / 'app/Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    info['CFBundleIdentifier'] = 'com.kgalligan.doctored.precision-probe'
    with (scratch / 'Info.plist').open('wb') as stream:
        plistlib.dump(info, stream)
    binary = scratch / 'probe'
    source_provenance = {str(p.relative_to(ROOT)): digest(p) for p in measured_sources()}
    (scratch / 'source-provenance.json').write_text(json.dumps(source_provenance, indent=2, sort_keys=True))
    probe_build.build(
        sources=[*probe_build.app_sources(), *probe_build.probe_sources('precision-probe')],
        bridging_header=ROOT / 'app/Bridging/Doctored-Bridging-Header.h', library=library,
        info_plist=scratch / 'Info.plist', output=binary, log=scratch / 'build.log',
        env=environment, timeout=900)
    command = ['/usr/bin/time', '-l', binary, '-ApplePersistenceIgnoreState', 'YES',
               '-RecoveryDiscoverySuppressed', 'YES',
               '-ColorWorkingSpace', 'srgb']
    logged(command, scratch / 'run.log', environment, 300)
    print((scratch / 'run.log').read_text())
    verify(files, profile)
    if large or large_color or large_pixels or large_public:
        create_large(files, profile)
    if large:
        environment['DR_PRECISION_LARGE'] = '1'
        logged(command, scratch / 'large-run.log', environment, 900)
        print((scratch / 'large-run.log').read_text())
        print('24MP outcome:', (files / 'large-report.json').read_text())
        environment.pop('DR_PRECISION_LARGE', None)
    if large_color:
        create_large_color_reference(files, profile, destination_profile)
        environment['DR_PRECISION_LARGE_COLOR'] = '1'
        logged(command, scratch / 'large-color-run.log', environment, 900)
        print((scratch / 'large-color-run.log').read_text())
        print('24MP color outcome:', (files / 'large-color-report.json').read_text())
        environment.pop('DR_PRECISION_LARGE_COLOR', None)
    if large_pixels:
        environment['DR_PRECISION_LARGE_PIXELS'] = '1'
        logged(command, scratch / 'large-pixels-run.log', environment, 900)
        print((scratch / 'large-pixels-run.log').read_text())
        print('24MP pixel outcome:', (files / 'large-pixels-report.json').read_text())
        verify_pixel_measurement(json.loads((files / 'large-pixels-report.json').read_text()))
        environment.pop('DR_PRECISION_LARGE_PIXELS', None)
    if large_public:
        environment['DR_PRECISION_LARGE_PUBLIC'] = '1'
        logged(command, scratch / 'large-public-run.log', environment, 900)
        print((scratch / 'large-public-run.log').read_text())
        print('24MP public outcome:', (files / 'large-public-report.json').read_text())
        verify_public_measurement(json.loads((files / 'large-public-report.json').read_text()))
    (scratch / 'provenance.txt').write_text(
        'release_staticlib_sha256=' + hashlib.sha256(library.read_bytes()).hexdigest() + '\n'
        + 'system_display_p3_sha256=' + hashlib.sha256(profile).hexdigest() + '\n')
    current_provenance = {str(p.relative_to(ROOT)): digest(p) for p in measured_sources()}
    mismatches = sorted(path for path in source_provenance.keys() | current_provenance.keys()
                        if source_provenance.get(path) != current_provenance.get(path))
    (scratch / 'source-check.json').write_text(json.dumps({
        'file_count': len(source_provenance), 'current_file_set_exact': source_provenance.keys() == current_provenance.keys(),
        'mismatches': mismatches, 'passed': not mismatches}, indent=2))
    if mismatches:
        raise RuntimeError('Probe sources changed during qualification: ' + ', '.join(mismatches))



def verify_pixel_measurement(report):
    """Refusal/missing resource data/remaining owners all fail qualification."""
    assert report['width'] == 6000 and report['height'] == 4000
    assert report['working_limit_bytes'] == 2 * 1024**3
    assert report['complete_workflow'] is True and report['resizing_permitted'] is False
    assert len(report['stages']) == 21
    fields = ('unique_retained_bytes', 'transient_estimate_bytes',
              'host_precision_document_handles', 'host_precision_image_handles',
              'host_precision_capture_payloads', 'precision_capture_payload_bytes')
    for stage in report['stages']:
        assert stage['status'] == 'completed', stage
        assert 'resource_report_error' not in stage
        for key in ('before', 'after'):
            for field in fields:
                assert type(stage[key][field]) is int and stage[key][field] >= 0
    for field in fields:
        assert report['after_releasing_sources'][field] == 0, field


def verify_public_measurement(report):
    """The actual editor workflow must finish at native size with real history."""
    assert report['width'] == 6000 and report['height'] == 4000
    assert report['working_limit_bytes'] == 2 * 1024**3
    assert report['complete_workflow'] is True and report['resizing_permitted'] is False
    assert report['public_document_window'] is True and report['all_resources_released'] is True
    assert report['refusals_fail_size_target'] is True
    assert len(report['stages']) == 9
    fields = ('unique_retained_bytes', 'transient_estimate_bytes', 'host_document_handles', 'host_image_handles',
              'host_precision_document_handles', 'host_precision_image_handles',
              'host_precision_capture_payloads', 'precision_capture_payload_bytes')
    for stage in report['stages']:
        assert stage['status'] == 'completed', stage
        assert 'resource_report_error' not in stage
        for key in ('before', 'after'):
            for field in fields:
                assert type(stage[key][field]) is int and stage[key][field] >= 0
    for field in fields:
        assert report['after_releasing_sources'][field] == 0, field


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--keep', action='store_true')
    parser.add_argument('--large', action='store_true')
    parser.add_argument('--large-color', action='store_true', help='Add separate native24MP tone/profile qualification.')
    parser.add_argument('--large-pixels', action='store_true', help='Add separate native24MP geometry/scalar/paint qualification.')
    parser.add_argument('--large-public', action='store_true', help='Add separate native24MP public window/history/save/export qualification.')
    parser.add_argument('--library', type=Path, default=ROOT / 'core/target/release/libdoctored_core.a',
                        help='Coordinated release archive to copy privately before linking.')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        parser.error('Requires macOS and the Apple Silicon Swift/AppKit toolchain.')
    scratch = Path(tempfile.mkdtemp(prefix='rasterize-precision-probe-'))
    print('Probe scratch directory:', scratch, flush=True)
    try:
        run(scratch, args.large, args.large_color, args.large_pixels, args.large_public,
            args.library.resolve())
    except (OSError, RuntimeError, AssertionError, subprocess.SubprocessError) as error:
        print('Probe failed:', error, '\nArtifacts retained at', scratch, file=sys.stderr)
        for name in ['build.log', 'run.log', 'large-run.log', 'large-color-run.log', 'large-pixels-run.log', 'large-public-run.log']:
            if (scratch / name).is_file():
                print(name, '\n'.join((scratch / name).read_text(errors='replace').splitlines()[-30:]), file=sys.stderr)
        return 1
    if args.keep:
        print('Artifacts retained at', scratch)
    else:
        shutil.rmtree(scratch)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
