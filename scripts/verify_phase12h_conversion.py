#!/usr/bin/env python3
"""Owned-app Phase 12H drive: structure-preserving precision conversion —
12H-1's 8→16 and 12H-2's 16→8, float→16 and float→8.

Every fixture is built over MCP on an 8-bit `new_document` (rasters with
offsets and brush strokes, an isolated and a Pass Through group, a Levels
adjustment, a text and a shape layer, a feathered layer mask, a saved
selection, a drop shadow + Blend If + knockout style, clipping, a lock, a link,
a label, a layer comp, guides, a moved ruler origin and a PPI), and every
expected value is read from that fixture before the conversion.

Gates (tolerances fixed in designs/phase-12h-conversion-status.md before
measuring): g1 dry-run plan; g2 a dry run records NO step; g3 the schema lists
only `mode` as required, preserve without `flatten` is accepted and
`{structure:"preserve", flatten:true}` is refused by name with the document
unchanged; g4 convert — rows equal, 8-bit render before vs the 16-bit render
after within max 3 / mean 0.5 codes of 255, the reply's preview_delta within
max 771 / mean 128.5 codes of 65535; g5 ONE undo restores a save byte-identical
to the pre-conversion save, redo re-applies; g6 save → reopen → resave is
byte-identical; g7 a placed image is an ordinary raster layer (`source:
null`) and is CARRIED to 16 bits like any other layer (12H-2); g8 a recorded
conversion saved as an Action converts a fresh fixture with the same row set.

12H-2 gates, on a 16-bit fixture (the 8-bit fixture carried by preserve
rgba16) and on the committed float fixture layered over MCP: n1 the rgba8
dry-run plan; n2 preserve rgba8 — rows, chrome and comps equal, preview_delta
within max 771 / mean 128.5, the 16-bit render (reduced to 8) vs the 8-bit
render after within max 3 / mean 0.5; n3 one undo restores a save
byte-identical to the pre-conversion 16-bit save, redo re-applies; n4 save →
reopen → resave byte-identical; n5 float → rgba16 preserve: rows equal,
linear_light, preview_delta RECORDED (an HDR fixture, never bounded), undo/redo,
resave identical; n6 float → rgba8 preserve with ordered dither: rows equal and
the recorded {mode, structure, dither} step replays through run_action; c1 a
warp binding survives 8→16 and back to 8 beside a placed raster layer; r1 a
write_recovery checkpoint of the n2 and n5 documents reopens and re-saves
digest-equal to a save of the live document. A failure exits nonzero and
leaves a JSON receipt in --receipts.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8
from support import HostApp

W, H = 48, 36
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
STYLE = {
    'fill_opacity': 0.9,
    'knockout': 'shallow',
    'blend_if': {'channel': 'gray', 'this_layer': [0, 0, 220, 255],
                 'underlying': [0, 20, 255, 255]},
    'effects': [{'type': 'drop_shadow', 'use_global_light': False,
                 'distance': 2, 'size': 2}],
}
LOCK_BITS = {'transparency': 1, 'pixels': 2, 'position': 4}


# ------------------------------------------------------------- plumbing --

def refusal(app, name, **arguments):
    reply = app.client.rpc('tools/call', {'name': name, 'arguments': arguments})
    assert reply.get('isError'), (name, reply)
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def index_of(app, document, name):
    return next(row['index'] for row in layers(app, document) if row['name'] == name)


def render8(app, document):
    """The render tool's PNG, reduced to 8-bit codes when it is 16-bit."""
    reply = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document}})
    assert not reply.get('isError'), reply
    encoded = next(row['data'] for row in reply['content'] if row['type'] == 'image')
    decoded = png_rgba(base64.b64decode(encoded))
    samples = list(decoded['samples'])
    if decoded['depth'] == 16:
        samples = [(v + 128) // 257 for v in samples]
    return decoded['width'], decoded['height'], decoded['depth'], samples


def save_sha(app, document, path):
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def resave_identical(app, document, folder, label):
    first = folder / (label + '-a.rz')
    second = folder / (label + '-b.rz')
    app.call('save_copy', document_id=document, path=str(first), format='rz')
    reopened = app.call('open_document', path=str(first))['document']['id']
    app.call('save_copy', document_id=reopened, path=str(second), format='rz')
    a = hashlib.sha256(first.read_bytes()).hexdigest()
    b = hashlib.sha256(second.read_bytes()).hexdigest()
    assert a == b, (label, 'resave differs', a, b)
    return {'sha256': a, 'reopened': reopened}


def parents(depths):
    """Postorder parent of each row: the first later row one level up."""
    out = []
    for i, depth in enumerate(depths):
        out.append(next((j for j in range(i + 1, len(depths)) if depths[j] < depth), None))
    return out


def canonical(rows):
    """The rows of either depth's `get_document`, read with ONE rule.

    Both depths answer the byte vocabulary (kind layer/group,
    is_adjustment, the adjustment's op), so the kind is derived the same way
    at both. Compared: id, name, kind, depth, parent, blend, opacity, visible,
    clipped, the mask (presence, enabled, properties, placement, extent),
    locks, link, label, style, text/shape, the adjustment's op, a group's
    `open` (carried, not discarded) and a raster's offset. Deliberately
    NOT compared: the group width/height/offset: the byte row's are a union
    bounding box, which the typed row does not compute the same way.
    """
    out = []
    for row, parent in zip(rows, parents([row['depth'] for row in rows])):
        typed = 'subtree_start' in row
        assert row['parent'] == parent, (row, parent)
        if row['kind'] == 'group':
            kind = 'pass_through_group' if row['blend_mode'] == 'Pass Through' \
                else 'isolated_group'
        else:
            kind = 'adjustment' if row['is_adjustment'] else 'raster'
        adjustment = row.get('adjustment') and row['adjustment']['op']
        if typed:
            blend = row['blend_mode']
            locks = sum(LOCK_BITS[name] for name in row.get('locks', []))
            link = row['link']
            mask = row.get('mask') and {
                'enabled': row['mask']['enabled'], 'density': row['mask']['density'],
                'feather': row['mask']['feather'], 'linked': row['mask']['linked'],
                'offset': list(row['mask']['offset']),
                'dimensions': list(row['mask']['dimensions']),
                'outside_coverage': row['mask']['outside_coverage']}
        else:
            blend = row['blend_mode']
            locks = sum(LOCK_BITS[name] for name in row.get('locks', []))
            link = row.get('linked', 0)
            props = row.get('mask_properties')
            mask = row['has_mask'] and {
                'enabled': row['mask_enabled'], 'density': props['density'],
                'feather': props['feather'], 'linked': props['linked'],
                'offset': [props['offset_x'], props['offset_y']],
                'dimensions': [props['width'], props['height']],
                'outside_coverage': props['outside_coverage']}
        out.append({'id': row['id'], 'name': row['name'], 'kind': kind, 'depth': row['depth'],
                    'parent': parent, 'blend': blend, 'opacity': row['opacity'],
                    'visible': row['visible'], 'clipped': row['clipped'],
                    'mask': mask or None, 'locks': locks, 'link': link, 'label': row['label'],
                    'style': row.get('style'), 'text': row.get('text'),
                    'shape': row.get('shape'), 'adjustment': adjustment or None,
                    'open': row['open'] if row['kind'] == 'group' else None,
                    'offset': [row['offset_x'], row['offset_y']]
                    if kind in ('raster', 'adjustment') else None})
    return out


def chrome(info):
    resolution = info['resolution']
    ppi = [resolution.get('ppi_x', resolution.get('x_ppi')),
           resolution.get('ppi_y', resolution.get('y_ppi'))]
    channels = []
    for row in info.get('channels', []):
        color = row['overlay_color']
        if isinstance(color, list):
            color = '#%02X%02X%02X' % tuple(color)
        selected = row.get('color_indicates_selected')
        if selected is None:
            selected = row.get('color_indicates') == 'selected'
        channels.append({'name': row['name'], 'overlay_color': color,
                         'overlay_opacity': row['overlay_opacity'], 'selected': selected})
    return {'guides': info.get('guides'), 'ruler': info.get('ruler'), 'ppi': ppi,
            'channels': channels}


def snapshot(app, document):
    info = app.call('get_document', document_id=document)
    return {'rows': canonical(info['layers']), 'chrome': chrome(info),
            'comps': app.call('list_layer_comps', document_id=document)}


def is16(app, document):
    precision = app.call('get_document', document_id=document).get('precision')
    assert precision in ('rgba8', 'rgba16'), precision
    return precision == 'rgba16'


def precision_of(app, document):
    return app.call('get_document', document_id=document).get('precision')


def policy(app, document):
    return app.call('get_document', document_id=document)['evaluation']['policy']


def notes(reply):
    return [row['note'] for row in reply['conversion']['rendering_differences']]


def warp_request(columns=3, rows=3):
    """The 12F-3 drive's lattice."""
    return dict(operation='set', target='pixels', columns=columns, rows=rows,
                displacements=[[0.35 + 0.45 * x / (columns - 1) - 0.19 * y / (rows - 1),
                                -0.27 + 0.21 * y / (rows - 1) + 0.11 * x / (columns - 1)]
                               for y in range(rows) for x in range(columns)],
                filter='bicubic')


def delta8(before, after):
    assert before[:2] == after[:2], (before[:2], after[:2])
    a, b = before[3], after[3]
    worst, total, count = 0, 0, 0
    for i in range(0, len(a), 4):
        channels_ = range(3, 4) if a[i + 3] == 0 and b[i + 3] == 0 else range(4)
        for c in channels_:
            d = abs(a[i + c] - b[i + c])
            worst = max(worst, d)
            total += d
            count += 1
    return {'max': worst, 'mean': total / count, 'compared': count}


# -------------------------------------------------------------- fixture --

def fixture(app):
    """The 8-bit fixture, returning its document id."""
    document = app.call('new_document', width=W, height=H, depth=8,
                        background='white')['document']['id']
    call = lambda tool, **a: app.call(tool, document_id=document, **a)  # noqa: E731
    call('new_layer', name='A')
    call('brush_stroke', layer=index_of(app, document, 'A'), color='#c83c3c',
         points=[[4, 4], [30, 20], [40, 30]], size=7)
    call('new_layer', name='B')
    call('brush_stroke', layer=index_of(app, document, 'B'), color='#3c7ac8',
         points=[[10, 30], [28, 6]], size=9)
    call('move_layers', layers=[index_of(app, document, 'B')], dx=3, dy=-2)
    call('set_layer_clipped', layer=index_of(app, document, 'B'), clipped=True)
    grouped = call('group_layers', layers=[index_of(app, document, 'A'),
                                           index_of(app, document, 'B')], name='Iso')
    call('set_layer_properties', index=grouped['group'], blend_mode='Multiply')
    call('new_layer', name='C')
    call('brush_stroke', layer=index_of(app, document, 'C'), color='#50b450',
         points=[[2, 20], [46, 22]], size=8)
    call('add_adjustment_layer', op='levels', params={'gamma': 0.8}, name='Levels')
    passed = call('group_layers', layers=[index_of(app, document, 'C'),
                                          index_of(app, document, 'Levels')], name='Pass')
    # A CLOSED group, so the conversion's disclosure carry is exercised.
    call('set_layer_properties', index=passed['group'], open=False)
    call('add_text_layer', text='Hi', x=4, y=6, size=12)
    call('add_shape_layer', kind='rect', x=20, y=18, w=16, h=10, fill='#e0a020')
    shape = next(row for row in layers(app, document) if 'shape' in row)
    call('select_rect', x=1, y=1, width=30, height=24)
    call('add_layer_mask', layer=index_of(app, document, 'C'), kind='from_selection')
    call('set_mask_properties', layer=index_of(app, document, 'C'), feather=2)
    call('save_selection', name='Saved')
    call('clear_selection')
    call('set_layer_style', index=shape['index'], style=STYLE)
    call('set_layer_lock', layer=shape['index'], locks=['position'])
    text = next(row for row in layers(app, document) if 'text' in row)
    call('link_layers', layers=[text['index'], shape['index']])
    call('set_layer_label', layer=text['index'], label='orange')
    call('save_layer_comp', name='Comp')
    call('add_guide', orientation='vertical', position=10.5)
    call('add_guide', orientation='horizontal', position=7)
    call('set_ruler_origin', x=2, y=3)
    call('set_resolution', ppi_x=200)
    return document


# ------------------------------------------------------------------ gates --

def g1(app, document):
    history = cursor(app, document)
    reply = app.call('convert_precision', document_id=document, mode='rgba16',
                     structure='preserve', dry_run=True)
    assert reply['ok'] and reply['dry_run'] and reply['structure'] == 'preserve', reply
    plan = reply['conversion']
    assert plan['structure'] == 'preserve' and plan['flattened'] is False, plan
    assert plan['blocked'] == [] and reply['would_refuse'] is None, reply
    assert 'group_open_state' in plan['preserved'], plan
    assert 'group_open_state' not in plan['discarded'], plan
    # The global light carries too.
    assert 'global_light' in plan['preserved'], plan
    assert 'global_light' not in plan['discarded'], plan
    notes = [row['note'] for row in plan['rendering_differences']]
    assert any('isolated group' in n for n in notes), notes
    assert any('adjustment' in n for n in notes), notes
    assert any('mask feather' in n for n in notes), notes
    assert cursor(app, document) == history
    assert not is16(app, document)
    return {'plan': plan}


def g2(app, document):
    app.call('start_recording')
    app.call('convert_precision', document_id=document, mode='rgba16',
             structure='preserve', dry_run=True)
    stopped = app.call('stop_recording')
    tools = [step['tool'] for step in stopped.get('steps', [])]
    assert 'convert_precision' not in tools, stopped
    return {'recorded_tools': tools}


def g3(app, document):
    listed = app.client.rpc('tools/list', {})
    entry = next(tool for tool in listed['tools'] if tool['name'] == 'convert_precision')
    schema = entry['inputSchema']
    assert schema.get('required') == ['mode'], schema
    assert set(schema['properties']) >= {'mode', 'structure', 'flatten', 'dither', 'dry_run'}
    before = snapshot(app, document)
    history = cursor(app, document)
    message = refusal(app, 'convert_precision', document_id=document, mode='rgba16',
                      structure='preserve', flatten=True)
    assert 'flatten=true contradicts structure=preserve' in message, message
    legacy = refusal(app, 'convert_precision', document_id=document, mode='rgba16')
    assert 'flatten=true' in legacy, legacy
    assert snapshot(app, document) == before and cursor(app, document) == history
    return {'required': schema['required'], 'contradiction': message, 'no_flatten': legacy}


def g4(app, document, folder):
    info8 = app.call('get_document', document_id=document)
    before = snapshot(app, document)
    picture = render8(app, document)
    pre_sha = save_sha(app, document, folder / 'g4-before.rz')
    # The schema gate's other half: preserve WITHOUT `flatten` succeeds.
    reply = app.call('convert_precision', document_id=document, mode='rgba16',
                     structure='preserve')
    assert reply['ok'] and reply['structure'] == 'preserve', reply
    delta = reply['preview_delta']
    assert delta['scale'] == 65535 and delta['compared_samples'] > 0, delta
    assert delta['max_delta'] <= 771 and delta['mean_delta'] <= 128.5, delta
    assert is16(app, document)
    after = snapshot(app, document)
    for old, new in zip(before['rows'], after['rows']):
        assert old == new, ('row differs', old, new)
    assert len(before['rows']) == len(after['rows'])
    assert before['chrome'] == after['chrome'], (before['chrome'], after['chrome'])
    assert before['comps'] == after['comps'], (before['comps'], after['comps'])
    render_delta = delta8(picture, render8(app, document))
    assert render_delta['max'] <= 3 and render_delta['mean'] <= 0.5, render_delta
    info16 = app.call('get_document', document_id=document)
    parity = key_parity(info8, info16)
    # list_channels answers get_document's channel rows at every depth.
    listed = app.call('list_channels', document_id=document)
    assert listed['channels'] == info16['channels'], (listed, info16['channels'])
    closed = closed_groups(app, document, folder / 'g4-after.rz')
    assert closed == [index_of(app, document, 'Pass')], closed
    return {'pre_sha': pre_sha, 'preview_delta': delta, 'render_delta': render_delta,
            'rows': len(after['rows']), 'key_parity': parity}


def closed_groups(app, document, path):
    """The typed file's closed groups, read by the shared independent
    Python peel (`scripts/raw-probe/native.py`), which must understand word 55
    and its LOPN table as well as the product does."""
    spec = importlib.util.spec_from_file_location(
        'raw_probe_native', ROOT / 'scripts/raw-probe/native.py')
    reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reader)
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    data = Path(path).read_bytes()
    assert data[4:12] == bytes([46, 0, 0, 0, 55, 0, 0, 0]), data[4:12]
    body, shift, ids, word, _ = reader.identity_body(data)
    assert word == 46 and shift == 8 and ids, (word, shift)
    lids = data.rfind(b'LIDS')
    return reader.group_open_table(data, 12, lids)[1]


def key_parity(info8, info16):
    """The typed reply carries every byte key in the byte shape.

    The allowed gap is the row content box; the typed extras are
    deliberate and not compared. `global_light` is typed.
    """
    missing = set(info8) - set(info16)
    assert not missing, ('document keys missing at 16 bits', missing)
    assert info8['global_light'] == info16['global_light'], (info8['global_light'], info16['global_light'])
    assert len(info8['layers']) == len(info16['layers'])
    row_gaps = set()
    for r8, r16 in zip(info8['layers'], info16['layers']):
        assert r8['id'] == r16['id'], (r8['id'], r16['id'])
        gap = set(r8) - set(r16)
        assert gap <= {'content_x', 'content_y', 'content_width', 'content_height'}, \
            ('row keys missing at 16 bits', r8['name'], gap)
        row_gaps |= gap
        assert isinstance(r16['blend_mode'], str), r16
        # One vocabulary — the byte kind, the two-decimal opacity and
        # the adjustment's op.
        assert r8['kind'] == r16['kind'], (r8['name'], r8['kind'], r16['kind'])
        assert r8['opacity'] == r16['opacity'], (r8['name'], r8['opacity'], r16['opacity'])
        assert (r8.get('adjustment') or {}).get('op') == (r16.get('adjustment') or {}).get('op'), \
            (r8['name'], r8.get('adjustment'), r16.get('adjustment'))
    for c8, c16 in zip(info8.get('channels', []), info16.get('channels', [])):
        assert set(c8) <= set(c16), ('channel keys missing at 16 bits', set(c8) - set(c16))
    assert 'ppi_x' in info16['resolution'], info16['resolution']
    return {'document_8': sorted(info8), 'document_16': sorted(info16),
            'document_missing_16': sorted(missing), 'row_missing_16': sorted(row_gaps),
            'channel_8': sorted(info8['channels'][0]) if info8.get('channels') else [],
            'channel_16': sorted(info16['channels'][0]) if info16.get('channels') else []}


def g5(app, document, folder, pre_sha):
    app.call('undo', document_id=document)
    assert not is16(app, document)
    undone = save_sha(app, document, folder / 'g5-undone.rz')
    assert undone == pre_sha, ('undo is not the pre-conversion document', undone, pre_sha)
    app.call('redo', document_id=document)
    assert is16(app, document)
    return {'undone_sha': undone}


def placed_fixture(app, folder, label):
    document = fixture(app)
    png = folder / (label + '-place.png')
    write_png8(png, 8, 6, bytes([(i * 37) % 256 for i in range(8 * 6 * 4)]))
    placed = app.call('place_image', document_id=document, path=str(png))
    index = placed['layer']
    # A placed PNG is an ordinary raster layer: nothing of the file is bound.
    assert placed['source'] is None, placed
    assert app.call('get_image_source', document_id=document,
                    layer=index)['source'] is None
    return document, index


def g7(app, folder):
    """12H-2: a placed image, an ordinary raster layer, is CARRIED to 16 bits."""
    document, index = placed_fixture(app, folder, 'g7')
    before = snapshot(app, document)
    dry = app.call('convert_precision', document_id=document, mode='rgba16',
                   structure='preserve', dry_run=True)
    assert dry['would_refuse'] is None and dry['conversion']['blocked'] == [], dry
    reply = app.call('convert_precision', document_id=document, mode='rgba16',
                     structure='preserve')
    assert is16(app, document)
    assert not any('source re-renders' in n for n in notes(reply)), notes(reply)
    after = snapshot(app, document)
    assert after['rows'] == before['rows'], (before['rows'], after['rows'])
    carried = app.call('get_image_source', document_id=document, layer=index)['source']
    assert carried is None, carried
    return {'source_before': None, 'source_after': carried, 'rows': len(after['rows'])}


def g8(app):
    source = fixture(app)
    app.call('start_recording')
    app.call('convert_precision', document_id=source, mode='rgba16', structure='preserve')
    stopped = app.call('stop_recording')
    steps = [step for step in stopped['steps'] if step['tool'] == 'convert_precision']
    assert len(steps) == 1, stopped['steps']
    arguments = steps[0]['arguments']
    assert arguments.get('structure') == 'preserve' and arguments.get('mode') == 'rgba16', steps
    assert 'flatten' not in arguments or arguments['flatten'] is False, steps
    app.call('save_action', name='12h preserve', steps=stopped['steps'], overwrite=True)
    fresh = fixture(app)
    before = snapshot(app, fresh)
    ran = app.call('run_action', name='12h preserve', document_id=fresh)
    assert ran['ok'], ran
    assert is16(app, fresh)
    after = snapshot(app, fresh)
    assert [row['name'] for row in after['rows']] == [row['name'] for row in before['rows']]
    for old, new in zip(before['rows'], after['rows']):
        assert old == new, ('replayed row differs', old, new)
    return {'step': steps[0], 'rows': len(after['rows'])}


# ------------------------------------------------------------ 12H-2 --

def sixteen_fixture(app):
    """The 8-bit fixture carried to 16 bits by preserve."""
    document = fixture(app)
    app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    assert is16(app, document)
    return document


def n1(app, document):
    history = cursor(app, document)
    reply = app.call('convert_precision', document_id=document, mode='rgba8',
                     structure='preserve', dry_run=True)
    plan = reply['conversion']
    assert plan['direction'] == 'rgba16_to_rgba8' and plan['blocked'] == [], plan
    assert reply['would_refuse'] is None, reply
    assert plan['preserved'][0] == 'raster_pixels_quantized', plan['preserved']
    assert plan['alpha_quantization'] == 'nearest, never dithered', plan
    assert any('byte rounding' in row['note'] for row in plan['rendering_differences']), plan
    assert cursor(app, document) == history and is16(app, document)
    return {'plan': plan}


def n2(app, document, folder):
    before = snapshot(app, document)
    picture = render8(app, document)
    pre_sha = save_sha(app, document, folder / 'n2-before.rz')
    reply = app.call('convert_precision', document_id=document, mode='rgba8',
                     structure='preserve', dither='none')
    delta = reply['preview_delta']
    assert delta['scale'] == 65535 and delta['compared_samples'] > 0, delta
    assert delta['max_delta'] <= 771 and delta['mean_delta'] <= 128.5, delta
    assert not is16(app, document)
    after = snapshot(app, document)
    assert len(before['rows']) == len(after['rows'])
    for old, new in zip(before['rows'], after['rows']):
        assert old == new, ('row differs', old, new)
    assert before['chrome'] == after['chrome'], (before['chrome'], after['chrome'])
    assert before['comps'] == after['comps'], (before['comps'], after['comps'])
    render_delta = delta8(picture, render8(app, document))
    assert render_delta['max'] <= 3 and render_delta['mean'] <= 0.5, render_delta
    return {'pre_sha': pre_sha, 'preview_delta': delta, 'render_delta': render_delta,
            'rows': len(after['rows'])}


def n3(app, document, folder, pre_sha):
    app.call('undo', document_id=document)
    assert is16(app, document)
    undone = save_sha(app, document, folder / 'n3-undone.rz')
    assert undone == pre_sha, ('undo is not the pre-conversion document', undone, pre_sha)
    app.call('redo', document_id=document)
    assert not is16(app, document)
    return {'undone_sha': undone}


def float_fixture(app, folder, label):
    """The committed 11F float scene, layered over MCP: a new layer, a
    duplicate clipped onto the background, a group holding the new layer
    with a mask, a lock, a label, a guide and a comp. Each call opens its
    own COPY, because opening an already open path returns that document."""
    path = folder / (label + '-scene.tiff')
    path.write_bytes(FLOAT_FIXTURE.read_bytes())
    document = app.call('open_document', path=str(path))['document']['id']
    assert precision_of(app, document) not in ('rgba8', 'rgba16'), precision_of(app, document)
    call = lambda tool, **a: app.call(tool, document_id=document, **a)  # noqa: E731
    call('new_layer', name='Float New')
    call('duplicate_layer', index=0)
    call('set_layer_clipped', layer=1, clipped=True)
    grouped = call('group_layers', layers=[index_of(app, document, 'Float New')],
                   name='Float Group')
    call('select_rect', x=2, y=2, width=20, height=12)
    call('add_layer_mask', layer=index_of(app, document, 'Float New'), kind='from_selection')
    call('set_layer_lock', layer=0, locks=['position'])
    call('set_layer_label', layer=grouped['group'], label='green')
    call('add_guide', orientation='vertical', position=9.5)
    call('save_layer_comp', name='Float Comp')
    return document


def n5(app, folder):
    document = float_fixture(app, folder, 'n5')
    before = snapshot(app, document)
    pre_sha = save_sha(app, document, folder / 'n5-before.rz')
    reply = app.call('convert_precision', document_id=document, mode='rgba16',
                     structure='preserve')
    plan = reply['conversion']
    assert plan['direction'] == 'rgba32_float_to_rgba16', plan
    assert 'extended_range' in plan['discarded'], plan
    assert any('tone-mapped per record' in n for n in notes(reply)), notes(reply)
    assert is16(app, document) and policy(app, document) == 'linear_light'
    after = snapshot(app, document)
    assert len(before['rows']) == len(after['rows'])
    for old, new in zip(before['rows'], after['rows']):
        assert old == new, ('row differs', old, new)
    assert before['chrome'] == after['chrome'] and before['comps'] == after['comps']
    app.call('undo', document_id=document)
    assert precision_of(app, document) not in ('rgba8', 'rgba16')
    assert save_sha(app, document, folder / 'n5-undone.rz') == pre_sha
    app.call('redo', document_id=document)
    assert is16(app, document)
    resave = resave_identical(app, document, folder, 'n5')
    return {'document': document, 'preview_delta_recorded': reply['preview_delta'],
            'tone_map': plan.get('tone_map'), 'rows': len(after['rows']), 'resave': resave}


def n6(app, folder):
    source = float_fixture(app, folder, 'n6')
    before = snapshot(app, source)
    # g8's recording is still pending in memory (saved as an Action through
    # its steps, not through the window), so this one discards it.
    app.call('start_recording', discard=True)
    reply = app.call('convert_precision', document_id=source, mode='rgba8',
                     structure='preserve', dither='ordered')
    stopped = app.call('stop_recording')
    assert reply['conversion']['direction'] == 'rgba32_float_to_rgba8', reply['conversion']
    assert not is16(app, source) and policy(app, source) == 'linear_light'
    after = snapshot(app, source)
    for old, new in zip(before['rows'], after['rows']):
        assert old == new, ('row differs', old, new)
    steps = [step for step in stopped['steps'] if step['tool'] == 'convert_precision']
    assert len(steps) == 1, stopped['steps']
    arguments = steps[0]['arguments']
    assert arguments == {'mode': 'rgba8', 'structure': 'preserve', 'dither': 'ordered'}, arguments
    app.call('save_action', name='12h2 preserve rgba8', steps=stopped['steps'], overwrite=True)
    fresh = float_fixture(app, folder, 'n6-replay')
    fresh_before = snapshot(app, fresh)
    ran = app.call('run_action', name='12h2 preserve rgba8', document_id=fresh)
    assert ran['ok'], ran
    assert not is16(app, fresh)
    replayed = snapshot(app, fresh)
    assert len(replayed['rows']) == len(fresh_before['rows'])
    for old, new in zip(fresh_before['rows'], replayed['rows']):
        assert old == new, ('replayed row differs', old, new)
    return {'step': steps[0], 'preview_delta_recorded': reply['preview_delta'],
            'rows': len(after['rows'])}


def c1(app, folder):
    """A warp binding survives 8→16 and back to 8 beside a placed raster layer."""
    document, index = placed_fixture(app, folder, 'c1')
    warped = index_of(app, document, 'A')
    app.call('warp_layer', document_id=document, layer=warped, **warp_request())
    field = app.call('get_warp', document_id=document, layer=warped)['field']
    assert field, field
    out = {}
    for mode, needle in (('rgba16', 'warp re-edits at 16 bits'),
                         ('rgba8', "warp's retained original quantized to 8 bits")):
        reply = app.call('convert_precision', document_id=document, mode=mode,
                         structure='preserve')
        assert any(needle in n for n in notes(reply)), notes(reply)
        assert is16(app, document) == (mode == 'rgba16')
        carried = app.call('get_image_source', document_id=document, layer=index)['source']
        assert carried is None, (mode, carried)
        again = app.call('get_warp', document_id=document, layer=warped)['field']
        assert again == field, (mode, again, field)
        out[mode] = {'notes': [n for n in notes(reply) if 'warp' in n]}
    out['placed_source'] = None
    out['field_columns'] = field.get('columns')
    return out


def r1(app, folder, documents):
    """write_recovery → open the checkpoint's image.rz → save both: equal."""
    out = {}
    for label, document in documents.items():
        app.call('write_recovery', document_id=document, wait=True)
        checkpoints = sorted((folder / 'recovery').rglob('image.rz'),
                             key=lambda p: p.stat().st_mtime)
        assert checkpoints, 'write_recovery wrote no native checkpoint'
        checkpoint = app.call('open_document', path=str(checkpoints[-1]))['document']['id']
        live = save_sha(app, document, folder / f'r1-{label}-live.rz')
        recovered = save_sha(app, checkpoint, folder / f'r1-{label}-recovered.rz')
        assert live == recovered, (label, live, recovered)
        out[label] = {'sha256': live, 'checkpoint': str(checkpoints[-1])}
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            document = fixture(app)
            receipts['fixture_rows'] = layers(app, document)
            receipts['g1'] = g1(app, document)
            print('PASS g1 dry-run plan edits nothing', flush=True)
            receipts['g2'] = g2(app, document)
            print('PASS g2 a dry run records no step', flush=True)
            receipts['g3'] = g3(app, document)
            print('PASS g3 schema: only mode required; flatten=true + preserve refused', flush=True)
            receipts['g4'] = g4(app, document, folder)
            print('PASS g4 preserve converts with every row carried, within tolerance', flush=True)
            receipts['g5'] = g5(app, document, folder, receipts['g4']['pre_sha'])
            print('PASS g5 one undo restores the byte document; redo re-applies', flush=True)
            receipts['g6'] = resave_identical(app, document, folder, 'g6')
            print('PASS g6 save, reopen, resave byte-identical', flush=True)
            receipts['g7'] = g7(app, folder)
            print('PASS g7 a placed raster layer is carried to 16 bits', flush=True)
            receipts['g8'] = g8(app)
            print('PASS g8 recorded preserve replays on a fresh fixture', flush=True)
            sixteen = sixteen_fixture(app)
            receipts['n1'] = n1(app, sixteen)
            print('PASS n1 16→8 dry-run plan edits nothing', flush=True)
            receipts['n2'] = n2(app, sixteen, folder)
            print('PASS n2 16→8 preserve carries every row, within tolerance', flush=True)
            receipts['n3'] = n3(app, sixteen, folder, receipts['n2']['pre_sha'])
            print('PASS n3 one undo restores the 16-bit document; redo re-applies', flush=True)
            receipts['n4'] = resave_identical(app, sixteen, folder, 'n4')
            print('PASS n4 save, reopen, resave byte-identical', flush=True)
            receipts['n5'] = n5(app, folder)
            print('PASS n5 float→16 preserve carries every row (delta recorded)', flush=True)
            receipts['n6'] = n6(app, folder)
            print('PASS n6 float→8 preserve with ordered dither replays as an Action', flush=True)
            receipts['c1'] = c1(app, folder)
            print('PASS c1 a warp survives 8→16→8 beside a placed raster layer', flush=True)
            receipts['r1'] = r1(app, folder, {'n2': sixteen, 'n5': receipts['n5']['document']})
            print('PASS r1 recovery checkpoints re-save digest-equal', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))


if __name__ == '__main__':
    main()
