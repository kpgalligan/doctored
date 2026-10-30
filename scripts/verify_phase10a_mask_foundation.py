#!/usr/bin/env python3
"""Verify independent byte/native outside-mask fixtures through an owned Release app."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


support = load('outside_support', 'scripts/retouch-probe/support.py')
fixtures = load('outside_fixtures', 'scripts/mask-probe/fixtures.py')
decoder = load('outside_decoder', 'scripts/live-filter-probe/native.py')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def nested_words(data):
    """The first two words inside the identity wrapper (and a typed file's
    optional word 55), through the shared peel rather than a fixed offset."""
    return struct.unpack_from('<II', decoder._identity_body(data)[0], 4)


def check_decoder():
    def table(indexes, *, schema=1, count=None):
        return b'MOUT' + struct.pack('<II', schema, len(indexes) if count is None else count) + b''.join(struct.pack('<I', index) for index in indexes)

    def layers():
        return {0: dict(mask=dict(outside_coverage=0)), 1: dict(mask=None),
                2: dict(mask=dict(outside_coverage=0))}

    result = layers()
    assert decoder.read_mask_outside(decoder.Reader(table([0, 2])), result) == [0, 2]
    assert result[0]['mask']['outside_coverage'] == result[2]['mask']['outside_coverage'] == 255
    bad = [table([]), table([0], schema=2), table([0], count=4), table([1]), table([3]),
           table([0, 0]), table([2, 0]), table([0, 1]), table([0, 3]), table([0])[:-1]]
    for data in bad:
        result = layers()
        try:
            decoder.read_mask_outside(decoder.Reader(data), result)
        except AssertionError:
            pass
        else:
            raise AssertionError('Malformed MOUT was accepted')
        assert result == layers(), 'Refusal partially changed independent mask properties'
    # A typed file with a closed group carries word 55 and a LOPN table
    # inside the identity wrapper; the nested MOUT/blend words read the same.
    closed = (b'RZDC' + struct.pack('<IIII', 46, 55, 29, 26) + b'body'
              + b'LOPN' + struct.pack('<III', 1, 1, 0) + b'LIDS' + struct.pack('<IIQ', 1, 1, 7))
    assert nested_words(closed) == (29, 26), nested_words(closed)
    return len(bad)


def stored(call, path, expected, *, density=None, translate=0):
    call('save_copy', path=str(path))
    value = support.read_native(path)
    doc, mask = value['document'], expected['mask']
    depth = expected['depth']
    assert (doc['width'], doc['height'], doc['depth']) == (*expected['canvas'], depth)
    # 28/29 are mask_outside_native::BYTE_VERSION/PRECISION_VERSION (the
    # conditional MOUT tag); 10 is drdc::LEGACY_DRDC_VERSION and 13 the
    # source-free typed body (precision_native.rs). Fixed tags under the identity
    # wrapper `read_native` peels, never the writer version (45/46).
    assert value['version'] == ((28 if depth == 8 else 29) if mask['outside_coverage'] else (10 if depth == 8 else 13))
    assert len(doc['layers']) == (2 if expected['group'] else 1)
    source = support.png_samples(doc['fallbacks'][0])
    assert (source['width'], source['height'], source['depth']) == (*expected['canvas'], depth)
    assert list(source['samples']) == expected['rgba']
    assert doc['profile'] == fixtures.PROFILE.read_bytes()
    actual = doc['layers'][int(expected['group'])]['mask']
    for key in ('width', 'height', 'enabled', 'linked', 'feather', 'outside_coverage'):
        assert actual[key] == mask[key], (key, actual, mask)
    assert (actual['x'], actual['y']) == (mask['x'] + translate, mask['y'])
    assert actual['density'] == (mask['density'] if density is None else density)
    assert actual['data'] == fixtures.mask_bytes(mask, depth)
    return digest(path)


def rendered(call, path, expected, **changes):
    call('save_copy', path=str(path))
    value = support.png_samples(path.read_bytes())
    assert (value['width'], value['height'], value['depth']) == (*expected['canvas'], expected['depth'])
    want = fixtures.composite(expected, **changes)
    delta = max(abs(a-b) for a, b in zip(value['samples'], want, strict=True))
    assert delta <= (1 if expected['depth'] == 16 else 0), (path, delta, list(value['samples']), want)
    return delta


def history(call):
    value = call('list_history')
    return value['current_state_id'], value['undo_count'], value['redo_count']


def exercise(app, path, expected, folder):
    folder.mkdir()
    identifier = app.call('open_document', path=str(path), precision='preserve')['document']['id']
    call = support.call_for(app, identifier, folder / 'calls.jsonl')
    index = int(expected['group'])
    properties = call('get_mask_properties', layer=index)
    assert properties['outside_coverage'] == expected['mask']['outside_coverage'], properties
    original = folder / 'original.rz'
    sha = stored(call, original, expected)
    error = rendered(call, folder / 'original.png', expected)
    before = history(call)
    app.call('start_recording')
    call('set_mask_properties', layer=index, density=.5)
    action_name = 'Outside mask ' + path.stem
    action = app.call('stop_recording', name=action_name)
    assert [step['tool'] for step in action['steps']] == ['set_mask_properties']
    assert history(call)[1] == before[1] + 1
    changed = stored(call, folder / 'density.rz', expected, density=.5)
    error = max(error, rendered(call, folder / 'density.png', expected, density=.5))
    call('undo')
    assert history(call)[0] == before[0] and stored(call, folder / 'undone.rz', expected) == sha
    call('redo')
    assert stored(call, folder / 'redone.rz', expected, density=.5) == changed
    call('undo')
    assert call('run_action', name=action_name)['ok']
    assert stored(call, folder / 'replayed.rz', expected, density=.5) == changed
    call('undo')
    call('transform_mask', layer=index, matrix=[1, 0, 0, 1, 1, 0], filter='nearest')
    assert history(call)[1] == before[1] + 1
    stored(call, folder / 'translated.rz', expected, translate=1)
    error = max(error, rendered(call, folder / 'translated.png', expected, translate=1))
    call('undo')
    assert stored(call, folder / 'translation-undone.rz', expected) == sha
    reopened_id = app.call('open_document', path=str(original), precision='preserve')['document']['id']
    reopened = support.call_for(app, reopened_id, folder / 'reopened-calls.jsonl')
    assert stored(reopened, folder / 'reopened.rz', expected) == sha
    error = max(error, rendered(reopened, folder / 'reopened.png', expected))
    nested = []
    if expected['depth'] == 16 and expected['mask']['outside_coverage']:
        for name, mode, inner in [('Multiply', 1, 26), ('Overlay', 3, 27)]:
            call('set_layer_properties', index=index, blend_mode=name)
            path = folder / ('nested-' + name + '.rz')
            nested_sha = stored(call, path, expected)
            # 29 is mask_outside_native::PRECISION_VERSION (a fixed MOUT tag),
            # the first word inside the writer-tracking identity wrapper.
            assert nested_words(path.read_bytes()) == (29, inner)
            assert support.read_native(path)['document']['layers'][index]['blend'] == mode
            nested_id = app.call('open_document', path=str(path), precision='preserve')['document']['id']
            again = support.call_for(app, nested_id, folder / ('nested-' + name + '-calls.jsonl'))
            saved = folder / ('nested-' + name + '-reopened.rz')
            assert stored(again, saved, expected) == nested_sha
            assert support.read_native(saved)['document']['layers'][index]['blend'] == mode
            call('undo')
            assert stored(call, folder / ('nested-' + name + '-undone.rz'), expected) == sha
            nested.append(inner)
    return dict(native_sha256=sha, maximum_code_error=error, exact_raw_and_mask=True,
                outside_reported=True, density_action_undo_redo=True, mask_translation_undo=True,
                native_reopen_exact=True, nested_blend_versions=nested)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--fixtures-only', action='store_true')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error('output must be new and outside the repository')
    if not args.fixtures_only and args.app is None:
        parser.error('--app is required unless --fixtures-only is selected')
    output.mkdir(exist_ok=False)
    cases = fixtures.write(output / 'fixtures')
    hashes = {str(path): digest(path) for path in (output / 'fixtures').iterdir()}
    report = dict(fixture_sha256=hashes, malformed_decoder_cases=check_decoder(), cases={})
    (output / 'expectations.json').write_text(json.dumps({key: value[1] for key, value in cases.items()}, indent=2) + '\n')
    if not args.fixtures_only:
        executable = (args.app / 'Contents/MacOS/Doctored' if args.app.suffix == '.app' else args.app).resolve()
        report.update(app=str(executable), app_sha256=digest(executable))
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            for name, (path, expected) in cases.items():
                report['cases'][name] = exercise(app, path, expected, output / name)
    assert all(digest(Path(path)) == sha for path, sha in hashes.items())
    report['fixtures_unchanged'] = True
    (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), checked_cases=len(report['cases']),
                          malformed_decoder_cases=report['malformed_decoder_cases']), indent=2))


if __name__ == '__main__':
    main()
