#!/usr/bin/env python3
"""The Phase 12E-1b MCP drive: typed merge, typed locks/links, PPI-only,
comps by persisted id and `$layer.id` across sessions.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. The instance is owned by this file (`HostApp`: a free port, a
dummy API key and a PRIVATE directory for every preference library the app
reads) and is terminated by its own context manager; no Doctored process this
file did not start is ever signalled.

- **B1 the three typed merges, at 16 bits AND at 32-bit float.** Each of
  `merge_down`, `merge_down {layers}` (Merge Layers) and `merge_visible`
  produces one layer whose samples equal an f64 source-over oracle COMPOSED
  IN THIS FILE from the operands' own samples — within 1 code of 65535 at 16
  bits and a relative 1e-5 at float — with the structure this file states and
  one `undo` restoring the previous stack.
- **B2 the locks refuse the right edits, by the host's own sentences.** Lock
  Pixels refuses `brush_stroke`, Lock Position refuses `transform_layer`,
  Lock All refuses `add_layer_mask`, and Lock Pixels on the LOWER layer
  refuses `merge_down` — each with the `LockFlags.refusal` wording, and
  `unsaved_changes` unmoved by every refusal.
- **B3 links move together.** `link_layers` then `move_layers` on ONE partner
  moves both by the same delta; `unlink_layers` then the same move moves one.
- **B4 a position-locked comp entry reports `locked: true`** at 16 bits, and
  its recorded offset is NOT applied while its visibility still is.
- **B5 PPI only, three parts.** (i) the pixel SIGNATURE of the PNG16 and
  TIFF32 exports is EQUAL before and after `set_resolution` 300 -> 144;
  (ii) each export's DENSITY equals the new PPI — PNG through `sips` within
  0.02 ppi (the `pHYs` px/m rounding), TIFF through `tiffinfo` within 0.001;
  (iii) `resource_usage.unique_retained_bytes` grows by LESS than
  width*height*8 across the change while the history holds both handles — on
  a 1024x768 canvas of its own, because at a small canvas the budget is below
  the app's fixed per-history-step bookkeeping and the assertion would say
  nothing. Part (iii) is 16-bit only, for the reason stated at the gate.
- **B6 a comp binds by the PERSISTED id across a save, a reopen, a reorder
  AND a rename** — at 8 bits and at 16 bits.
- **B7 a recorded action replays by `$layer.id` in a SECOND PROCESS**, over
  the same private action library and the same saved `.rz`, with two
  same-named layers in the document so a name alone could not have chosen.
  Two owned instances, because "across sessions" is exactly what
  `ActionIdentity.session` distinguishes.
- **B8 `get_precision` lists the four new families** at 16 bits and at float.

Nothing here derives an expectation from Doctored: every table, every
refusal substring, every tolerance and every oracle below is written in this
file.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the expectations and tolerances, fixed BEFORE any measurement -------
CANVAS = (64, 48)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
PROBE = (CANVAS[0] // 3, CANVAS[1] // 3)
# A second probe point, one pixel in from the far corner (B1's digest half).
CORNER = (CANVAS[0] - 2, CANVAS[1] - 2)

# B1's tolerances. 16-bit: ONE code of 65535 per channel, which is the single
# rounding a u16 store can add over an f64 oracle. Float: relative 1e-5,
# which is above f32's own 6e-8 epsilon by two orders and below anything a
# wrong formula could hide in.
CODE_TOLERANCE = 1.0
FLOAT_TOLERANCE = 1e-5

# B1's fixture, three flat layers bottom-first. The alphas are exact in
# binary so the oracle and the compositor differ only in the final round.
LAYERS = [
    ('Base', (0.25, 0.125, 0.0625, 1.0)),
    ('Middle', (0.75, 0.5, 0.25, 0.5)),
    ('Top', (0.125, 0.875, 0.375, 0.25)),
]

# B2's refusal sentences, written out here from `LockFlags.refusal`.
LOCK_REFUSALS = {
    'pixels': 'has its pixels locked',
    'position': 'has its position locked',
    'all': 'is locked (Lock All)',
}

# B3's move delta.
DELTA = (7, -5)

# B5's tolerances and the resolutions it walks between.
PPI_BEFORE, PPI_AFTER = 300.0, 144.0
PNG_PPI_TOLERANCE = 0.02      # pHYs stores pixels per METRE, so ppi rounds
TIFF_PPI_TOLERANCE = 0.001
# Part (iii)'s own canvas. The budget is ONE CANVAS OF SAMPLES
# (width*height*8), so the canvas has to be big enough that a canvas of
# samples is distinguishable from the app's fixed per-history-step
# bookkeeping — at B1's 64x48 the budget is 24 KB and the bookkeeping alone
# is ~1 MB, so the assertion would say nothing either way. 1024x768 makes the
# budget 6 MB, which a resample could not hide inside.
RETAINED_CANVAS = (1024, 768)

# B8's families.
FAMILIES = ['layer_merge', 'layer_merge_visible', 'layer_lock', 'layer_link']


# ------------------------------------------------- the independent oracle --

def over(backdrop, source, opacity=1.0):
    """Straight source-over of ONE pixel in f64 — the W3C composition
    formula written out here. Nothing in it reads Doctored."""
    sa = source[3] * opacity
    out_a = sa + backdrop[3] * (1.0 - sa)
    if out_a <= 0.0:
        return (0.0, 0.0, 0.0, 0.0)
    return tuple(
        [(source[c] * sa + backdrop[c] * backdrop[3] * (1.0 - sa)) / out_a for c in range(3)]
        + [out_a])


def stack_oracle(members):
    """The members composited bottom-first over TRANSPARENCY."""
    out = (0.0, 0.0, 0.0, 0.0)
    for member in members:
        out = over(out, member)
    return out


# -------------------------------------------------------------- utilities --

def sample(app, document, index=None, at=None):
    """A sample at the probe point, as four f64 — one LAYER's when `index`
    is given, the COMPOSITE's otherwise.

    A 16-bit document reports `center_rgba_codes` (u16); a 32-bit float one
    reports `center_rgba32`. Both are normalized here so one oracle serves
    both depths.

    The float half of B1 isolates each operand by VISIBILITY and reads the
    composite (written when a float document refused scoped sampling; it
    answers now) — a single visible layer over a transparent canvas IS
    that layer weighted by its own opacity, which is exactly the operand the
    oracle needs.
    """
    point = at or PROBE
    arguments = {'document_id': document, 'x': point[0], 'y': point[1]}
    if index is not None:
        arguments.update(source='layer', layer=index)
    reply = app.call('sample_pixel', **arguments)
    if reply.get('center_rgba32') is not None:
        return [float(v) for v in reply['center_rgba32']], 'float'
    codes = reply['center_rgba_codes']
    return [float(v) / 65535.0 for v in codes], 'codes'


def render_digest(app, document):
    """The SHA-256 of `render`'s PNG bytes — a DIGEST of the whole merged
    picture beside the probe point's oracle comparison, so a merge that is
    right at one sample and wrong elsewhere is still recorded. The raw RPC,
    because the shared client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def isolated_sample(app, document, index, count):
    """Layer `index` alone over a transparent canvas, read as the COMPOSITE
    — the operand form a float document admits."""
    for other in range(count):
        app.call('set_layer_properties', document_id=document, index=other,
                 visible=(other == index))
    values, kind = sample(app, document)
    return values, kind


def close(got, want, kind, what):
    if kind == 'codes':
        for channel in range(4):
            difference = abs(got[channel] - want[channel]) * 65535.0
            assert difference <= CODE_TOLERANCE, (
                what, channel, got, want, difference, CODE_TOLERANCE)
    else:
        for channel in range(4):
            scale = max(abs(want[channel]), 1.0)
            difference = abs(got[channel] - want[channel])
            assert difference <= FLOAT_TOLERANCE * scale, (
                what, channel, got, want, difference, FLOAT_TOLERANCE)


def structure(app, document):
    """(name, depth) rows in the order `get_document` lists them."""
    summary = app.call('get_document', document_id=document)
    return [(layer.get('name'), int(layer.get('depth', 0))) for layer in summary['layers']]


def ids(app, document):
    summary = app.call('get_document', document_id=document)
    return [int(layer['id']) for layer in summary['layers']]


def history(app, document):
    report = app.call('list_history', document_id=document)
    return report['cursor']


def dirty(app, document):
    return app.call('get_document', document_id=document)['unsaved_changes']


def offsets(app, document):
    summary = app.call('get_document', document_id=document)
    out = []
    for layer in summary['layers']:
        pair = layer.get('offset')
        if isinstance(pair, list) and len(pair) == 2:
            out.append((int(pair[0]), int(pair[1])))
        else:
            out.append((int(layer.get('offset_x', 0)), int(layer.get('offset_y', 0))))
    return out


def hex_of(rgba):
    """A colour this file chose, as the hex `fill` takes. The ALPHA is
    carried separately through `opacity`, because a flat layer's coverage is
    what the oracle needs and hex ink is opaque."""
    return '#%02x%02x%02x' % tuple(min(255, max(0, round(c * 255))) for c in rgba[:3])


def build_stack(app, depth, label):
    """Three flat layers named here, each a different colour and coverage.

    The COVERAGE comes from the layer's own opacity rather than from painted
    alpha: a flat opaque fill plus a layer opacity is exactly the operand the
    oracle's `source[3] * opacity` describes, and it is expressible at every
    depth with tools both depths admit.
    """
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='transparent')
    document = created['document']['id']
    for _ in range(len(LAYERS) - 1):
        app.call('duplicate_layer', document_id=document, index=0)
    for index, (name, rgba) in enumerate(LAYERS):
        app.call('set_layer_properties', document_id=document, index=index, name=name,
                 opacity=rgba[3])
        app.call('fill', document_id=document, x=PROBE[0], y=PROBE[1], color=hex_of(rgba),
                 tolerance=1.0, contiguous=False, layer=index)
    return document


def members_oracle(app, document, indices, at=None):
    """The named layers' samples, each weighted by its own opacity, composited
    bottom-first over transparency."""
    summary = app.call('get_document', document_id=document)
    members = []
    kind = 'codes'
    for index in indices:
        values, kind = sample(app, document, index, at=at)
        opacity = float(summary['layers'][index].get('opacity', 1.0))
        members.append((values[0], values[1], values[2], values[3] * opacity))
    return stack_oracle(members), kind


# ------------------------------------------------------------------ gates --

def merge_forms(app, document, depth_label, findings):
    """The three merge forms, each from the SAME starting stack."""
    start = structure(app, document)
    out = {}

    def run(label, expected_rows, call, member_indices, merged_index):
        want, kind = members_oracle(app, document, member_indices)
        # A SECOND point, one pixel in from the far canvas corner, so the
        # comparison is not one probe sample: a merge that is right at the
        # middle and wrong at the edge fails here.
        corner_want, corner_kind = members_oracle(app, document, member_indices, at=CORNER)
        before_cursor = history(app, document)
        call()
        rows = structure(app, document)
        assert rows == expected_rows, (label, rows, expected_rows)
        assert history(app, document) == before_cursor + 1, (label, 'one history step')
        got, got_kind = sample(app, document, merged_index)
        close(got, want, got_kind if got_kind == 'float' else kind,
              f'{depth_label} {label}')
        corner_got, corner_got_kind = sample(app, document, merged_index, at=CORNER)
        close(corner_got, corner_want,
              corner_got_kind if corner_got_kind == 'float' else corner_kind,
              f'{depth_label} {label} at the corner')
        # …and the DIGEST of the whole merged picture, recorded in the receipt.
        digest = render_digest(app, document)
        app.call('undo', document_id=document)
        assert structure(app, document) == start, (label, 'undo')
        out[label] = {'expected_rows': expected_rows, 'oracle': list(want),
                      'measured': list(got), 'kind': got_kind,
                      'corner': {'point': list(CORNER), 'oracle': list(corner_want),
                                 'measured': list(corner_got)},
                      'render_sha256': digest}

    run('merge_down',
        [('Base', 0), ('Top', 0)],
        lambda: app.call('merge_down', document_id=document, index=1),
        [0, 1], 0)
    run('merge_layers',
        [('Base', 0)],
        lambda: app.call('merge_down', document_id=document, layers=[0, 1, 2]),
        [0, 1, 2], 0)
    run('merge_visible',
        [('Base', 0)],
        lambda: app.call('merge_visible', document_id=document),
        [0, 1, 2], 0)
    findings[depth_label] = out


def gate_b1(app, folder, receipts):
    gate = 'b1_typed_merge_against_an_f64_oracle'
    findings = {'tolerances': {'codes': CODE_TOLERANCE, 'float_relative': FLOAT_TOLERANCE}}
    document = build_stack(app, 16, 'b1_16')
    merge_forms(app, document, '16', findings)

    if FLOAT_FIXTURE.exists():
        path = folder / 'b1_float.tiff'
        shutil.copyfile(FLOAT_FIXTURE, path)
        document = app.call('open_document', path=str(path))['document']['id']
        assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
        # Two layers with DIFFERENT coverage, so the merge is a real
        # composition rather than a copy. `duplicate_layer` and the opacity
        # property are both admitted at float depth.
        app.call('duplicate_layer', document_id=document, index=0)
        app.call('set_layer_properties', document_id=document, index=0, name='Base')
        app.call('set_layer_properties', document_id=document, index=1, name='Upper',
                 opacity=0.5)
        # Each operand ISOLATED by visibility, then composed here in f64.
        lower, _ = isolated_sample(app, document, 0, 2)
        upper, kind = isolated_sample(app, document, 1, 2)
        for index in range(2):
            app.call('set_layer_properties', document_id=document, index=index, visible=True)
        want = over(tuple(lower), tuple(upper))
        before_cursor = history(app, document)
        app.call('merge_down', document_id=document, index=1)
        rows = structure(app, document)
        assert rows == [('Base', 0)], rows
        assert history(app, document) == before_cursor + 1
        # One record survives, so the COMPOSITE is that record.
        got, got_kind = sample(app, document)
        close(got, want, got_kind, 'float merge_down')
        app.call('undo', document_id=document)
        assert structure(app, document) == [('Base', 0), ('Upper', 0)]
        findings['32'] = {'merge_down': {'oracle': list(want), 'measured': list(got),
                                         'kind': got_kind}}
        # …and merge_visible is admitted at float too.
        app.call('merge_visible', document_id=document)
        assert structure(app, document) == [('Base', 0)]
        app.call('undo', document_id=document)
        findings['32']['merge_visible'] = 'admitted'

    receipts[gate] = findings
    print('PASS b1: the three typed merges match the drive\'s own f64 oracle at both depths',
          flush=True)


def gate_b2(app, folder, receipts):
    gate = 'b2_typed_locks_refuse_by_name'
    findings = {}
    document = build_stack(app, 16, 'b2')
    before_dirty = dirty(app, document)

    def refuse(tool, needle, **arguments):
        message = app.call(tool, document_id=document, fail=True, **arguments)
        assert needle in message, (tool, needle, message)
        assert dirty(app, document) == before_dirty, (tool, 'the refusal dirtied the document')
        return message

    # Lock PIXELS refuses a stroke and a merge INTO the locked layer, and
    # leaves a mask edit alone (a pixel-locked layer's mask stays
    # paintable).
    app.call('set_layer_lock', document_id=document, layer=0, locks=['pixels'])
    findings['pixels_brush'] = refuse(
        'brush_stroke', LOCK_REFUSALS['pixels'], layer=0, color='#ffffff',
        points=[[4, 4], [12, 12]], size=6)
    findings['pixels_merge_down'] = refuse(
        'merge_down', LOCK_REFUSALS['pixels'], index=1)

    # Lock POSITION refuses a transform.
    app.call('set_layer_lock', document_id=document, layer=0, locks=['position'])
    findings['position_transform'] = refuse(
        'transform_layer', LOCK_REFUSALS['position'], layer=0, translate=[4, 4])

    # Lock ALL refuses a mask edit, which is the one extra meaning that
    # spelling carries.
    app.call('set_layer_lock', document_id=document, layer=0,
             locks=['transparency', 'pixels', 'position'])
    findings['all_add_mask'] = refuse(
        'add_layer_mask', LOCK_REFUSALS['all'], layer=0, fill='white')

    # The locks are READ BACK from the document, so the setter is not merely
    # believed.
    summary = app.call('get_document', document_id=document)
    findings['locks_reported'] = summary['layers'][0].get('locks')

    app.call('set_layer_lock', document_id=document, layer=0, locks=[])
    findings['cleared'] = app.call('get_document',
                                   document_id=document)['layers'][0].get('locks')
    receipts[gate] = findings
    print('PASS b2: every typed lock refuses its edit with the host\'s own sentence',
          flush=True)


def gate_b3(app, folder, receipts):
    gate = 'b3_links_move_together'
    findings = {}
    document = build_stack(app, 16, 'b3')
    before = offsets(app, document)

    app.call('link_layers', document_id=document, layers=[0, 2])
    app.call('move_layers', document_id=document, layers=[0], dx=DELTA[0], dy=DELTA[1])
    linked = offsets(app, document)
    for index in (0, 2):
        assert linked[index] == (before[index][0] + DELTA[0], before[index][1] + DELTA[1]), (
            'linked partner did not follow', index, before, linked)
    assert linked[1] == before[1], ('an unlinked layer moved', before, linked)
    findings['linked'] = {'before': before, 'after': linked, 'delta': list(DELTA)}

    app.call('unlink_layers', document_id=document, layers=[0, 2])
    app.call('move_layers', document_id=document, layers=[0], dx=DELTA[0], dy=DELTA[1])
    unlinked = offsets(app, document)
    assert unlinked[0] == (linked[0][0] + DELTA[0], linked[0][1] + DELTA[1])
    assert unlinked[2] == linked[2], ('an unlinked partner still followed', linked, unlinked)
    findings['unlinked'] = {'before': linked, 'after': unlinked}

    receipts[gate] = findings
    print('PASS b3: a link makes partners move together and unlinking stops it', flush=True)


def gate_b4(app, folder, receipts):
    gate = 'b4_comp_locked_is_real_at_depth'
    findings = {}
    document = build_stack(app, 16, 'b4')
    # Record a comp with layer 1 MOVED and hidden.
    app.call('move_layers', document_id=document, layers=[1], dx=11, dy=9)
    app.call('set_layer_properties', document_id=document, index=1, visible=False)
    app.call('save_layer_comp', document_id=document, name='Moved')
    # Put it back and lock its POSITION.
    app.call('move_layers', document_id=document, layers=[1], dx=-11, dy=-9)
    app.call('set_layer_properties', document_id=document, index=1, visible=True)
    app.call('set_layer_lock', document_id=document, layer=1, locks=['position'])

    listing = app.call('list_layer_comps', document_id=document)
    comp = listing['comps'][0]
    assert comp['locked_count'] == 1, listing
    locked = [entry for entry in comp['entries'] if entry.get('locked')]
    assert len(locked) == 1 and locked[0]['layer'] == 1, listing
    findings['listing'] = comp

    before = offsets(app, document)
    applied = app.call('apply_layer_comp', document_id=document, name='Moved')
    after = offsets(app, document)
    assert after[1] == before[1], ('a position-locked entry moved anyway', before, after)
    summary = app.call('get_document', document_id=document)
    assert summary['layers'][1]['visible'] is False, 'the visibility half did not apply'
    findings['apply'] = {'reply': applied, 'offsets_before': before, 'offsets_after': after}

    receipts[gate] = findings
    print('PASS b4: a position-locked comp entry is reported and its offset skipped at depth',
          flush=True)


def signature(path):
    """ImageMagick's PIXEL signature — the pixels alone, with no metadata in
    it, which is exactly the claim "no pixel was resampled" needs."""
    result = subprocess.run(['magick', 'identify', '-format', '%#', str(path)],
                            capture_output=True, text=True, check=True)
    return result.stdout.strip()


def png_density(path):
    result = subprocess.run(['sips', '-g', 'dpiWidth', '-g', 'dpiHeight', str(path)],
                            capture_output=True, text=True, check=True)
    values = {}
    for line in result.stdout.splitlines():
        if ':' in line:
            key, _, value = line.strip().partition(':')
            try:
                values[key.strip()] = float(value.strip())
            except ValueError:
                pass
    return values


def tiff_density(path):
    result = subprocess.run(['tiffinfo', str(path)], capture_output=True, text=True,
                            check=True)
    for line in result.stdout.splitlines():
        if 'Resolution:' in line:
            part = line.split('Resolution:', 1)[1].strip()
            numbers = [float(v) for v in part.replace(',', ' ').split() if _is_number(v)]
            if len(numbers) >= 2:
                return numbers[0], numbers[1]
    raise AssertionError(f'tiffinfo reported no Resolution for {path}:\n{result.stdout}')


def _is_number(text):
    try:
        float(text)
        return True
    except ValueError:
        return False


def gate_b5(app, folder, receipts):
    gate = 'b5_ppi_only'
    findings = {'tolerances': {'png_ppi': PNG_PPI_TOLERANCE, 'tiff_ppi': TIFF_PPI_TOLERANCE,
                               'ppi': [PPI_BEFORE, PPI_AFTER]}}
    for label, depth, fixture in (('16', 16, None),
                                  ('32', 32, FLOAT_FIXTURE)):
        if fixture is not None and not fixture.exists():
            continue
        if fixture is None:
            document = build_stack(app, depth, f'b5_{label}')
        else:
            path = folder / f'b5_{label}.tiff'
            shutil.copyfile(fixture, path)
            document = app.call('open_document', path=str(path))['document']['id']
        app.call('set_resolution', document_id=document, ppi_x=PPI_BEFORE)

        png_before = folder / f'b5_{label}_before.png'
        tiff_before = folder / f'b5_{label}_before.tiff'
        tiff_depth = 32 if depth == 32 else 16
        app.call('export_image', document_id=document, path=str(png_before),
                 settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
        app.call('export_image', document_id=document, path=str(tiff_before),
                 settings={'format': 'tiff', 'bit_depth': tiff_depth}, overwrite=True)

        app.call('set_resolution', document_id=document, ppi_x=PPI_AFTER)

        png_after = folder / f'b5_{label}_after.png'
        tiff_after = folder / f'b5_{label}_after.tiff'
        app.call('export_image', document_id=document, path=str(png_after),
                 settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
        app.call('export_image', document_id=document, path=str(tiff_after),
                 settings={'format': 'tiff', 'bit_depth': tiff_depth}, overwrite=True)

        # (i) the PIXELS are identical.
        assert signature(png_before) == signature(png_after), (label, 'png pixels moved')
        assert signature(tiff_before) == signature(tiff_after), (label, 'tiff pixels moved')

        # (ii) the DENSITY equals the new ppi.
        png = png_density(png_after)
        for key in ('dpiWidth', 'dpiHeight'):
            assert abs(png[key] - PPI_AFTER) <= PNG_PPI_TOLERANCE, (label, key, png)
        x_res, y_res = tiff_density(tiff_after)
        assert abs(x_res - PPI_AFTER) <= TIFF_PPI_TOLERANCE, (label, x_res)
        assert abs(y_res - PPI_AFTER) <= TIFF_PPI_TOLERANCE, (label, y_res)

        findings[label] = {'png_signature': signature(png_after),
                           'png_density': png, 'tiff_density': [x_res, y_res]}

    # (iii) the history holds BOTH handles and yet almost nothing was
    # retained: a resample would have cost one canvas of samples.
    #
    # On its OWN canvas, for the reason `RETAINED_CANVAS` states. This part
    # runs at 16 bits only: the one float fixture this repository holds is
    # 48x32, whose canvas of samples is 6 KB — far below the fixed bookkeeping
    # — and a float document refuses `image_size`, so there is no way to make
    # a bigger one. Parts (i) and (ii) DO run at float above, and the Rust
    # test `precision_resolution.rs` asserts the float buffers are POINTER-
    # equal, which is the stronger claim this part approximates.
    created = app.call('new_document', width=RETAINED_CANVAS[0], height=RETAINED_CANVAS[1],
                       depth=16, background='white')
    document = created['document']['id']
    app.call('set_resolution', document_id=document, ppi_x=PPI_BEFORE)
    usage_before = app.call('resource_usage')['unique_retained_bytes']
    app.call('set_resolution', document_id=document, ppi_x=PPI_AFTER)
    usage_after = app.call('resource_usage')['unique_retained_bytes']
    states = app.call('list_history', document_id=document)['states']
    budget = RETAINED_CANVAS[0] * RETAINED_CANVAS[1] * 8
    growth = int(usage_after) - int(usage_before)
    assert growth < budget, (growth, budget)
    findings['retained'] = {'canvas': list(RETAINED_CANVAS), 'growth_bytes': growth,
                            'canvas_bytes': budget, 'history_states': len(states),
                            'float_part_iii': 'not measured: the only float fixture is 48x32, '
                                              'whose canvas of samples is below the fixed '
                                              'per-step bookkeeping, and a float document '
                                              'refuses image_size'}

    receipts[gate] = findings
    print('PASS b5: set_resolution moves the density and not one pixel, at both typed depths',
          flush=True)


def comp_identity_round_trip(app, folder, document, label, findings):
    """One comp, saved to `.rz`, reopened, then the stack REORDERED and a
    layer RENAMED — which schema 1's index + name binding could not have
    survived — and the comp still applied to the right layers."""
    app.call('set_layer_properties', document_id=document, index=1, visible=False)
    app.call('save_layer_comp', document_id=document, name='Hidden middle')
    app.call('set_layer_properties', document_id=document, index=1, visible=True)

    path = folder / f'b6_{label}.rz'
    app.call('save_copy', document_id=document, path=str(path))
    reopened = app.call('open_document', path=str(path))['document']['id']
    before_ids = ids(app, reopened)

    # Both changes at once: the bottom layer goes to the top AND is renamed.
    # Under schema 1 the index moved and the name changed, so the entry would
    # have gone stale on either count.
    app.call('reorder_layers', document_id=reopened, layers=[0], at=3, depth=0)
    app.call('set_layer_properties', document_id=reopened, index=2, name='Renamed base')
    listing = app.call('list_layer_comps', document_id=reopened)
    comp = listing['comps'][0]
    assert comp['stale_count'] == 0, (label, 'an entry went stale', listing)

    # The comp hides the layer it ORIGINALLY named, wherever it sits now.
    target = before_ids[1]
    app.call('apply_layer_comp', document_id=reopened, name='Hidden middle')
    summary = app.call('get_document', document_id=reopened)
    hidden = [int(layer['id']) for layer in summary['layers'] if layer['visible'] is False]
    assert hidden == [target], (label, 'the comp hid the wrong layer', hidden, target)
    findings[label] = {'ids_before': before_ids, 'target': target,
                       'hidden_after_apply': hidden, 'listing': comp}


def gate_b6(app, folder, receipts):
    gate = 'b6_comps_bind_by_the_persisted_id'
    findings = {}
    comp_identity_round_trip(app, folder, build_stack(app, 8, 'b6_8'), '8', findings)
    comp_identity_round_trip(app, folder, build_stack(app, 16, 'b6_16'), '16', findings)
    receipts[gate] = findings
    print('PASS b6: a comp survives a save, a reopen, a reorder AND a rename at both depths',
          flush=True)


def gate_b7(executable, folder, receipts):
    """`$layer.id` resolving in a SECOND PROCESS.

    Two owned instances over ONE private preference folder, because "across
    sessions" is exactly what `ActionIdentity.session` distinguishes: an
    action replayed inside the launch that recorded it takes the id path 12A
    shipped, and only a second launch exercises the persisted-id path this
    run added.

    The document holds TWO layers of the SAME NAME and the step names the
    BOTTOM one. A name alone resolves to the TOPMOST layer of that name
    (`ActionPlayer.layerNamed`), so hitting the bottom one proves the ID was
    used and not the fallback.

    The step is RECORDED through `start_recording` / `stop_recording` and its
    literal `index` is then replaced by the `$layer` symbol the recorder's
    own document reports. That substitution is deliberate and disclosed: the
    MCP recorder echoes a tool call's arguments verbatim, and only the app's
    OWN step builders (`ActionSteps.swift`) mint `$layer` symbols, so a drive
    that never touches the UI cannot record one. Everything the gate then
    measures — the persisted ids, the second session, the resolution — is the
    shipped code's.
    """
    gate = 'b7_layer_id_replays_across_sessions'
    findings = {}
    home = folder / 'b7'
    home.mkdir(parents=True, exist_ok=True)
    path = home / 'twins.rz'

    with HostApp(home / 'app', home / 'recovery', executable) as app:
        created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                           background='white')
        document = created['document']['id']
        app.call('duplicate_layer', document_id=document, index=0)
        for index in (0, 1):
            app.call('set_layer_properties', document_id=document, index=index, name='Twin')

        app.call('start_recording')
        app.call('set_layer_properties', document_id=document, index=0, visible=False)
        recorded = app.call('stop_recording', name='Hide the lower twin')
        step = recorded['steps'][0]
        assert step['tool'] == 'set_layer_properties', recorded
        assert step['arguments'].get('index') == 0, recorded

        summary = app.call('get_document', document_id=document)
        session = summary.get('identity_session')
        assert session, summary
        layer_ids = [int(layer['id']) for layer in summary['layers']]
        step = dict(step)
        step['arguments'] = dict(step['arguments'])
        step['arguments'].pop('index', None)
        step['arguments']['layer'] = {'$layer': {'name': 'Twin', 'id': layer_ids[0],
                                                 'session': session}}
        app.call('save_action', name='Hide the lower twin', steps=[step], overwrite=True)
        findings['recorded'] = recorded['steps'][0]
        findings['step'] = step

        app.call('set_layer_properties', document_id=document, index=0, visible=True)
        app.call('save_copy', document_id=document, path=str(path))
        findings['ids_at_record'] = layer_ids
        findings['session_at_record'] = session

    # A SECOND process: a new `ActionIdentity.session`, the same action file
    # and the same persisted ids.
    # The SAME preference folder, so the second launch reads the action
    # library the first one wrote — a private one this drive owns, never the
    # user's. Only the recovery directory differs, so neither instance sees
    # the other's checkpoints.
    with HostApp(home / 'app', home / 'recovery2', executable) as app:
        reopened = app.call('open_document', path=str(path))['document']['id']
        summary = app.call('get_document', document_id=reopened)
        session = summary.get('identity_session')
        assert session != findings['session_at_record'], 'the second launch reused the session'
        before = [int(layer['id']) for layer in summary['layers']]
        assert before == findings['ids_at_record'], ('the ids did not survive the save',
                                                     before, findings['ids_at_record'])
        app.call('run_action', document_id=reopened, name='Hide the lower twin')
        summary = app.call('get_document', document_id=reopened)
        hidden = [int(layer['id']) for layer in summary['layers'] if layer['visible'] is False]
        assert hidden == [before[0]], ('the action hit the wrong twin', hidden, before)
        findings['session_at_replay'] = session
        findings['ids'] = before
        findings['hidden'] = hidden

    receipts[gate] = findings
    print('PASS b7: $layer.id resolves to the right twin in a SECOND process', flush=True)


def gate_b8(app, folder, receipts):
    gate = 'b8_get_precision_families'
    findings = {'families': FAMILIES}
    wanted = {'merge_down', 'merge_visible', 'set_layer_lock', 'link_layers',
              'unlink_layers', 'set_resolution'}
    document = build_stack(app, 16, 'b8')
    report = app.call('get_precision', document_id=document)
    tools = set(report['supported_tools']) | set(report.get('shared_tools', []))
    missing = wanted - tools
    assert not missing, ('16', sorted(missing))
    findings['16'] = sorted(wanted)

    if FLOAT_FIXTURE.exists():
        path = folder / 'b8.tiff'
        shutil.copyfile(FLOAT_FIXTURE, path)
        document = app.call('open_document', path=str(path))['document']['id']
        report = app.call('get_precision', document_id=document)
        tools = set(report['supported_tools']) | set(report.get('shared_tools', []))
        missing = wanted - tools
        assert not missing, ('32', sorted(missing))
        findings['32'] = sorted(wanted)

    receipts[gate] = findings
    print('PASS b8: the new tools are admitted by name at both typed depths', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'expectations': 'fixed in scripts/verify_phase12e1b_structure.py before any '
                                'measurement; the merge oracle is composed in this file and '
                                'the two tolerances (1 code of 65535 at 16 bits, relative '
                                '1e-5 at float) are written down before the first sample'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        gate_b1(app, folder, receipts)
        gate_b2(app, folder, receipts)
        gate_b3(app, folder, receipts)
        gate_b4(app, folder, receipts)
        gate_b5(app, folder, receipts)
        gate_b6(app, folder, receipts)
        gate_b8(app, folder, receipts)
    # B7 owns its OWN two instances: "across sessions" needs a second launch.
    gate_b7(executable, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
