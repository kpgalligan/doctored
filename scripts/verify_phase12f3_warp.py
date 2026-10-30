#!/usr/bin/env python3
"""Owned MCP qualification for the persisted warp's native state and Actions."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))
from pixels import png_rgba, write_png8, write_png16
from support import HostApp

WIDTH, HEIGHT = 36, 28
PNG = b'\x89PNG\r\n\x1a\n'


def identity_word(depth):
    """The outermost `.rz` word the core writes today at `depth`
    (`identity_native::BYTE_VERSION` / `PRECISION_VERSION`)."""
    name = 'BYTE_VERSION' if depth == 8 else 'PRECISION_VERSION'
    source = (ROOT / 'core/src/identity_native.rs').read_text()
    found = re.findall(rf'pub const {name}: u32 = (\d+);', source)
    assert len(found) == 1, (name, found)
    return int(found[0])


def fixture_samples(depth):
    scale = 1 if depth == 8 else 257
    samples = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            alpha = [0, 1, 71, 179, 255][(x * 3 + y * 5) % 5]
            row = [(x * 13 + y * 7 + 241) % 256,
                   (x * 3 + y * 19 + 199) % 256,
                   (x * 17 + y * 11 + 113) % 256, alpha]
            if depth == 16:
                row = [(v * scale + (x * 17 + y * 11 + c) % 257) % 65536
                       for c, v in enumerate(row[:3])] + [
                           [0, 1, 179, 8191, 32769, 65535][(x * 3 + y * 5) % 6]]
            samples.extend(row)
    return samples


def request(columns=3, rows=3, filter='bicubic'):
    return dict(operation='set', target='pixels', columns=columns, rows=rows,
                displacements=[[0.35 + 0.45 * x / (columns - 1) - 0.19 * y / (rows - 1),
                                -0.27 + 0.21 * y / (rows - 1) + 0.11 * x / (columns - 1)]
                               for y in range(rows) for x in range(columns)], filter=filter)


def pngs(native):
    results = []
    at = 0
    while (at := native.find(PNG, at)) >= 0:
        cursor = at + len(PNG)
        while True:
            length = struct.unpack_from('>I', native, cursor)[0]
            kind = native[cursor + 4:cursor + 8]
            cursor += 12 + length
            if kind == b'IEND':
                results.append(png_rgba(native[at:cursor]))
                at = cursor
                break
    return results


def warp_records(native, depth):
    """Decode the WARP table's original planes independently of rendered PNGs."""
    at = native.index(b'WARP') + 4
    schema, count = struct.unpack_from('<II', native, at)
    assert schema == 1 and count > 0, (schema, count)
    at += 8
    result = []
    for _ in range(count):
        index, target, filter_code, linked, outside, columns, rows = struct.unpack_from(
            '<IBBBBII', native, at)
        at += 16
        values = [list(struct.unpack_from('<dd', native, at + n * 16))
                  for n in range(columns * rows)]
        at += columns * rows * 16
        planes = []
        for channels in (4, 1):
            present = native[at]
            at += 1
            if not present:
                planes.append(None)
                continue
            x, y, width, height, length = struct.unpack_from('<iiIIQ', native, at)
            at += 24
            assert length == width * height * channels * (depth // 8), (length, width, height)
            raw = native[at:at + length]
            assert len(raw) == length
            at += length
            samples = list(raw) if depth == 8 else list(struct.unpack('<' + 'H' * (length // 2), raw))
            planes.append({'offset': [x, y], 'size': [width, height], 'samples': samples})
        result.append({'layer': index, 'target': target, 'filter': filter_code,
                       'linked': linked, 'outside': outside, 'columns': columns,
                       'rows': rows, 'values': values, 'pixels': planes[0], 'mask': planes[1]})
    return result


def native(app, document, folder, label):
    path = folder / (label + '.rz')
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    encoded = path.read_bytes()
    images = pngs(encoded)
    assert len(images) == 1, (label, len(images))
    return encoded, images[0]


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def refusal(app, tool_name, **arguments):
    reply = app.client.rpc('tools/call', {'name': tool_name, 'arguments': arguments})
    assert reply.get('isError'), (tool_name, reply)
    return ' '.join(item.get('text', '') for item in reply.get('content', []))


def open_fixture(app, folder, depth, label):
    path = folder / f'{label}-{depth}.png'
    samples = fixture_samples(depth)
    if depth == 8:
        write_png8(path, WIDTH, HEIGHT, bytes(samples))
    else:
        write_png16(path, WIDTH, HEIGHT, samples)
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        precision = app.call('get_precision', document_id=document)
        assert '16' in json.dumps(precision) or 'rgba16' in json.dumps(precision), precision
    return document, path


def base_gate(app, folder, depth):
    doc, fixture = open_fixture(app, folder, depth, 'base')
    original, original_pixels = native(app, doc, folder, f'original-{depth}')
    assert original_pixels['depth'] == depth, original_pixels['depth']
    initial = cursor(app, doc)
    field = request()
    result = app.call('warp_layer', document_id=doc, **field)
    assert result['ok'] and result['changed'] and result['field']['columns'] == 3, result
    assert cursor(app, doc) == initial + 1
    first, pixels = native(app, doc, folder, f'set-{depth}')
    raw = warp_records(first, depth)
    assert len(raw) == 1 and raw[0]['target'] == 0 and raw[0]['pixels'] is not None
    assert raw[0]['pixels']['size'] == [WIDTH, HEIGHT]
    assert raw[0]['pixels']['samples'] == fixture_samples(depth), 'WARP original codes changed'
    assert first[:4] in (b'RZDC', b'DRDC') and first[4:8] == struct.pack('<I', identity_word(depth))
    assert first[8:12] == struct.pack('<I', 53 if depth == 8 else 54)
    assert b'WARP' in first and first.endswith(b'LIDS' + first.split(b'LIDS')[-1])
    assert pixels['samples'] != original_pixels['samples']
    dirty_after_set = app.call('get_document', document_id=doc)['unsaved_changes']
    no_op = app.call('warp_layer', document_id=doc, **field)
    assert no_op['ok'] and not no_op['changed'] and cursor(app, doc) == initial + 1, no_op
    assert app.call('get_document', document_id=doc)['unsaved_changes'] == dirty_after_set
    app.call('undo', document_id=doc)
    undo, before_pixels = native(app, doc, folder, f'undo-{depth}')
    assert before_pixels['samples'] == original_pixels['samples'] and undo == original
    app.call('redo', document_id=doc)
    redo, after_pixels = native(app, doc, folder, f'redo-{depth}')
    assert redo == first and after_pixels['samples'] == pixels['samples']
    copy = folder / f'reopened-{depth}.rz'
    reopened = app.call('open_document', path=str(folder / f'set-{depth}.rz'))['document']['id']
    app.call('save_copy', document_id=reopened, path=str(copy), format='rz')
    assert copy.read_bytes() == first
    assert app.call('get_warp', document_id=reopened)['field'] == result['field']
    changed = request(4, 2, 'bilinear')
    reedit = app.call('warp_layer', document_id=reopened, **changed)
    assert reedit['changed'] and reedit['field']['columns'] == 4
    app.call('warp_layer', document_id=doc, operation='clear')
    cleared, cleared_pixels = native(app, doc, folder, f'clear-{depth}')
    assert cleared == original and cleared_pixels['samples'] == original_pixels['samples']
    app.call('warp_layer', document_id=doc, **field)
    app.call('warp_layer', document_id=doc, operation='bake')
    baked, baked_pixels = native(app, doc, folder, f'bake-{depth}')
    assert b'WARP' not in baked and baked_pixels['samples'] == pixels['samples']
    return {'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(),
            'native_sha256': hashlib.sha256(first).hexdigest(),
            'depth': depth, 'history_delta': 1,
            'native_codes': len(pixels['samples']), 'raw_original_codes':
            len(raw[0]['pixels']['samples']), 'reopen_identical': True}


def refusal_gate(app, folder):
    document, _ = open_fixture(app, folder, 8, 'refusal')
    prior = cursor(app, document)
    prior_dirty = app.call('get_document', document_id=document)['unsaved_changes']
    for label, args, part in [
        ('missing', {'operation': 'set'}, 'needs'),
        ('unknown', {'operation': 'clear', 'filter': 'bicubic'}, 'clear/bake'),
        ('invalid', {**request(), 'columns': 17}, 'lattice'),
        ('fold', {**request(2, 2), 'displacements': [[0, 0], [20, 0], [0, 0], [20, 0]]}, 'contraction')]:
        message = refusal(app, 'warp_layer', document_id=document, **args)
        assert part in message.lower(), (label, message)
        assert cursor(app, document) == prior, label
        assert app.call('get_document', document_id=document)['unsaved_changes'] == prior_dirty, label
    return {'unchanged_history_and_dirty': True}


def replay_gate(app, folder, depth):
    source, _ = open_fixture(app, folder, depth, 'record')
    twin, _ = open_fixture(app, folder, depth, 'twin')
    app.call('start_recording', discard=True)
    app.call('warp_layer', document_id=source, **request())
    app.call('warp_layer', document_id=source, **request(4, 2, 'bilinear'))
    saved = app.call('stop_recording', name=f'12f3-warp-{depth}')
    assert saved['saved'] and len(saved['steps']) == 2, saved
    assert all(step['tool'] == 'warp_layer' for step in saved['steps']), saved
    assert '__unrecorded' not in json.dumps(saved['steps']), saved
    source_native, source_pixels = native(app, source, folder, f'recorded-{depth}')
    replay = app.call('run_action', document_id=twin, name=f'12f3-warp-{depth}')
    assert replay.get('failed', 0) == 0, replay
    twin_native, twin_pixels = native(app, twin, folder, f'replayed-{depth}')
    assert twin_pixels['samples'] == source_pixels['samples']
    source_field = app.call('get_warp', document_id=source)['field']
    twin_field = app.call('get_warp', document_id=twin)['field']
    assert twin_field == source_field, (saved['steps'], source_field, twin_field)
    assert b'WARP' in twin_native and b'WARP' in source_native
    before = cursor(app, twin)
    step = {'tool': 'warp_layer', 'arguments': {**request(4, 2, 'bilinear'),
            'layer': {'$layer': 'active'}}}
    app.call('save_action', name=f'12f3-noop-{depth}', overwrite=True, steps=[
        step, {'tool': 'set_layer_label', 'arguments': {'layer': {'$layer': 'active'},
                                                     'label': 'red'}}])
    continued = app.call('run_action', document_id=twin, name=f'12f3-noop-{depth}')
    assert continued.get('failed', 0) == 0 and cursor(app, twin) == before + 1, continued
    invalid = refusal(app, 'save_action', name=f'12f3-relative-{depth}', overwrite=True,
                      steps=[{**step, 'geometry': {'mode': 'relative'}}])
    assert 'absolute' in invalid.lower() and 'warp_layer' in invalid, invalid
    return {'steps': len(saved['steps']), 'exact_native_replay': True,
            'no_op_continues': True, 'absolute_only': invalid}


def mask_gate(app, folder, depth):
    document, _ = open_fixture(app, folder, depth, 'masks')
    app.call('add_layer_mask', document_id=document, layer=0, kind='reveal_all')
    linked = app.call('warp_layer', document_id=document, **request())
    assert linked['field']['linked_mask'] is True, linked
    linked_native, _ = native(app, document, folder, f'linked-mask-{depth}')
    linked_raw = warp_records(linked_native, depth)[0]
    assert linked_raw['mask'] and linked_raw['linked'] == 1
    assert all(value == (255 if depth == 8 else 65535)
               for value in linked_raw['mask']['samples'])
    app.call('warp_layer', document_id=document, operation='clear')
    app.call('set_mask_properties', document_id=document, layer=0,
             linked=False, density=0.625, feather=1.25)
    before = app.call('get_mask_properties', document_id=document, layer=0)
    unlinked = app.call('warp_layer', document_id=document, **request())
    assert unlinked['field']['linked_mask'] is False
    after = app.call('get_mask_properties', document_id=document, layer=0)
    assert after == before, (before, after)
    app.call('warp_layer', document_id=document, operation='clear')
    mask_only = app.call('warp_layer', document_id=document,
                         **{**request(), 'target': 'mask'})
    assert mask_only['changed'] and mask_only['field']['target'] == 'mask'
    saved, pixels = native(app, document, folder, f'mask-only-{depth}')
    assert b'WARP' in saved and pixels['depth'] == depth
    mask_raw = warp_records(saved, depth)[0]
    assert mask_raw['pixels'] is None and mask_raw['mask'] is not None
    assert all(value == (255 if depth == 8 else 65535)
               for value in mask_raw['mask']['samples'])
    switch_history = cursor(app, document)
    switch_dirty = app.call('get_document', document_id=document)['unsaved_changes']
    switch = refusal(app, 'warp_layer', document_id=document, **request())
    assert 'clear or bake' in switch.lower(), switch
    assert cursor(app, document) == switch_history
    assert app.call('get_document', document_id=document)['unsaved_changes'] == switch_dirty
    app.call('warp_layer', document_id=document, operation='clear')
    return {'linked': True, 'raw_original_masks_decoded': True,
            'unlinked_exact_properties': True,
            'mask_only_reopenable': True, 'target_switch_refusal': switch}


def preservation_gate(app, folder, depth):
    document, _ = open_fixture(app, folder, depth, 'preserve')
    app.call('warp_layer', document_id=document, **request())
    report = app.call('inspect_psd_export', document_id=document, mode='layers')
    assert any('persisted warp field' in loss for loss in report.get('losses', [])), report
    save = app.call('save_copy', document_id=document,
                    path=str(folder / f'warp-flat-{depth}.png'), format='png')
    if depth == 8:
        assert any('persisted warp field' in loss for loss in save.get('flat_save_losses', [])), save
    return {'psd_loss': True, 'flat_loss': depth == 8}


def lock_source_float_gate(app, folder):
    document, _ = open_fixture(app, folder, 8, 'locks')
    app.call('set_layer_lock', document_id=document, layer=0, locks=['position'])
    prior = cursor(app, document)
    dirty_before = app.call('get_document', document_id=document)['unsaved_changes']
    lock = refusal(app, 'warp_layer', document_id=document, **request())
    assert 'lock' in lock.lower() and cursor(app, document) == prior, lock
    assert app.call('get_document', document_id=document)['unsaved_changes'] == dirty_before
    master = folder / 'placed.png'
    write_png8(master, WIDTH, HEIGHT, bytes(fixture_samples(8)))
    doc = app.call('new_document', width=WIDTH, height=HEIGHT, depth=8,
                   background='white')['document']['id']
    placed = app.call('place_image', document_id=doc, path=str(master), x=0, y=0,
                      name='Master')
    # A placed PNG is an ordinary raster layer: it keeps no source, so the
    # warp is admitted as one step and keeps its field.
    assert placed['source'] is None, placed
    prior = cursor(app, doc)
    on_placed = app.call('warp_layer', document_id=doc, layer=placed['layer'], **request())
    assert on_placed['ok'] and on_placed['changed'] and on_placed['field']['columns'] == 3, on_placed
    assert cursor(app, doc) == prior + 1
    # A float warp is admitted. It is applied at once as one undo step
    # and keeps no re-editable field, so get_warp answers null and undo
    # restores the composite.
    float_fixture = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
    float_doc = app.call('open_document', path=str(float_fixture))['document']['id']
    before = cursor(app, float_doc)
    original = composite_png(app, float_doc)
    warped = app.call('warp_layer', document_id=float_doc, **request())
    assert warped['ok'] and warped['changed'] and warped['field'] is None, warped
    assert cursor(app, float_doc) == before + 1
    assert app.call('get_warp', document_id=float_doc)['field'] is None
    assert composite_png(app, float_doc) != original
    app.call('undo', document_id=float_doc)
    assert cursor(app, float_doc) == before
    assert composite_png(app, float_doc) == original
    return {'lock': lock, 'placed_raster_layer': {'source': None, 'history_delta': 1},
            'float': {'history_delta': 1, 'field': None, 'undo_restores': True}}


def composite_png(app, document):
    """The `render` tool's PNG of the flattened canvas, as its base64 text."""
    reply = app.client.rpc('tools/call', {'name': 'render',
                                          'arguments': {'document_id': document}})
    assert not reply.get('isError'), reply
    images = [item.get('data') for item in reply.get('content', [])
              if item.get('type') == 'image']
    assert images, reply
    return images


def catalog_gate(app):
    # A check only: the generator's default output is cwd-relative, so it
    # runs from the repository root, and the drive never rewrites the page.
    command = [sys.executable, str(ROOT / 'scripts/generate_mcp_reference.py'),
               '--socket', app.client.socket_path]
    result = subprocess.run([*command, '--check'], cwd=ROOT,
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr
    return {'checked_from_owned_server': True}


def dependencies():
    """The code files `dependency_sha256` hashes: the script's whole load chain."""
    return [Path(__file__),
            ROOT / 'scripts/host-source-probe/support.py', ROOT / 'scripts/phase5_support.py',
            ROOT / 'scripts/pixels.py', ROOT / 'scripts/precision-probe/fixtures.py',
            ROOT / 'scripts/precision-probe/retained_fixtures.py',
            ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
            ROOT / 'scripts/verification_isolation.py',
            ROOT / 'scripts/app_under_test.py', ROOT / 'scripts/verify_phase1.py',
            ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
            ROOT / 'core/src/identity_native.rs']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts)
    folder.mkdir(parents=True, exist_ok=True)
    receipt = {'app_sha256': hashlib.sha256(Path(args.app).read_bytes()).hexdigest(),
               'helper_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'dependency_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in dependencies()}}
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(Path(args.app).resolve())) as app:
            for depth in (8, 16):
                receipt[f'base_{depth}'] = base_gate(app, folder, depth)
                print('PASS', f'base_{depth}', flush=True)
                receipt[f'masks_{depth}'] = mask_gate(app, folder, depth)
                print('PASS', f'masks_{depth}', flush=True)
                receipt[f'replay_{depth}'] = replay_gate(app, folder, depth)
                print('PASS', f'replay_{depth}', flush=True)
                receipt[f'preservation_{depth}'] = preservation_gate(app, folder, depth)
                print('PASS', f'preservation_{depth}', flush=True)
            receipt['refusals'] = refusal_gate(app, folder)
            print('PASS refusals', flush=True)
            receipt['lock_source_float'] = lock_source_float_gate(app, folder)
            print('PASS lock_source_float', flush=True)
            receipt['catalog'] = catalog_gate(app)
            print('PASS catalog', flush=True)
    except BaseException as error:
        receipt['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipt, indent=2, default=str))
        raise
    (folder / 'report.json').write_text(json.dumps(receipt, indent=2, default=str))
    print('ALL GATES PASS', len(receipt) - 2, flush=True)


if __name__ == '__main__':
    main()
