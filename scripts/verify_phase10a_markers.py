#!/usr/bin/env python3
"""Qualify finite PSD metadata notices through an owned Release application.

The 60 binary PSDs and adjacent expectations are authored by the independent
FFI fixture writer. These cases qualify marker disclosure, default handling and
unchanged raster/native storage, not descriptor interpretation or Photoshop
appearance. No product parser or encoded output supplies the fixture oracle.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('marker_pixel_helpers', ROOT / 'scripts/verify_phase10a_pixels.py')
pixels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pixels)
support, digest = pixels.support, pixels.digest

DESCRIPTORS = {
    **{key: 'layer-style metadata' for key in ('lrFX', 'lfx2', 'lmfx')},
    **{key: 'adjustment/fill metadata' for key in ('SoCo', 'GdFl', 'PtFl', 'brit', 'levl', 'curv', 'expA',
       'vibA', 'hue ', 'hue2', 'blnc', 'blwh', 'phfl', 'mixr', 'clrL', 'nvrt', 'post', 'thrs', 'grdm', 'selc')},
    **{key: 'text/type metadata' for key in ('tySh', 'TySh', 'Txt2')},
    **{key: 'placed/smart-object or linked-layer metadata' for key in ('plLd', 'PlLd', 'SoLd', 'SoLE', 'lnkD', 'lnk2', 'lnk3', 'lnkE')},
    **{key: 'vector stroke/origination metadata' for key in ('vstk', 'vscg', 'vogk')},
}
def marker_name(key):
    # PSD keys are case-sensitive; fixture paths must also remain distinct on
    # a case-insensitive filesystem (tySh/TySh and plLd/PlLd are separate keys).
    return 'marker-' + key.replace(' ', '_') + '-' + key.encode('ascii').hex()


LOSSES = {marker_name(key): [value] for key, value in DESCRIPTORS.items()}
LOSSES.update({name: ['nondefault or unrecognized Blend If'] for name in ('changed-ranges', 'unrecognized-ranges')})
LOSSES.update({f'changed-knko-{size}': ['nonzero or unrecognized knockout'] for size in (2, 4)})
LOSSES.update({f'changed-lspf-{size}': ['nonzero or unrecognized layer-protection'] for size in (2, 4)})
LOSSES.update({'duplicate-effects': ['layer-style metadata'],
               'conflicting-protection': ['nonzero or unrecognized layer-protection'],
               'protected-header': ['layer-protection metadata'],
               'irrelevant-pixels': ['raster pixels irrelevant to document appearance']})
OBSERVATIONS = {
    'default-ranges-1': ['unrestricted default Blend If ranges'],
    'default-ranges-4': ['unrestricted default Blend If ranges'],
    'default-knko': ['disabled knockout flag'],
    'default-lspf': ['zero layer-protection flags'],
    'default-clbl': ['additional clipping/interior blending metadata'],
    'default-infx': ['additional clipping/interior blending metadata'],
    'default-iOpa': ['fill-opacity metadata'],
    **{f'opaque-{key}': ['payload is not interpreted or retained'] for key in ('clbl', 'infx', 'iOpa')},
}
QUIET = {'plain', 'uninspected-keys', 'invalid-pixel-meaning'}
NAMES = LOSSES.keys() | OBSERVATIONS.keys() | QUIET
RGBA = [11, 29, 47, 255, 101, 151, 199, 0]
RENDER = [11, 29, 47, 255, 0, 0, 0, 0]
STORED = [13, 27, 41, 255, 13, 27, 41, 255]


def load_corpus(directory):
    paths = {p.name.removesuffix('.psd.json'): p for p in directory.glob('*.psd.json')}
    assert set(paths) == NAMES and {p.stem for p in directory.glob('*.psd')} == NAMES
    assert (len(DESCRIPTORS), len(LOSSES), len(OBSERVATIONS), len(QUIET), len(NAMES)) == (37, 47, 10, 3, 60)
    assert len({value for value in DESCRIPTORS.values()}) == 5
    expected = {name: json.loads(path.read_text()) for name, path in paths.items()}
    for name, value in expected.items():
        assert value['canvas'] == [2, 1]
        assert value['loss_fragments'] == LOSSES.get(name, [])
        assert value['observation_fragments'] == OBSERVATIONS.get(name, [])
        assert len(value['layers']) == 1
        assert value['layers'][0] == dict(name='Duplicate', kind='raster', depth=0, offset=[0, 0],
                                          size=[2, 1], rgba=RGBA, visible=True, opacity=1,
                                          blend='Normal', open=True, mask=None)
        assert value['composite_rgba'] == RENDER and value['stored_composite_rgba'] == STORED
        # 10 is drdc::LEGACY_DRDC_VERSION (the plain byte body): a fixed tag under
        # the identity wrapper `read_native` peels, never the writer version (45).
        value['native_version'] = 10
    assert len({digest(path.with_suffix('')) for path in paths.values()}) == 60, 'Every marker/control must have distinct input bytes'
    return paths, expected


def check_report(name, expected, report):
    assert report['layered_available'] and report['stored_composite_available'] and report['requires_explicit_choice']
    assert report['metadata_captured'] is False
    assert report['layer_count'] == 1 and report['group_count'] == 0
    losses, observations = report['verified_losses'], report['observations']
    assert len(losses) == len(expected['loss_fragments']), (name, losses)
    for fragment in expected['loss_fragments']:
        assert any(fragment in text for text in losses), (name, losses, fragment)
    assert all(text.startswith('Layer 0 (“Duplicate”) ') for text in losses), (name, losses)
    for fragment in expected['observation_fragments']:
        assert any(fragment in text for text in observations), (name, observations, fragment)
    if name in QUIET:
        assert not any('metadata' in text or 'irrelevant to document appearance' in text for text in observations)
    if name in ('conflicting-protection', 'protected-header'):
        assert not any('zero layer-protection flags' in text for text in observations)
    if name in {marker_name(key) for key in ('lrFX', 'lfx2', 'lmfx')} | {'duplicate-effects'}:
        assert 'without interpreting whether effects are active' in losses[0]
    if name.startswith('opaque-') or name in ('default-clbl', 'default-infx', 'default-iOpa'):
        assert any('presence alone does not establish' in text for text in observations)
    assert any('unknown keys and global additional-info blocks are not interpreted' in text for text in report['general_limitations'])


def dependencies():
    """The code files `dependency_sha256` hashes: the script's whole load chain."""
    return [Path(__file__), Path(pixels.__file__),
            ROOT / 'scripts/host-source-probe/support.py', ROOT / 'scripts/live-filter-probe/native.py',
            ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
            ROOT / 'scripts/precision-probe/retained_fixtures.py',
            ROOT / 'scripts/precision-probe/retained_linked_fixtures.py', ROOT / 'scripts/raw-probe/native.py',
            ROOT / 'scripts/retouch-probe/fixtures.py', ROOT / 'scripts/retouch-probe/support.py',
            ROOT / 'scripts/verification_isolation.py',
            ROOT / 'scripts/app_under_test.py', ROOT / 'scripts/verify_phase1.py',
            ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
            *sorted((ROOT / 'core/src').rglob('*.rs'))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error('output must be new and outside the repository')
    paths, expectations = load_corpus(args.fixtures.resolve())
    output.mkdir(exist_ok=False)
    executable = (args.app / 'Contents/MacOS/Doctored' if args.app.suffix == '.app' else args.app).resolve()
    hashes = {str(path): digest(path) for manifest in paths.values() for path in (manifest, manifest.with_suffix(''))}
    report = dict(app=str(executable), app_sha256=digest(executable), script_sha256=digest(Path(__file__)),
                  shared_helper_sha256=digest(Path(pixels.__file__)), fixture_sha256=hashes,
                  dependency_sha256={str(p): digest(p) for p in dependencies()},
                  category_counts=dict(descriptor_markers=37, controls=23, total=60), cases={})
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        for name, path in sorted(paths.items()):
            expected = expectations[name]
            result = pixels.case(app, path.with_suffix(''), expected, output / name)
            check_report(name, expected, result['preflight'])
            result['expectation_sha256'] = digest(path)
            report['cases'][name] = result
    assert set(report['cases']) == NAMES
    assert len({case['native_sha256'] for case in report['cases'].values()}) == 1, 'Discarded metadata must not alter native document bytes'
    assert all(digest(Path(path)) == wanted for path, wanted in hashes.items())
    report['fixtures_unchanged'] = True
    report['checks_passed'] = ['all60 fixed cases, including every37 descriptor marker key and23 defaults/opaque/header controls',
        'finite per-file notices distinguish descriptor presence, known simple defaults and uninterpreted payloads',
        'unknown keys do not acquire invented feature semantics',
        'unchanged exact raw RGBA, hidden RGB, native10 bytes and canonical reopen across every case',
        'independently expected layered rendering and separate explicit stored-composite choice']
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), cases=len(report['cases'])), indent=2))


if __name__ == '__main__':
    main()
