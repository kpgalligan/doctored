#!/usr/bin/env python3
"""Qualify the pinned independently authored PSD corpus in an owned Release app.

GIMP authors and reopens the source PSD; its two layer-derived PNGs are appearance
references. A separate small PSD decoder checks exact storage against GIMP's
recorded layer data and the existing independent native decoder. Render tolerance
is fixed in designs/phase-10a-qualification.md before any Doctored measurement.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
import psd_qualification_decode as psd
import verify_phase10a_masks as masks
import verify_phase10a_pixels as pixels

ROOT = Path(__file__).resolve().parents[1]
support = pixels.support
BLENDS = {'norm': 0, 'mul ': 1, 'scrn': 2, 'pass': 27}
GIMP_BLENDS = {'normal': 0, 'normal-legacy': 0, 'multiply': 1, 'multiply-legacy': 1,
               'screen': 2, 'screen-legacy': 2, 'pass-through': 27}
MAX_APPEARANCE_ERROR = 2
CASES = {'rgb_mask', 'rgb_mask_disabled', 'rgb_partial_alpha', 'rgb_offcanvas',
         'rgb_group_isolated', 'rgb_group_pass', 'rgb_nested_hidden', 'gray_mask',
         'rgb_p3_ppi', 'rgb_saved_channel'}
LOSSES = {'rgb_saved_channel': 'additional saved channels'}
REFUSALS = {'gray_mask': 'embedded ICC profile is unsupported or invalid'}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def reference(path, canvas, profile=None):
    decoded = support.png_samples(path.read_bytes())
    assert (decoded['width'], decoded['height'], decoded['depth']) == (*canvas, 8)
    assert len(decoded['samples']) == canvas[0] * canvas[1] * 4
    if profile is not None:
        assert decoded['profile'] == profile, (path, 'PNG ICC must match the document-code domain')
    return decoded['samples']


def appearance(actual, wanted):
    assert len(actual) == len(wanted) and len(actual) % 4 == 0
    raw_errors = [abs(a - b) for a, b in zip(actual, wanted)]
    errors = list(raw_errors)
    transparent = 0
    for index in range(0, len(actual), 4):
        if actual[index + 3] == wanted[index + 3] == 0:
            # RGB at joint zero alpha contributes no appearance on any backdrop.
            # Raw PSD/native source RGB has a separate exact gate; keep the
            # original render-code discrepancy visible in this receipt too.
            errors[index:index + 3] = [0, 0, 0]
            transparent += 1
    maximum = [max(errors[c::4]) for c in range(4)]
    mean = [sum(errors[c::4]) / len(errors[c::4]) for c in range(4)]
    result = dict(max_rgba=maximum, mean_rgba=mean, threshold=MAX_APPEARANCE_ERROR,
                  unnormalized_max_rgba=[max(raw_errors[c::4]) for c in range(4)],
                  joint_zero_alpha_pixels=transparent)
    assert max(maximum) <= MAX_APPEARANCE_ERROR, result
    if all(a == 255 for a in wanted[3::4]):
        assert all(a == 255 for a in actual[3::4]), 'Opaque backdrop alpha changed'
        result['opaque_alpha_exact'] = True
    return result


def backdrop_appearance(actual, wanted):
    """Visible reference for irreversible low-alpha merged-preview quantization."""
    assert len(actual) == len(wanted) and tuple(actual[3::4]) == tuple(wanted[3::4])
    result = {}
    for name, backdrop in [('black', 0), ('white', 255)]:
        errors = [[], [], []]
        for i in range(0, len(actual), 4):
            for c in range(3):
                a = (actual[i + c] * actual[i + 3] + backdrop * (255 - actual[i + 3])) / 255
                b = (wanted[i + c] * wanted[i + 3] + backdrop * (255 - wanted[i + 3])) / 255
                errors[c].append(abs(a - b))
        maximum = [max(channel) for channel in errors]
        assert max(maximum) <= MAX_APPEARANCE_ERROR, (name, maximum)
        result[name] = dict(max_rgb=maximum, mean_rgb=[sum(e) / len(e) for e in errors],
                            threshold=MAX_APPEARANCE_ERROR)
    return result


def audit_source(folder):
    manifest = json.loads((folder / 'manifest.json').read_text())
    assert manifest['schema'] == 1 and manifest['name'] == folder.name
    assert manifest['application'] == {'name': 'GIMP', 'version': '3.2.6'}
    assert manifest.get('expected_refusal_contains') == REFUSALS.get(folder.name)
    assert manifest.get('expected_loss_contains') == LOSSES.get(folder.name)
    assert {'source.psd', 'authored.png', 'reopened.png'} <= manifest['files'].keys()
    for name, sha in manifest['files'].items():
        assert Path(name).name == name and digest(folder / name) == sha
    decoded = psd.decode(folder / 'source.psd')
    for stage in ('author', 'reopened'):
        observed = manifest[stage]
        assert observed['canvas'] == decoded['canvas']
        assert observed['precision'] == 'u8-non-linear'
        assert observed['color_mode'] == ('rgb' if decoded['color'] == 3 else 'gray')
        assert observed['resolution'] == decoded['resolution']
        assert len(observed['saved_channels']) == len(decoded['auxiliary'])
        for recorded, stored in zip(observed['saved_channels'], decoded['auxiliary'], strict=True):
            assert recorded['size'] == decoded['canvas'] and bytes(recorded['coverage']) == stored
        if decoded['profile'] is not None:
            assert observed['profile_sha256'] == hashlib.sha256(decoded['profile']).hexdigest()
            assert observed['profile'] in manifest['files']
            assert (folder / observed['profile']).read_bytes() == decoded['profile']
        for raw, row in zip(decoded['layers'], observed['layers'], strict=True):
            for key in ('name', 'kind', 'depth', 'visible'):
                assert raw[key] == row[key], (folder, stage, key, raw[key], row[key])
            assert BLENDS[raw['blend']] == GIMP_BLENDS[row['blend']]
            if stage == 'author':
                assert round(row['opacity'] * 255) == round(raw['opacity'] * 255)
            else:
                assert abs(raw['opacity'] - row['opacity']) < 1e-7
            if raw['kind'] == 'raster':
                assert raw['offset'] == row['offset'] and raw['size'] == row['size']
                assert raw['rgba'] == bytes(row['rgba']), (folder, stage, row['name'], 'raw RGBA')
            else:
                assert raw['open'] == row['open']
            assert (raw['mask'] is None) == (row['mask'] is None)
            if raw['mask'] is not None:
                for key in ('size', 'offset', 'coverage', 'enabled'):
                    assert raw['mask'][key] == row['mask'][key], (folder, stage, row['name'], key)
    expected = dict(canvas=decoded['canvas'], layers=[])
    for row in decoded['layers']:
        group = row['kind'] == 'group'
        expected['layers'].append(dict(
            name=row['name'], kind=row['kind'], depth=row['depth'], visible=row['visible'],
            opacity=row['opacity'], blend=BLENDS[row['blend']], open=row['open'],
            size=[1, 1] if group else row['size'],
            offset=(row['mask']['offset'] if row['mask'] else [0, 0]) if group else row['offset'],
            rgba=[0, 0, 0, 0] if group else list(row['rgba']), mask=row['mask']))
    # 28 is mask_outside_native::BYTE_VERSION (the conditional MOUT tag) and 10
    # is drdc::LEGACY_DRDC_VERSION (the plain byte body): fixed tags under the
    # identity wrapper `read_native` peels, never the writer version (45/46).
    expected['native_version'] = 28 if any(row['mask'] and row['mask']['outside_coverage'] == 255
                                          for row in decoded['layers']) else 10
    if folder.name in REFUSALS:
        assert decoded['color'] == 1 and decoded['profile'][16:20] == b'GRAY'
        return manifest, decoded, expected, dict(appearance_qualified=False,
            reason='Independent exporter embeds a gray ICC; this fixture qualifies explicit refusal')
    assert decoded['color'] == 3 and decoded['profile'][16:20] == b'RGB '
    authored = reference(folder / 'authored.png', decoded['canvas'], decoded['profile'])
    reopened = reference(folder / 'reopened.png', decoded['canvas'], decoded['profile'])
    # First validate independent writer/reader consistency, without Doctored.
    assert authored == reopened, 'Pinned GIMP author/reopened reference samples must remain exact'
    comparison = appearance(authored, reopened)
    return manifest, decoded, expected, comparison


def audit_corpus(cases):
    """Pin the claimed evidence, so keeping filenames cannot hide reduced coverage."""
    audited = {folder.name: audit_source(folder) for folder in cases}
    for name, (manifest, decoded, _, _) in audited.items():
        assert decoded['canvas'] == [4, 3] and decoded['resolution'] == [300, 240]
        assert manifest['opaque_backdrop'] is (name != 'rgb_partial_alpha')
        if manifest['opaque_backdrop']:
            for reference_name in ('authored.png', 'reopened.png'):
                assert all(a == 255 for a in reference(cases[0].parent / name / reference_name, [4, 3])[3::4])
        assert len(decoded['auxiliary']) == (1 if name == 'rgb_saved_channel' else 0)
        assert all(not row['clipped'] for row in decoded['layers'])
    plain = audited['rgb_mask'][1]['layers'][1]['mask']
    disabled = audited['rgb_mask_disabled'][1]['layers'][1]['mask']
    assert plain['enabled'] is True and disabled['enabled'] is False
    assert plain['coverage'] == disabled['coverage'] and {0, 255} <= set(plain['coverage'])
    assert any(0 < value < 255 for value in plain['coverage'])
    partial = audited['rgb_partial_alpha'][1]['layers'][0]['rgba']
    assert set(partial[3::4]) == {0, 1, 127, 255}
    assert any(any(partial[i:i + 3]) for i in range(0, len(partial), 4) if partial[i + 3] == 0)
    offcanvas = audited['rgb_offcanvas'][1]['layers']
    assert offcanvas[1]['offset'] == [-2, -1] and offcanvas[1]['size'] == [6, 4]
    assert offcanvas[1]['mask']['offset'] == [-2, -1]
    assert offcanvas[2]['offset'] == [8, -3] and offcanvas[2]['size'] == [2, 2]
    assert offcanvas[2]['visible'] is False and not any(offcanvas[2]['rgba'][3::4])
    assert any(offcanvas[2]['rgba'][0::4])
    for name, mode in [('rgb_group_isolated', 'norm'), ('rgb_group_pass', 'pass')]:
        rows = audited[name][1]['layers']
        assert [row['depth'] for row in rows] == [0, 1, 1, 0]
        assert [row['blend'] for row in rows] == ['norm', 'mul ', 'scrn', mode]
        assert rows[-1]['kind'] == 'group' and rows[-1]['mask'] is not None
        assert rows[-1]['opacity'] == 128 / 255 and rows[-1]['open'] is False
    nested = audited['rgb_nested_hidden'][1]['layers']
    assert max(row['depth'] for row in nested) == 2
    assert any(row['kind'] == 'group' and not row['visible'] for row in nested)
    assert {row['open'] for row in nested if row['kind'] == 'group'} == {False, True}
    assert audited['rgb_p3_ppi'][1]['profile'] != audited['rgb_mask'][1]['profile']
    return {name: values[3] for name, values in audited.items()}


def audit_merged(source_folder, merged_folder, decoded):
    """Compare the interpreted preview with GIMP's separate merged-only reader."""
    manifest = json.loads((merged_folder / 'manifest.json').read_text())
    assert manifest['schema'] == 1 and manifest['name'] == source_folder.name == merged_folder.name
    assert manifest['application'] == {'name': 'GIMP', 'version': '3.2.6'}
    assert manifest['loader'] == 'file-psd-load-merged'
    assert manifest['source_psd_sha256'] == digest(source_folder / 'source.psd')
    assert set(manifest['files']) == {'merged.png', 'merged.icc'}
    for name, sha in manifest['files'].items():
        assert digest(merged_folder / name) == sha
    actual = manifest['merged']
    assert actual['canvas'] == decoded['canvas'] and actual['resolution'] == decoded['resolution']
    assert actual['precision'] == 'u8-non-linear'
    assert actual['color_mode'] == ('rgb' if decoded['color'] == 3 else 'gray')
    profile = (merged_folder / 'merged.icc').read_bytes()
    assert actual['profile'] == 'merged.icc' and actual['profile_sha256'] == digest(merged_folder / 'merged.icc')
    assert manifest['original_icc_sha256'] == hashlib.sha256(decoded['profile']).hexdigest()
    timestamp_changed = profile != decoded['profile']
    assert manifest['icc_creation_time_only_difference'] == timestamp_changed
    # A new GIMP process regenerates its builtin ICC creation date; its complete
    # remaining color payload must be identical. This exception applies only to
    # the external reference. PSD/native preservation remains byte-exact.
    if source_folder.name == 'rgb_p3_ppi':
        assert not timestamp_changed
    assert profile[:24] + profile[36:] == decoded['profile'][:24] + decoded['profile'][36:]
    assert len(actual['layers']) == 1
    layer = actual['layers'][0]
    assert layer['kind'] == 'raster' and layer['offset'] == [0, 0] and layer['size'] == decoded['canvas']
    assert layer['mask'] is None and layer['opacity'] == 1 and layer['visible'] is True
    assert layer['depth'] == 0 and GIMP_BLENDS[layer['blend']] == 0
    assert len(actual['saved_channels']) == len(decoded['auxiliary'])
    for recorded, plane in zip(actual['saved_channels'], decoded['auxiliary'], strict=True):
        assert recorded['size'] == decoded['canvas'] and bytes(recorded['coverage']) == plane
    samples = reference(merged_folder / 'merged.png', decoded['canvas'], profile)
    assert tuple(samples) == tuple(layer['rgba']), 'Merged reference PNG changed the reader samples'
    if source_folder.name in REFUSALS:
        return dict(appearance_qualified=False), samples
    comparison = appearance(decoded['composite'], samples)
    comparison['reference_icc_creation_time_changed_only'] = timestamp_changed
    return comparison, samples


def native_metadata(path, decoded):
    document = support.read_native(path)['document']
    assert document['resolution'] == tuple(decoded['resolution'])
    if decoded['profile'] is not None:
        assert document['profile'] == decoded['profile'], 'Embedded RGB ICC bytes changed'
    return dict(resolution=document['resolution'],
                profile_sha256=hashlib.sha256(document['profile']).hexdigest() if document['profile'] else None)


def verify_refusal(app, folder, output):
    output.mkdir()
    reason = REFUSALS[folder.name]
    source = folder / 'source.psd'
    preflight = app.call('inspect_psd', path=str(source))
    assert not preflight['layered_available'] and not preflight['stored_composite_available']
    assert reason in preflight['layered_refusal'] and reason in preflight['stored_composite_refusal']
    sentinel = output / 'sentinel.psd'
    sentinel.write_bytes((folder.parent / 'rgb_mask/source.psd').read_bytes())
    inspected = app.call('inspect_psd', path=str(sentinel))
    opened = app.call('open_document', path=str(sentinel), psd_mode='layers',
                      psd_fingerprint=inspected['source_fingerprint'])
    call = support.call_for(app, opened['document']['id'], output / 'sentinel-calls.jsonl')
    call('set_layer_properties', index=0, name='Edited refusal sentinel')
    call('select_rect', x=1, y=1, width=2, height=1)

    def capture(path):
        call('save_copy', path=str(path))
        info = call('get_document')
        assert info['unsaved_changes'] and pixels.history(call)[1] > 0
        return dict(native_sha256=digest(path), history=pixels.history(call),
                    dirty=info['unsaved_changes'], active_layer=info['active_layer'],
                    selected_layers=info['selected_layers'], selection=call('get_selection'),
                    document_ids=sorted(row['id'] for row in app.call('list_documents')['documents']))

    results = {}
    for mode in ('layers', 'composite'):
        before = capture(output / (mode + '-before.rz'))
        error = app.call('open_document', path=str(source), psd_mode=mode,
                         psd_fingerprint=preflight['source_fingerprint'], fail=True)
        assert reason in error
        after = capture(output / (mode + '-after.rz'))
        assert before == after, (mode, before, after)
        results[mode] = dict(error=error, before=before, after=after)
    return dict(preflight=preflight, refusal_atomicity=results, independent_gray_appearance_qualified=False)


def verify_case(app, folder, output, merged_folder):
    manifest, decoded, expected, independent = audit_source(folder)
    merged_comparison, merged_reference = audit_merged(folder, merged_folder, decoded)
    if folder.name in REFUSALS:
        return verify_refusal(app, folder, output)
    output.mkdir()
    source = folder / 'source.psd'
    preflight = app.call('inspect_psd', path=str(source))
    assert preflight['layered_available'] and preflight['stored_composite_available'], preflight
    assert preflight['requires_explicit_choice']
    if folder.name in LOSSES:
        assert len(preflight['verified_losses']) == 1
        assert LOSSES[folder.name] in preflight['verified_losses'][0]
    else:
        assert not preflight['verified_losses'], preflight
    assert preflight['resolution'] == dict(zip(('x', 'y'), decoded['resolution']))
    opened = app.call('open_document', path=str(source), psd_mode='layers',
                      psd_fingerprint=preflight['source_fingerprint'])
    assert opened['psd_import']['chosen_mode'] == 'layers'
    call = support.call_for(app, opened['document']['id'], output / 'calls.jsonl')
    original = output / 'original.rz'
    baseline = pixels.stored(call, original, expected)
    metadata = native_metadata(original, decoded)
    call('save_copy', path=str(output / 'imported.png'))
    actual = reference(output / 'imported.png', decoded['canvas'], decoded['profile'])
    comparisons = {name: appearance(actual, reference(folder / name, decoded['canvas'], decoded['profile']))
                   for name in ('authored.png', 'reopened.png')}

    before = pixels.history(call)
    index = len(expected['layers']) - 1
    call('set_layer_properties', index=index, name='10A qualification edit')
    assert pixels.history(call)[1] == before[1] + 1
    call('undo')
    assert pixels.stored(call, output / 'undone.rz', expected) == baseline
    assert pixels.history(call)[0] == before[0]
    call('redo')
    assert call('get_document')['layers'][index]['name'] == '10A qualification edit'
    call('undo')
    assert pixels.stored(call, output / 'restored.rz', expected) == baseline

    reopened = app.call('open_document', path=str(original))['document']['id']
    again = support.call_for(app, reopened, output / 'reopened-calls.jsonl')
    assert pixels.stored(again, output / 'reopened.rz', expected) == baseline
    assert native_metadata(output / 'reopened.rz', decoded) == metadata
    again('save_copy', path=str(output / 'reopened.png'))
    assert reference(output / 'reopened.png', decoded['canvas'], decoded['profile']) == actual
    mask_rows = masks.mask_workflow(app, original, expected, output)

    # Fresh URL proves the explicit composite choice independently of the open
    # layer document. Its actual stored samples have an exact, separate gate.
    copied = output / 'composite-choice.psd'
    copied.write_bytes(source.read_bytes())
    inspected = app.call('inspect_psd', path=str(copied))
    composite = app.call('open_document', path=str(copied), psd_mode='composite',
                         psd_fingerprint=inspected['source_fingerprint'])
    assert composite['psd_import']['chosen_mode'] == 'composite'
    flat = support.call_for(app, composite['document']['id'], output / 'composite-calls.jsonl')
    flat('save_copy', path=str(output / 'composite.png'))
    flat_samples = reference(output / 'composite.png', decoded['canvas'], decoded['profile'])
    flat_appearance = appearance(flat_samples, decoded['composite'])
    flat('save_copy', path=str(output / 'composite.rz'))
    assert native_metadata(output / 'composite.rz', decoded) == metadata
    native = support.read_native(output / 'composite.rz')['document']
    assert len(native['layers']) == 1
    raw = support.png_samples(native['fallbacks'][0])
    assert (raw['width'], raw['height'], raw['depth']) == (*decoded['canvas'], 8)
    assert bytes(raw['samples']) == decoded['composite'], 'Exact stored-composite source samples'
    stored_comparison = appearance(flat_samples, merged_reference)
    backdrops = backdrop_appearance(flat_samples, reference(folder / 'authored.png', decoded['canvas'], decoded['profile']))
    return dict(preflight=preflight, native_sha256=baseline, metadata=metadata,
                application_reopen_comparison=independent, layered_comparisons=comparisons,
                stored_composite_comparison=stored_comparison, mask_action_rows=mask_rows,
                stored_composite_render=flat_appearance,
                independent_merged_decode=merged_comparison, stored_composite_backdrops=backdrops,
                exact_inverse_of_white_matted_samples=decoded['merged_transparency'],
                raw_structure_mask_metadata_exact=True, native_roundtrip_exact=True,
                undo_redo_exact=True, explicit_saved_composite_exact=True)


def dependencies():
    """The code files `helper_sha256` hashes."""
    helpers = [Path(__file__), Path(psd.__file__), Path(masks.__file__), Path(pixels.__file__),
               ROOT / 'scripts/live-filter-probe/native.py', ROOT / 'scripts/raw-probe/native.py',
               ROOT / 'scripts/retouch-probe/support.py', ROOT / 'scripts/host-source-probe/support.py']
    helpers += sorted((ROOT / 'scripts/psd-exchange-probe').glob('*.py'))
    helpers += [ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
                ROOT / 'scripts/precision-probe/retained_fixtures.py',
                ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
                ROOT / 'scripts/retouch-probe/fixtures.py', ROOT / 'scripts/verification_isolation.py',
                ROOT / 'scripts/app_under_test.py',
                ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py',
                ROOT / 'scripts/verify_phase8b.py', *sorted((ROOT / 'core/src').rglob('*.rs'))]
    return helpers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'core/tests/fixtures/psd_10a')
    parser.add_argument('--merged-fixtures', type=Path, default=ROOT / 'core/tests/fixtures/psd_10a_merged')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--oracle-only', action='store_true')
    args = parser.parse_args()
    cases = sorted(path.parent for path in args.fixtures.glob('*/manifest.json'))
    assert {path.name for path in cases} == CASES, 'Missing/extra independent application cases'
    merged_cases = {path.parent.name: path.parent for path in args.merged_fixtures.glob('*/manifest.json')}
    assert set(merged_cases) == CASES, 'Missing/extra merged-reader references'
    hashes = {str(path): digest(path) for folder in [*cases, *merged_cases.values()]
              for path in folder.iterdir() if path.is_file()}
    audits = audit_corpus(cases)
    for folder in cases:
        decoded = psd.decode(folder / 'source.psd')
        audits[folder.name]['merged_reader'] = audit_merged(folder, merged_cases[folder.name], decoded)[0]
    if args.oracle_only:
        print(json.dumps(dict(cases=audits, fixture_hashes=hashes), indent=2))
        return
    if args.app is None or args.output is None:
        parser.error('--app and --output are required for production verification')
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error('output must be new and outside the repository')
    output.mkdir(exist_ok=False)
    executable = (args.app / 'Contents/MacOS/Doctored' if args.app.suffix == '.app' else args.app).resolve()
    helpers = dependencies()
    result = dict(app=str(executable), app_sha256=digest(executable), fixture_sha256=hashes,
                  helper_sha256={str(path): digest(path) for path in helpers}, cases={})
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        for folder in cases:
            result['cases'][folder.name] = verify_case(app, folder, output / folder.name, merged_cases[folder.name])
    assert all(digest(Path(path)) == sha for path, sha in hashes.items())
    assert all(digest(Path(path)) == sha for path, sha in result['helper_sha256'].items())
    assert digest(executable) == result['app_sha256']
    result['fixtures_unchanged'] = True
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), cases=len(cases)), indent=2))


if __name__ == '__main__':
    main()
