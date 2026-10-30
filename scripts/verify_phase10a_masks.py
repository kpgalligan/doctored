#!/usr/bin/env python3
"""Check authored PSD masks, explicit losses/refusals, native storage and history."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('psd_mask_pixels', ROOT / 'scripts/verify_phase10a_pixels.py')
pixels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pixels)
support = pixels.support

ORDINARY_CASES = {
    'ordinary_raw_outside0', 'ordinary_raw_outside255',
    'ordinary_rle_outside0', 'ordinary_rle_outside255', 'disabled_rle',
    'mask_wholly_offcanvas', 'mask_ending_zero', 'mask_signed_min', 'mask_signed_max',
    'grayscale_repeat_mask', 'folder_isolated_mask', 'folder_pass_mask',
    'empty_folder_masks', 'group_independent_move', 'folder_relative_flag_mask',
}
LOSS_CASES = {
    'loss_codec_2': 'compression is unsupported',
    'loss_codec_3': 'compression is unsupported',
    'loss_codec_91': 'compression is unsupported',
    'loss_extended_body': 'metadata length',
    'loss_extended_parameters': 'metadata length',
    'loss_lmgm': 'advanced mask',
    'loss_missing_channel': 'not both present',
    'loss_missing_metadata': 'not both present',
    'loss_obsolete_invert': 'inverted',
    'loss_parameters': 'density/feather',
    'loss_real_only': 'real-user',
    'loss_real_plus_user': 'real-user',
    'loss_rendered': 'rendered',
    'loss_short_body': 'metadata length',
    'loss_unknown_flags': 'unknown',
    'loss_unsupported_default': 'default is not',
    'loss_vector_only': 'advanced mask',
    'loss_vmgm': 'advanced mask',
    'loss_vmsk': 'advanced mask',
    'loss_vsms': 'advanced mask',
}
REFUSAL_CASES = {
    'refusal_divider_mask': 'bounding divider',
    'refusal_duplicate_mask': 'duplicate channel',
    'refusal_group_mask_envelope': '2 GiB decode memory envelope',
    'refusal_mask_aggregate': 'aggregate mask pixels',
    'refusal_mask_empty': 'mask rectangle',
    'refusal_mask_envelope': '2 GiB decode memory envelope',
    'refusal_mask_pixel_cap': 'mask rectangle',
    'refusal_mask_reverse': 'mask rectangle',
    'refusal_mask_signed_overflow': 'mask rectangle',
    'refusal_mask_wide': 'mask rectangle',
    'refusal_raw_long': 'length mismatch',
    'refusal_raw_short': 'length mismatch',
    'refusal_rle_literal': 'RLE row',
    'refusal_rle_repeat': 'RLE row',
    'refusal_rle_spill': 'RLE row',
    'refusal_rle_table': 'RLE row table',
    'refusal_rle_trailing': 'trailing RLE',
    'refusal_rle_underflow': 'decoded width',
}
CASES = ORDINARY_CASES | LOSS_CASES.keys() | REFUSAL_CASES.keys()
MOVED_CASES = {'group_independent_move', 'ordinary_raw_outside0', 'ordinary_raw_outside255',
               'ordinary_rle_outside0', 'ordinary_rle_outside255'}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_corpus(expected):
    """Reject a narrowed/reclassified corpus before starting any application.

    These checks pin authored fixture coverage, not an additional pixel oracle.
    The independent manifests remain the source of raw and rendered expectations.
    """
    assert (len(ORDINARY_CASES), len(LOSS_CASES), len(REFUSAL_CASES)) == (15, 20, 18)
    assert len(CASES) == 53 and set(expected) == CASES, set(expected) ^ CASES
    for name, fixture in expected.items():
        assert 'losses_contain' not in fixture, (name, 'Expected singular loss_contains')
        if name in REFUSAL_CASES:
            assert fixture['layered_available'] is False
            assert fixture['layered_refusal_contains'] == REFUSAL_CASES[name]
            assert 'loss_contains' not in fixture
            continue
        assert fixture.get('layered_available', True) is True
        masks = [layer['mask'] for layer in fixture['layers'] if layer.get('mask') is not None]
        if name in LOSS_CASES:
            assert fixture['loss_contains'] == LOSS_CASES[name], (name, fixture)
            assert not masks, (name, 'Unqualified mask must remain a disclosed loss')
        else:
            assert 'loss_contains' not in fixture and masks, name
            for mask in masks:
                assert mask['outside_coverage'] in (0, 255)
                assert mask['linked'] is False and (mask['density'], mask['feather']) == (1, 0)
                assert isinstance(mask['enabled'], bool)
                assert len(mask['coverage']) == mask['size'][0] * mask['size'][1]
        assert fixture['composite_rgba'] != fixture['stored_composite_rgba'], name
    assert {name for name, fixture in expected.items() if 'moved' in fixture} == MOVED_CASES
    for outside in (0, 255):
        raw = expected[f'ordinary_raw_outside{outside}']
        rle = expected[f'ordinary_rle_outside{outside}']
        assert (raw['layers'], raw['composite_rgba'], raw['moved']) == (rle['layers'], rle['composite_rgba'], rle['moved'])
        layer = raw['layers'][0]
        mask = layer['mask']
        assert layer['kind'] == 'raster' and mask['outside_coverage'] == outside
        assert mask['offset'] == [0, -1] and mask['size'] == [2, 3] and layer['size'] == [4, 2]
        assert 0 in mask['coverage'] and 255 in mask['coverage'] and any(0 < value < 255 for value in mask['coverage'])
    # A folder mask with the position-relative flag set is the same document as
    # the flag-clear one, because the flag is recorded and not applied.
    assert expected['folder_relative_flag_mask'] == expected['folder_isolated_mask']
    assert expected['disabled_rle']['layers'][0]['mask']['enabled'] is False
    assert expected['grayscale_repeat_mask']['layers'][0]['mask']['coverage'] == [63, 63, 63, 1, 127, 255]
    for name, mode in [('folder_isolated_mask', 'Normal'), ('folder_pass_mask', 'Pass Through')]:
        layers = expected[name]['layers']
        assert [layer['depth'] for layer in layers] == [0, 1, 0]
        assert layers[1]['blend'] == 'Multiply'
        group = layers[2]
        assert group['kind'] == 'group' and group['blend'] == mode and group['open'] is False
        assert group['opacity'] == 128 / 255 and group['mask'] is not None
    assert expected['folder_isolated_mask']['composite_rgba'] != expected['folder_pass_mask']['composite_rgba']
    empty = expected['empty_folder_masks']['layers']
    assert len(empty) == 2 and all(layer['kind'] == 'group' for layer in empty)
    assert empty[0]['name'] == empty[1]['name'] and empty[0]['mask'] != empty[1]['mask']
    assert {layer['mask']['outside_coverage'] for layer in empty} == {0, 255}
    assert {layer['mask']['enabled'] for layer in empty} == {False, True}
    group_move = expected['group_independent_move']
    assert group_move['moved']['layer'] == 1 and group_move['layers'][1]['kind'] == 'group'
    assert group_move['moved']['layer_offsets'] == {'0': [0, 0], '1': [0, 0]}
    assert group_move['layers'][0]['offset'] == [1, 0] and group_move['layers'][1]['mask']['offset'] == [0, 0]


def load_corpus(directory):
    manifests = {path.name.removesuffix('.psd.json'): path for path in directory.glob('*.psd.json')}
    assert set(manifests) == CASES, ('Missing/extra manifests', set(manifests) ^ CASES)
    assert {path.stem for path in directory.glob('*.psd')} == CASES, 'PSD files must match the frozen corpus'
    expected = {name: json.loads(path.read_text()) for name, path in manifests.items()}
    validate_corpus(expected)
    for outside in (0, 255):
        assert digest(directory / f'ordinary_raw_outside{outside}.psd') != digest(directory / f'ordinary_rle_outside{outside}.psd')
    for name, fixture in expected.items():
        if name not in REFUSAL_CASES:
            # 28 is mask_outside_native::BYTE_VERSION (the conditional MOUT tag) and 10
            # is drdc::LEGACY_DRDC_VERSION (the plain byte body): fixed tags under the
            # identity wrapper `read_native` peels, never the writer version (45/46).
            fixture['native_version'] = 28 if any(layer.get('mask') is not None and layer['mask']['outside_coverage'] == 255
                                                 for layer in fixture['layers']) else 10
    return manifests, expected


def check_preflight(name, expected, preflight):
    assert preflight['requires_explicit_choice'] and preflight['stored_composite_available'], (name, preflight)
    retained = [text for text in preflight['observations'] if 'ordinary raster mask samples' in text]
    if name in REFUSAL_CASES:
        assert preflight['layered_available'] is False
        assert expected['layered_refusal_contains'] in preflight['layered_refusal']
        assert not retained, (name, preflight)
    elif name in LOSS_CASES:
        assert preflight['layered_available'] is True
        assert any(expected['loss_contains'] in loss for loss in preflight['verified_losses']), (name, preflight)
        assert not retained, (name, preflight)
    else:
        assert preflight['layered_available'] is True and not preflight['verified_losses'], (name, preflight)
        masks = [layer for layer in expected['layers'] if layer.get('mask') is not None]
        assert len(retained) == len(masks), (name, preflight)
        assert all('independent (unlinked)' in text and 'PSD linkage is not inferred or preserved' in text for text in retained)


def mask_workflow(app, native, expected, folder):
    identifier = app.call('open_document', path=str(native))['document']['id']
    call = support.call_for(app, identifier, folder / 'mask-calls.jsonl')
    before = pixels.history(call)
    baseline = pixels.stored(call, folder / 'mask-baseline.rz', expected)
    checked = []
    for index, layer in enumerate(expected['layers']):
        if layer.get('mask') is None:
            continue
        mask = layer['mask']
        properties = call('get_mask_properties', layer=index)
        assert (properties['offset_x'], properties['offset_y']) == tuple(mask['offset'])
        assert (properties['width'], properties['height']) == tuple(mask['size'])
        assert properties['outside_coverage'] == mask['outside_coverage']
        assert properties['linked'] is False
        assert (properties['density'], properties['feather']) == (mask['density'], mask['feather'])
        edited = copy.deepcopy(expected)
        edited['layers'][index]['mask']['density'] = .5
        action_name = native.parent.name + ' mask ' + str(index)
        app.call('start_recording')
        assert call('set_mask_properties', layer=index, density=.5)['changed']
        action = app.call('stop_recording', name=action_name)
        assert [step['tool'] for step in action['steps']] == ['set_mask_properties']
        assert action['steps'][0]['arguments']['layer'] == index
        assert action['steps'][0]['arguments']['density'] == .5
        assert pixels.history(call)[1] == before[1] + 1
        changed = pixels.stored(call, folder / f'mask-{index}-density.rz', edited)
        assert changed != baseline
        call('undo')
        assert pixels.history(call)[0] == before[0]
        assert pixels.stored(call, folder / f'mask-{index}-undone.rz', expected) == baseline
        call('redo')
        assert pixels.stored(call, folder / f'mask-{index}-redone.rz', edited) == changed
        call('undo')
        assert call('run_action', name=action_name)['ok']
        assert pixels.history(call)[1] == before[1] + 1
        assert pixels.stored(call, folder / f'mask-{index}-action.rz', edited) == changed
        call('undo')
        assert pixels.history(call)[:2] == before[:2]
        assert pixels.stored(call, folder / f'mask-{index}-action-undone.rz', expected) == baseline
        checked.append(index)
    return checked


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
    manifests, expectations = load_corpus(args.fixtures.resolve())
    output.mkdir(exist_ok=False)
    executable = (args.app / 'Contents/MacOS/Doctored' if args.app.suffix == '.app' else args.app).resolve()
    hashes = {str(path): digest(path) for manifest in manifests.values() for path in (manifest, manifest.with_suffix(''))}
    report = dict(app=str(executable), app_sha256=digest(executable), fixture_sha256=hashes,
                  script_sha256=digest(Path(__file__)), shared_helper_sha256=digest(Path(pixels.__file__)),
                  dependency_sha256={str(p): digest(p) for p in dependencies()},
                  category_counts=dict(ordinary=15, loss=20, refusal=18), cases={})
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        for name, manifest in sorted(manifests.items()):
            expected = expectations[name]
            folder = output / name
            result = pixels.case(app, manifest.with_suffix(''), expected, folder)
            preflight = result['preflight']
            check_preflight(name, expected, preflight)
            result['category'] = 'ordinary' if name in ORDINARY_CASES else 'loss' if name in LOSS_CASES else 'refusal'
            result['expectation_sha256'] = digest(manifest)
            if name not in REFUSAL_CASES:
                result['native_version'] = expected['native_version']
            if name in ORDINARY_CASES:
                checked = mask_workflow(app, folder / 'original.rz', expected, folder)
                assert checked == [index for index, layer in enumerate(expected['layers']) if layer.get('mask') is not None]
                result['mask_property_action_undo_redo_layers'] = checked
            report['cases'][name] = result
    assert all(digest(Path(path)) == sha for path, sha in hashes.items())
    assert set(report['cases']) == CASES
    assert sum(len(result.get('mask_property_action_undo_redo_layers', [])) for result in report['cases'].values()) == 16
    assert {name for name, result in report['cases'].items() if result.get('move_undo_redo_checked')} == MOVED_CASES
    report['fixtures_unchanged'] = True
    report['checks_passed'] = ['complete frozen corpus: 15 ordinary, 20 explicit losses and 18 refusals',
                             'authored masks and straight raster pixels exact',
                             'Raw/RLE, both outside defaults, disabled masks and distinct folder behavior',
                             'independent original and moved composites',
                             'conditional native28 or native10 and canonical outside table',
                             'native reopen, mask properties, Actions and one Undo',
                             'specific unsupported losses and malformed layer refusals',
                             'explicit saved composite remains available']
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), checked_cases=len(report['cases']), checks_passed=report['checks_passed']), indent=2))


if __name__ == '__main__':
    main()
