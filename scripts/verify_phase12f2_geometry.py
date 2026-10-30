#!/usr/bin/env python3
"""Owned-app Phase 12F-2 set, persistence, replay and refusal smoke qualification.

The source PNG and every expected history/render equality are independent of
the app. A failure exits nonzero and leaves a JSON receipt in --receipts.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8
from support import HostApp
from verify_phase11f_float import read_float_tiff

FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

WIDTH, HEIGHT = 36, 28
AFFINE = [1.0, 0.0, 0.0, 1.0, 3.0, -2.0]
RECT = [0.0, 0.0, float(WIDTH), float(HEIGHT)]
QUAD = [[2.0, 1.0], [34.0, 2.0], [32.0, 27.0], [3.0, 25.0]]


def pixels():
    out = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            out += bytes(((x * 13 + y * 7) % 256, (x * 3 + y * 19) % 256,
                          (x * 17 + y * 11) % 256, 255 if (x + y) % 5 else 173))
    return bytes(out)


def render(app, document):
    reply = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document}})
    assert not reply.get('isError'), reply
    encoded = next(row['data'] for row in reply['content'] if row['type'] == 'image')
    decoded = png_rgba(base64.b64decode(encoded))
    return (decoded['width'], decoded['height'], decoded['depth'], decoded['samples'])


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def refusal(app, name, **arguments):
    reply = app.client.rpc('tools/call', {'name': name, 'arguments': arguments})
    assert reply.get('isError'), (name, reply)
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def fixture(app, folder, label, depth):
    image = folder / (label + '.png')
    write_png8(image, WIDTH, HEIGHT, pixels())
    document = app.call('open_document', path=str(image))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', flatten=True)
    app.call('duplicate_layer', document_id=document, index=0)
    app.call('add_layer_mask', document_id=document, layer=0, kind='reveal_all')
    app.call('group_layers', document_id=document, layers=[0, 1], name='Set')
    rows = app.call('get_document', document_id=document)['layers']
    group = next(i for i, row in enumerate(rows) if row['name'] == 'Set')
    return document, group


def group_gate(app, folder, depth, perspective):
    label = f'{depth}_{"perspective" if perspective else "affine"}'
    document, group = fixture(app, folder, label, depth)
    before = render(app, document)
    history = cursor(app, document)
    arguments = ({'source_rect': RECT, 'corners': QUAD} if perspective
                 else {'matrix': AFFINE})
    reply = app.call('transform_layers', document_id=document,
                     layers=[group], sampler='nearest', **arguments)
    assert reply['ok'] and reply['changed'], reply
    assert sorted(reply['layers']) == list(range(len(app.call(
        'get_document', document_id=document)['layers']))), reply
    assert reply['rasterized_layers'] == [], reply
    after = render(app, document)
    assert after != before, (label, 'set geometry left the render unchanged')
    assert cursor(app, document) == history + 1, (label, 'not one history step')
    app.call('undo', document_id=document)
    assert render(app, document) == before, (label, 'undo')
    app.call('redo', document_id=document)
    assert render(app, document) == after, (label, 'redo')
    original = folder / (label + '-a.rz')
    second = folder / (label + '-b.rz')
    app.call('save_copy', document_id=document, path=str(original))
    reopened = app.call('open_document', path=str(original))['document']['id']
    assert render(app, reopened) == after, (label, 'reopen')
    app.call('save_copy', document_id=reopened, path=str(second))
    first_hash = hashlib.sha256(original.read_bytes()).hexdigest()
    second_hash = hashlib.sha256(second.read_bytes()).hexdigest()
    assert first_hash == second_hash, (label, 'byte-identical resave')
    return {'expanded': reply['layers'], 'group': group, 'sha256': first_hash,
            'history_delta': 1, 'reopen': True}


def atomic_refusals(app, folder):
    document, group = fixture(app, folder, 'refusals', 8)
    before = render(app, document)
    history = cursor(app, document)
    duplicate = refusal(app, 'transform_layers', document_id=document,
                        layers=[group, group], matrix=AFFINE)
    assert 'twice' in duplicate.lower() or 'duplicate' in duplicate.lower(), duplicate
    invalid = refusal(app, 'transform_layers', document_id=document,
                      layers=[group, 99999], matrix=AFFINE)
    assert 'layer' in invalid.lower() or 'index' in invalid.lower(), invalid
    app.call('set_layer_lock', document_id=document, layer=0, locks=['position'])
    locked = refusal(app, 'transform_layers', document_id=document,
                     layers=[group], matrix=AFFINE)
    assert 'lock' in locked.lower(), locked
    assert render(app, document) == before and cursor(app, document) == history + 1, \
        'a refused set changed pixels or history beyond the lock-setting step'
    return {'duplicate': duplicate, 'invalid': invalid, 'locked': locked}


def replay_gate(app, folder):
    source, group = fixture(app, folder, 'record', 8)
    twin, twin_group = fixture(app, folder, 'replay', 8)
    assert group == twin_group
    before = render(app, twin)
    app.call('start_recording', discard=True)
    app.call('transform_layers', document_id=source, layers=[group], matrix=AFFINE,
             sampler='nearest')
    saved = app.call('stop_recording', name='12f2-transform-layers')
    assert saved['saved'] and len(saved['steps']) == 1, saved
    assert saved['steps'][0]['tool'] == 'transform_layers', saved['steps']
    assert '__unrecorded' not in json.dumps(saved['steps']), saved['steps']
    report = app.call('run_action', document_id=twin, name='12f2-transform-layers')
    assert report.get('failed', 0) == 0, report
    assert render(app, twin) == render(app, source), 'recorded set replay differs'
    app.call('undo', document_id=twin)
    assert render(app, twin) == before, 'replayed set was not one undo'
    return {'steps': saved['steps'], 'replay': report}


def artwork_gate(app, folder, depth):
    document, _ = fixture(app, folder, f'artwork_{depth}', depth)
    app.call('add_text_layer', document_id=document, text='Geometry', x=4, y=7,
             size=12)
    rows = app.call('get_document', document_id=document)['layers']
    described = [row['index'] for row in rows if 'text' in row]
    assert len(described) == 1, rows
    index = described[0]
    affine = app.call('transform_layers', document_id=document, layers=[index],
                      matrix=[0.9, 0, 0, 0.9, 1, 1], sampler='bilinear')
    assert affine['changed'] and affine['rasterized_layers'] == [], affine
    assert any('text' in row for row in app.call('get_document',
        document_id=document)['layers']), 'affine lost editable text'
    before = render(app, document)
    history = cursor(app, document)
    refused = refusal(app, 'transform_layers', document_id=document,
                      layers=[index], source_rect=RECT, corners=QUAD,
                      sampler='bilinear')
    assert 'rasterize_artwork' in refused, refused
    assert render(app, document) == before and cursor(app, document) == history
    warped = app.call('transform_layers', document_id=document, layers=[index],
                      source_rect=RECT, corners=QUAD, sampler='bilinear',
                      rasterize_artwork=True)
    assert warped['changed'] and warped['rasterized_layers'] == [index], warped
    assert not any('text' in row for row in app.call('get_document',
        document_id=document)['layers']), 'authorized perspective kept stale text'
    app.call('undo', document_id=document)
    assert render(app, document) == before
    assert any('text' in row for row in app.call('get_document',
        document_id=document)['layers']), 'Undo did not restore editable text'
    return {'layer': index, 'affine_editable': True,
            'perspective_refusal': refused, 'authorized_loss': warped['rasterized_layers'],
            'undo_restored': True}


def source_gate(app, folder, depth):
    """A placed PNG is an ordinary raster layer: a projective set transform
    resamples its pixels as one step, nothing of the file is bound before or
    after, and the result reopens and survives crop / rotate plus undo."""
    master = folder / f'placed_{depth}.png'
    write_png8(master, WIDTH, HEIGHT, pixels())
    document = app.call('new_document', width=WIDTH, height=HEIGHT, depth=depth,
                        background='white')['document']['id']
    placed = app.call('place_image', document_id=document, path=str(master),
                      x=0, y=0, name='Original')
    index = placed['layer']
    assert placed['source'] is None, placed
    unwarped = render(app, document)
    history = cursor(app, document)
    reply = app.call('transform_layers', document_id=document, layers=[index],
                     source_rect=RECT, corners=QUAD, sampler='bilinear')
    assert reply['ok'] and reply['changed'], reply
    assert cursor(app, document) == history + 1, 'the projective transform is not one step'
    assert app.call('get_image_source', document_id=document,
                    layer=index)['source'] is None
    before = render(app, document)
    assert before != unwarped, 'the projective transform left the render unchanged'
    native = folder / f'placed_{depth}.rz'
    app.call('save_copy', document_id=document, path=str(native))
    reopened = app.call('open_document', path=str(native))['document']['id']
    assert render(app, reopened) == before, 'the placed raster layer changed on reopen'
    assert app.call('get_image_source', document_id=reopened,
                    layer=index)['source'] is None
    for tool, arguments in [
            ('crop', {'x': 1, 'y': 1, 'width': WIDTH - 2, 'height': HEIGHT - 2}),
            ('rotate', {'degrees': 90})]:
        prior = render(app, reopened)
        app.call(tool, document_id=reopened, **arguments)
        app.call('undo', document_id=reopened)
        assert render(app, reopened) == prior, (tool, 'undo changed the placed layer')
    return {'source': None, 'projective_history_delta': 1, 'projective_reopen': True,
            'crop_rotate_undo_exact': True}


def signed_zero_float_fixture(destination):
    """Copy the committed independent float TIFF and change one opaque
    interior blue component to negative zero, preserving all other HDR and
    negative samples. Only the Deflate strip and its byte-count field change.
    """
    data = bytearray(FLOAT_FIXTURE.read_bytes())
    ifd = struct.unpack_from('<I', data, 4)[0]
    count = struct.unpack_from('<H', data, ifd)[0]
    entries = {struct.unpack_from('<H', data, ifd + 2 + i * 12)[0]: ifd + 2 + i * 12
               for i in range(count)}
    start = struct.unpack_from('<I', data, entries[273] + 8)[0]
    length = struct.unpack_from('<I', data, entries[279] + 8)[0]
    assert start + length == len(data), 'fixture strip must be the final payload'
    raw = bytearray(zlib.decompress(data[start:start + length]))
    assert len(raw) == 48 * 32 * 16
    sentinel = (10 * 48 + 30) * 16 + 8
    struct.pack_into('<f', raw, sentinel, -0.0)
    compressed = zlib.compress(raw, 6)
    struct.pack_into('<I', data, entries[279] + 8, len(compressed))
    destination.write_bytes(data[:start] + compressed)
    parsed = read_float_tiff(destination)
    assert struct.pack('<f', parsed['samples'][(10 * 48 + 30) * 4 + 2]) == b'\0\0\0\x80'
    assert max(parsed['samples']) >= 8.0 and min(parsed['samples']) <= -0.25


def float_gate(app, folder):
    fixture_path = folder / 'float-set.tiff'
    signed_zero_float_fixture(fixture_path)
    document = app.call('open_document', path=str(fixture_path))['document']['id']
    def native_layers(label, expected):
        path = folder / (label + '.rz')
        app.call('save_copy', document_id=document, path=str(path), format='rz')
        encoded = path.read_bytes()
        # Float DRDC records contain width, height, byte length and then raw
        # little-endian f32 RGBA. Find exactly the expected records and compare
        # their payload bytes; a flattened export would erase signed zero.
        header = struct.pack('<IIQ', 48, 32, 48 * 32 * 16)
        records = []
        at = 0
        while (at := encoded.find(header, at)) >= 0:
            body = encoded[at + len(header):at + len(header) + 48 * 32 * 16]
            assert len(body) == 48 * 32 * 16
            records.append(body)
            at += len(header) + len(body)
        assert len(records) == expected, (label, len(records), expected)
        return records

    before = native_layers('float-before', 1)[0]
    assert before[(10 * 48 + 30) * 16 + 8:(10 * 48 + 30) * 16 + 12] == b'\0\0\0\x80'
    assert struct.unpack_from('<f', before, 0)[0] == 8.0
    assert struct.unpack_from('<f', before, (4 * 48 + 16) * 16)[0] == -0.25

    before_history = cursor(app, document)
    single = app.call('transform_layers', document_id=document, layers=[0],
                      matrix=[1, 0, 0, 1, 3, -2], sampler='nearest')
    assert single['ok'] and single['changed'] and single['layers'] == [0], single
    assert native_layers('float-single-moved', 1)[0] == before, \
        'integer set transform changed signed-zero/HDR layer bits'
    moved_offset = app.call('get_document', document_id=document)['layers'][0]['offset']
    assert moved_offset == [3, -2], moved_offset
    assert cursor(app, document) == before_history + 1
    app.call('undo', document_id=document)
    assert native_layers('float-restored', 1)[0] == before

    app.call('duplicate_layer', document_id=document, index=0)
    two_before = native_layers('float-two-before', 2)
    two_offsets = [row['offset'] for row in app.call('get_document',
        document_id=document)['layers']]
    history = cursor(app, document)
    translated = app.call('transform_layers', document_id=document, layers=[0, 1],
                          matrix=[1, 0, 0, 1, 2, 1], sampler='nearest')
    assert translated['ok'] and translated['changed'] and translated['layers'] == [0, 1], translated
    assert native_layers('float-two-moved', 2) == two_before, \
        'two-root integer set transform changed raw float layer bits'
    after_offsets = [row['offset'] for row in app.call('get_document',
        document_id=document)['layers']]
    assert after_offsets == [[pair[0] + 2, pair[1] + 1] for pair in two_offsets]
    assert cursor(app, document) == history + 1
    for label, parameters in [
            ('filtered affine', {'matrix': [1.125, 0, 0, 1, 0, 0], 'sampler': 'bilinear'}),
            ('perspective', {'source_rect': [0, 0, 48, 32],
                             'corners': [[0, 0], [48, 0], [46, 32], [0, 32]],
                             'sampler': 'nearest'})]:
        # The filtered set transforms are admitted at 32-bit float.
        prior = cursor(app, document)
        reply = app.call('transform_layers', document_id=document, layers=[0, 1],
                         **parameters)
        assert reply['ok'] and reply['changed'], (label, reply)
        assert cursor(app, document) == prior + 1, (label, 'not one history step')
        app.call('undo', document_id=document)
        assert cursor(app, document) == prior, (label, 'undo did not restore the cursor')
    return {'single_signed_zero_hdr_exact': True, 'two_root_translation_exact': True,
            'filtered_affine_admitted': True, 'perspective_admitted': True}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts)
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {}
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(Path(args.app).resolve())) as app:
            for depth in (8, 16):
                for perspective in (False, True):
                    key = f'{depth}_{"perspective" if perspective else "affine"}'
                    receipts[key] = group_gate(app, folder, depth, perspective)
                    print('PASS', key, flush=True)
            receipts['atomic_refusals'] = atomic_refusals(app, folder)
            print('PASS atomic_refusals', flush=True)
            receipts['replay'] = replay_gate(app, folder)
            print('PASS replay', flush=True)
            for depth in (8, 16):
                receipts[f'artwork_{depth}'] = artwork_gate(app, folder, depth)
                print('PASS artwork', depth, flush=True)
                receipts[f'source_{depth}'] = source_gate(app, folder, depth)
                print('PASS placed raster layer', depth, flush=True)
            receipts['float'] = float_gate(app, folder)
            print('PASS float set exact/admission', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))


if __name__ == '__main__':
    main()
