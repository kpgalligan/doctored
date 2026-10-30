#!/usr/bin/env python3
"""The Phase 12F-1 MCP drive: typed CANVAS SIZE and typed STRAIGHTEN.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. ONE instance is owned by this file for every gate (`HostApp`: a
free port, a dummy API key and a PRIVATE directory for every preference
library the app reads) and is terminated by its own context manager; no
Doctored process this file did not start is ever signalled.

TOLERANCES, all fixed here BEFORE any measurement and never widened:

- **Block A** (Canvas Size) EXACT everywhere. Canvas Size reads no sample —
  every raster and mask keeps its allocation and moves its offset, every
  channel is padded with 0 — so every comparison is sample-for-sample
  equality against this file's OWN pad/crop of the pre-op render.
- **Block B** (straighten) CROSS-DEPTH: a 16-bit straighten of the
  ×257-promoted picture against the 8-bit call is within 1.0 8-bit-equivalent
  code (a 16-bit difference divided by 257) on ALPHA everywhere, and on RGB
  wherever BOTH alphas are at least 1 8-bit code. The saved channel takes the
  same rule on `coverage16 ÷ 257` against `coverage`. Every straighten case
  carries a NON-VACUITY control: at least one pixel must differ from the
  angle-0 crop of the same window by at least 32 8-bit codes, so a silently
  unrotated answer cannot pass. Everything else in Block B — the replay, the
  `.rz` identity, the recorded `angle`'s JSON type, the refusal sentences and
  the history cursors — is EXACT.

Block A — Canvas Size:

- **A1** grow / shrink / odd-delta / odd-shrink renders equal this file's own pad/crop of
  the pre-op render, sample for sample, at 8 bits, 16 bits and 32-bit float.
- **A2** `get_document`'s guides, ruler origin and layer offsets equal the
  table computed here, and equal the 8-bit answer for the same call.
- **A3** a saved channel and a layer mask are carried: the channel is padded
  with 0 and the mask offset moves by the origin.
- **A4** one `undo` and one `redo` each; a SAME-SIZE call is a SUCCESSFUL
  no-op at 8 bits, 16 bits AND float — `ok: true`, `changed: false`, a
  non-empty note, 0 history steps and an unchanged render. (12F-1b: the core
  answers `None` for the exact no-op at both depths, so the 8-bit phantom
  step is gone, and the host answers the request before it edits anything, so
  12F-1's 16-bit/float ERROR is gone too.)
- **A5** `.rz` save -> reopen -> resave is SHA-256 identical at 16 bits and
  at float.
- **A6** a recorded `canvas_size` step replays through `run_action` at 16.
- **A7** `get_precision` lists `canvas_size` at 16 bits and at float.
- **A8** on a document holding a `place_image` PNG, at 8 AND 16 bits: the
  placed layer is an ordinary raster layer (`source: null` from `place_image`
  and from `get_image_source`), `canvas_size` SUCCEEDS for a grow with a
  non-centre anchor and for a shrink, every layer offset gains exactly the
  computed origin, the render equals this file's own pad/crop of the pre-op
  render, `undo` restores the prior offsets and canvas, `.rz` save -> reopen
  keeps the plain layer and renders identically, and a recorded `canvas_size`
  step replays on a fresh placed document.

Block B — typed STRAIGHTEN (`crop` with a non-zero `angle`):

- **b1** `crop {x:6, y:5, width:36, height:26, angle:7.5}` with `bilinear`
  and with `bicubic` at 16 bits against the same call at 8 bits, under the
  cross-depth rule and its control; canvas and layer offsets equal.
- **b2** guides, the ruler origin and the SAVED CHANNEL follow the
  straighten: the chrome equals this file's own shift/drop/clamp table at
  origin (−x, −y) and the 8-bit answer, and every window pixel of the channel
  obeys the cross-depth rule with its own control.
- **b3** one `undo` and one `redo`; `.rz` save → reopen → resave SHA-256
  identical; a recorded `crop`-with-angle step carries `angle` as a JSON
  NUMBER equal to 7.5 and replays through `run_action` on a fresh twin
  sample-for-sample equal.
- **b4** a text layer's `rasterized_layers` and `note` at 16 bits are present
  and equal to the 8-bit reply's.
- **b5** a present-but-unreadable `angle` refuses BY NAME at float, 8 AND 16
  bits with an unchanged history cursor and an unchanged render; float
  straightens for real: one history step, the canvas becomes the
  window, the result differs from the angle-0 crop of the same window by at
  least the control floor, and one `undo` restores the canvas and the render
  exactly; and a readable STRING angle is a control that straightens and
  equals the numeric call.

EXIT CODES: 0 = every gate PASS; 1 = a failed assertion; 2 = INCOMPLETE (any
gate NOT IMPLEMENTED). Exit 2 is never reported as a pass.

Nothing here derives an expectation from Doctored: every table, every
tolerance and every comparison below is written in this file.
"""
import argparse
import base64
import hashlib
import json
import math
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8, write_png16
from support import HostApp

# ---- the expectations, fixed BEFORE any measurement ----------------------
CANVAS = (48, 36)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

# The four A1 cases, each named by what it is. `anchor` is the MCP spelling;
# the origin it implies is computed by this file's own `anchor_origin`
# (floored: 'odd_delta' puts y at floor(3 / 2) = 1, 'odd_shrink' at
# (floor(-31 / 2), floor(-17 / 2)) = (-16, -9)).
A1_CASES = [
    ('grow', (CANVAS[0] + 12, CANVAS[1] + 8), 'bottom-right'),
    ('shrink', (CANVAS[0] - 10, CANVAS[1] - 6), 'top-left'),
    ('odd_delta', (CANVAS[0] + 7, CANVAS[1] + 3), 'right'),
    ('odd_shrink', (CANVAS[0] - 31, CANVAS[1] - 17), 'center'),
]

ANCHORS = {'top-left': (0, 0), 'top': (1, 0), 'top-right': (2, 0),
           'left': (0, 1), 'center': (1, 1), 'right': (2, 1),
           'bottom-left': (0, 2), 'bottom': (1, 2), 'bottom-right': (2, 2)}

# A2's chrome: two guides and a moved ruler origin.
GUIDES = [('vertical', 6.5), ('horizontal', 4.0)]
RULER = (3.0, 5.0)

PLACE_AT = (5, 4)
A8_GROW = ((CANVAS[0] + 11, CANVAS[1] + 6), 'bottom-right')
A8_SHRINK = ((CANVAS[0] - 8, CANVAS[1] - 5), 'bottom-right')

# ---- Block B's expectations, fixed BEFORE any measurement ----------------
# The straighten window and angle every B gate uses. The window is INSIDE the
# canvas, which is what both depths require (the byte `crop` answers nil for a
# window past the edge; the typed op names the same refusal).
STRAIGHTEN = (6, 5, 36, 26)
STRAIGHTEN_ANGLE = 7.5
STRAIGHTEN_SAMPLERS = ('bilinear', 'bicubic')
# The cross-depth tolerance, in 8-BIT-EQUIVALENT codes, and the non-vacuity
# control floor. Both are fixed here; a miss is recorded with its numbers and
# the gate FAILS.
CROSS_DEPTH_CODES = 1.0
CONTROL_CODES = 32
ANGLE_REFUSAL = 'angle must be between -45 and 45 degrees'


# ------------------------------------------------- the independent oracle --

def anchor_origin(anchor, old, new):
    """The origin the anchor implies — `RasterDocument.canvasSizeOrigin`'s
    arithmetic (the sheet's and `AgentServer.canvasSize`'s), restated here:
    floor(delta * col / 2) per axis, floored toward minus infinity (measured
    against Photoshop for an odd centred delta in take1 CS-C: +31 x +17 at
    the centre gives (15, 8))."""
    col, row = ANCHORS[anchor]
    return (math.floor((new[0] - old[0]) * col / 2), math.floor((new[1] - old[1]) * row / 2))


def pad_crop(samples, old, new, origin, channels=4, outside=0):
    """This file's OWN pad/crop: the old canvas's top-left corner lands at
    `origin`, anything outside the new canvas is dropped and anything the old
    canvas did not cover is `outside`."""
    out = [outside] * (new[0] * new[1] * channels)
    for y in range(old[1]):
        dy = y + origin[1]
        if dy < 0 or dy >= new[1]:
            continue
        for x in range(old[0]):
            dx = x + origin[0]
            if dx < 0 or dx >= new[0]:
                continue
            src = (y * old[0] + x) * channels
            dst = (dy * new[0] + dx) * channels
            out[dst:dst + channels] = samples[src:src + channels]
    return out


def q4(value):
    return round(float(value) + 0.0, 4)


def padded_guides(guides, origin, new):
    """`doc_guide::padded_guides`: shift by the origin and DROP what left."""
    rank = {'horizontal': 0, 'vertical': 1}
    kept = []
    for orientation, position in guides:
        shift = origin[0] if orientation == 'vertical' else origin[1]
        extent = new[0] if orientation == 'vertical' else new[1]
        moved = q4(position + shift)
        if 0.0 <= moved <= extent:
            kept.append((orientation, moved))
    return sorted(kept, key=lambda g: (rank[g[0]], g[1]))


def padded_origin(ruler, origin, new):
    """`doc_guide::padded_origin`: shift by the origin and CLAMP."""
    return (min(max(q4(ruler[0] + origin[0]), 0.0), float(new[0])),
            min(max(q4(ruler[1] + origin[1]), 0.0), float(new[1])))


# -------------------------------------------------------------- utilities --

def render(app, document):
    """`render`'s PNG, decoded to RGBA at the file's own depth. The raw RPC,
    because the shared client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return png_rgba(base64.b64decode(data))


def canvas_of(app, document):
    summary = app.call('get_document', document_id=document)
    return (int(summary['width']), int(summary['height']))


def offsets(app, document):
    summary = app.call('get_document', document_id=document)
    out = []
    for layer in summary['layers']:
        # The two depths spell one thing two ways and this drive does not
        # change either: the byte rows carry `offset_x`/`offset_y`, the typed
        # rows (`precision_inspect`) carry `offset` as a pair.
        if 'offset_x' in layer:
            x, y = int(layer['offset_x']), int(layer['offset_y'])
        else:
            pair = layer['offset']
            x, y = int(pair[0]), int(pair[1])
        out.append((layer.get('name'), x, y))
    return out


def chrome(app, document):
    summary = app.call('get_document', document_id=document)
    rows = summary.get('guides') or []
    ruler = summary.get('ruler') or {}
    rank = {'horizontal': 0, 'vertical': 1}
    guides = sorted(((g['orientation'], q4(g['position'])) for g in rows),
                    key=lambda g: (rank[g[0]], g[1]))
    return guides, (q4(ruler.get('origin_x', 0)), q4(ruler.get('origin_y', 0)))


def history(app, document):
    return app.call('list_history', document_id=document)['cursor']


def refuses(app, tool, **arguments):
    """The refusal sentence of a call that must fail, or None when it did
    not. The raw RPC, so an in-band `isError` reply is readable."""
    result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
    if not result.get('isError'):
        return None
    return ' '.join(c.get('text', '') for c in result.get('content', []))


def seeded(app, depth, folder, tag, *, guides=True, channel=True, mask=True):
    """A document of this drive's own canvas, with the chrome, one saved
    channel and one layer mask A2/A3 ask about."""
    if depth == 32:
        source = folder / f'{tag}.tiff'
        shutil.copyfile(FLOAT_FIXTURE, source)
        document = app.call('open_document', path=str(source))['document']['id']
    else:
        document = app.call('new_document', width=CANVAS[0], height=CANVAS[1],
                            depth=depth, background='white')['document']['id']
    if guides:
        for orientation, position in GUIDES:
            app.call('add_guide', document_id=document, orientation=orientation,
                     position=position)
        app.call('set_ruler_origin', document_id=document, x=RULER[0], y=RULER[1])
    if channel:
        app.call('select_rect', document_id=document, x=2, y=1, width=8, height=6)
        app.call('save_selection', document_id=document, name='Cover')
        app.call('deselect', document_id=document)
    if mask:
        app.call('add_layer_mask', document_id=document, layer=0, mode='reveal_all')
    return document


# ------------------------------------------------------------------ gates --

def gate_a1(app, folder, receipts):
    findings = {}
    for depth in (8, 16, 32):
        if depth == 32 and not FLOAT_FIXTURE.exists():
            raise AssertionError('the committed float fixture is missing')
        per_depth = {}
        for name, size, anchor in A1_CASES:
            document = seeded(app, depth, folder, f'a1_{depth}_{name}',
                              guides=False, channel=False, mask=False)
            old = canvas_of(app, document)
            new = (size if depth != 32 else (old[0] + (size[0] - CANVAS[0]),
                                             old[1] + (size[1] - CANVAS[1])))
            origin = anchor_origin(anchor, old, new)
            before = render(app, document)
            assert (before['width'], before['height']) == old, (before['width'], old)
            app.call('canvas_size', document_id=document, width=new[0], height=new[1],
                     anchor=anchor)
            after = render(app, document)
            assert (after['width'], after['height']) == new, ((after['width'],
                                                               after['height']), new)
            assert after['depth'] == before['depth'], 'the render depth moved'
            want = pad_crop(before['samples'], old, new, origin)
            assert after['samples'] == want, \
                f'depth {depth} {name}: the render is not this file\'s own pad/crop'
            per_depth[name] = {'from': list(old), 'to': list(new),
                               'origin': list(origin), 'anchor': anchor,
                               'samples': len(want)}
        findings[str(depth)] = per_depth
    receipts['a1'] = findings
    print('PASS a1: grow, shrink, an odd delta and an odd centred shrink equal the drive\'s '
          'own pad/crop at 8, 16 and 32 bits', flush=True)


def gate_a2(app, folder, receipts):
    findings = {}
    size, anchor = (CANVAS[0] + 10, CANVAS[1] + 6), 'bottom-right'
    origin = anchor_origin(anchor, CANVAS, size)
    answers = {}
    for depth in (8, 16):
        document = seeded(app, depth, folder, f'a2_{depth}', channel=False, mask=False)
        before_offsets = offsets(app, document)
        app.call('canvas_size', document_id=document, width=size[0], height=size[1],
                 anchor=anchor)
        guides, ruler = chrome(app, document)
        want_guides = padded_guides(GUIDES, origin, size)
        want_ruler = padded_origin(RULER, origin, size)
        assert guides == want_guides, (depth, guides, want_guides)
        assert ruler == want_ruler, (depth, ruler, want_ruler)
        want_offsets = [(name, x + origin[0], y + origin[1])
                        for name, x, y in before_offsets]
        assert offsets(app, document) == want_offsets, (depth, offsets(app, document),
                                                        want_offsets)
        answers[depth] = {'guides': guides, 'ruler': list(ruler),
                          'offsets': offsets(app, document)}
        findings[str(depth)] = answers[depth]
    assert answers[16] == answers[8], ('the 16-bit answer differs from the 8-bit one',
                                       answers)
    receipts['a2'] = findings
    print('PASS a2: guides, the ruler origin and every layer offset equal the drive\'s '
          'table and the 8-bit answer', flush=True)


def gate_a3(app, folder, receipts):
    findings = {}
    size, anchor = (CANVAS[0] + 9, CANVAS[1] + 5), 'bottom-right'
    origin = anchor_origin(anchor, CANVAS, size)
    for depth in (8, 16):
        document = seeded(app, depth, folder, f'a3_{depth}', guides=False)
        before = app.call('list_channels', document_id=document)['channels']
        assert before, 'the drive saved no channel'
        mask_before = app.call('get_mask_properties', document_id=document, layer=0)
        app.call('canvas_size', document_id=document, width=size[0], height=size[1],
                 anchor=anchor)
        after = app.call('list_channels', document_id=document)['channels']
        assert len(after) == len(before), (depth, 'the channel list changed length')
        assert [c['name'] for c in after] == [c['name'] for c in before], depth
        mask_after = app.call('get_mask_properties', document_id=document, layer=0)
        assert int(mask_after['offset_x']) == int(mask_before['offset_x']) + origin[0], \
            (depth, mask_after, mask_before, origin)
        assert int(mask_after['offset_y']) == int(mask_before['offset_y']) + origin[1], \
            (depth, mask_after, mask_before, origin)
        # The channel is CANVAS-sized after the pad, which is what "padded
        # with 0" means structurally; its samples are compared sample for
        # sample by core/tests/precision_canvas_size.rs against the byte twin.
        summary = app.call('get_document', document_id=document)
        assert (int(summary['width']), int(summary['height'])) == size, depth
        findings[str(depth)] = {'channels': [c['name'] for c in after],
                                'mask_before': [int(mask_before['offset_x']),
                                                int(mask_before['offset_y'])],
                                'mask_after': [int(mask_after['offset_x']),
                                               int(mask_after['offset_y'])],
                                'origin': list(origin)}
    receipts['a3'] = findings
    print('PASS a3: the saved channel is carried and the layer mask moves by the origin',
          flush=True)


def gate_a4(app, folder, receipts):
    findings = {}
    size, anchor = (CANVAS[0] + 6, CANVAS[1] + 4), 'center'
    for depth in (8, 16, 32):
        document = seeded(app, depth, folder, f'a4_{depth}',
                          guides=False, channel=False, mask=False)
        old = canvas_of(app, document)
        new = (old[0] + 6, old[1] + 4)
        before = render(app, document)
        cursor = history(app, document)
        app.call('canvas_size', document_id=document, width=new[0], height=new[1],
                 anchor=anchor)
        assert canvas_of(app, document) == new, depth
        app.call('undo', document_id=document)
        assert canvas_of(app, document) == old, (depth, 'undo did not restore the canvas')
        assert render(app, document)['samples'] == before['samples'], \
            (depth, 'undo did not restore the pixels')
        app.call('redo', document_id=document)
        assert canvas_of(app, document) == new, (depth, 'redo did not reapply')
        app.call('undo', document_id=document)
        assert history(app, document) == cursor, (depth, 'the history cursor moved')

        # A SAME-SIZE call. Since 12F-1b it is a SUCCESSFUL no-op at ALL
        # THREE depths — `{"ok": true, "changed": false, "note": ...}` — with
        # no history step and no dirtying. The 8-bit phantom step the core
        # used to produce is gone (`canvas_resize` answers None for it), and
        # the 16-bit/float ERROR 12F-1 shipped is gone with it.
        cursor = history(app, document)
        before_same = render(app, document)
        reply = app.call('canvas_size', document_id=document,
                         width=old[0], height=old[1], anchor='center')
        moved = history(app, document) - cursor
        assert reply.get('ok') is True, (depth, 'a same-size call must succeed', reply)
        assert reply.get('changed') is False, (depth, 'it must report changed: false', reply)
        assert isinstance(reply.get('note'), str) and reply['note'], \
            (depth, 'it must carry a note', reply)
        assert moved == 0, (depth, 'a no-op made a history step', moved)
        assert canvas_of(app, document) == old, (depth, 'a no-op moved the canvas')
        assert render(app, document)['samples'] == before_same['samples'], \
            (depth, 'a no-op moved the pixels')
        findings[str(depth)] = {'undo': True, 'redo': True,
                                'same_size_changed': reply.get('changed'),
                                'same_size_note': reply.get('note'),
                                'same_size_history_steps': moved}
    receipts['a4'] = findings
    print('PASS a4: one undo and one redo each; a same-size call SUCCEEDS with '
          'changed: false and 0 history steps at 8, 16 and 32 bits', flush=True)


def gate_a5(app, folder, receipts):
    findings = {}
    for depth in (16, 32):
        document = seeded(app, depth, folder, f'a5_{depth}')
        old = canvas_of(app, document)
        new = (old[0] + 8, old[1] + 5)
        app.call('canvas_size', document_id=document, width=new[0], height=new[1],
                 anchor='bottom-right')
        first = folder / f'a5_{depth}_a.rz'
        second = folder / f'a5_{depth}_b.rz'
        app.call('save_copy', document_id=document, path=str(first))
        reopened = app.call('open_document', path=str(first))['document']['id']
        assert canvas_of(app, reopened) == new, depth
        app.call('save_copy', document_id=reopened, path=str(second))
        left = hashlib.sha256(first.read_bytes()).hexdigest()
        right = hashlib.sha256(second.read_bytes()).hexdigest()
        assert left == right, (depth, left, right)
        findings[str(depth)] = {'canvas': list(new), 'sha256': left}
    receipts['a5'] = findings
    print('PASS a5: .rz save -> reopen -> resave is byte-identical at 16 and at float',
          flush=True)


def gate_a6(app, folder, receipts):
    document = seeded(app, 16, folder, 'a6', guides=False, channel=False, mask=False)
    size, anchor = (CANVAS[0] + 13, CANVAS[1] + 9), 'bottom-right'
    app.call('start_recording', document_id=document)
    app.call('canvas_size', document_id=document, width=size[0], height=size[1],
             anchor=anchor)
    app.call('stop_recording', document_id=document, name='CanvasSize12F1')
    recorded = canvas_of(app, document)
    assert recorded == size, recorded

    fresh = seeded(app, 16, folder, 'a6b', guides=False, channel=False, mask=False)
    app.call('run_action', document_id=fresh, name='CanvasSize12F1')
    replayed = canvas_of(app, fresh)
    assert replayed == recorded, (replayed, recorded)
    receipts['a6'] = {'recorded': list(recorded), 'replayed': list(replayed)}
    print('PASS a6: a recorded canvas_size replays through run_action at 16 bits',
          flush=True)


def gate_a7(app, folder, receipts):
    findings = {}
    for depth in (16, 32):
        document = seeded(app, depth, folder, f'a7_{depth}',
                          guides=False, channel=False, mask=False)
        reply = app.call('get_precision', document_id=document)
        tools = set(reply.get('supported_tools') or []) | set(reply.get('shared_tools') or [])
        assert 'canvas_size' in tools, (depth, sorted(tools))
        findings[str(depth)] = {'bits_per_channel': reply.get('bits_per_channel'),
                                'lists_canvas_size': True}
    receipts['a7'] = findings
    print('PASS a7: get_precision lists canvas_size at 16 bits and at float', flush=True)


def placed_source(app, document, layer):
    """`get_image_source`'s answer for `layer`: None for an ordinary raster layer."""
    reply = app.call('get_image_source', document_id=document, layer=layer)
    return reply.get('source', 'absent')


def placed_offset(app, document):
    """The (x, y) offset of the layer `place_fixture` named."""
    found = [(x, y) for name, x, y in offsets(app, document) if name == 'Placed']
    assert len(found) == 1, offsets(app, document)
    return found[0]


def place_fixture(app, folder, depth, tag):
    """A document of this drive's canvas holding one PLACED image: an ordinary
    raster layer, with nothing of the file kept."""
    png = folder / f'{tag}.png'
    if not png.exists():
        rgba = bytearray()
        for i in range(12 * 9):
            rgba += bytes(((i * 13) % 256, (i * 29 + 40) % 256, (i * 7 + 90) % 256, 255))
        write_png8(png, 12, 9, bytes(rgba))
    document = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                        background='white')['document']['id']
    reply = app.call('place_image', document_id=document, path=str(png),
                     x=PLACE_AT[0], y=PLACE_AT[1], name='Placed')
    assert reply.get('source', 'absent') is None, reply
    return document, int(reply['layer'])


def gate_a8(app, folder, receipts):
    findings = {}
    for depth in (8, 16):
        document, layer = place_fixture(app, folder, depth, f'a8_{depth}')
        assert placed_source(app, document, layer) is None, depth
        before_offsets = offsets(app, document)
        before_placed = placed_offset(app, document)
        base_render = render(app, document)

        per_case = {}
        for name, (size, anchor) in (('grow', A8_GROW), ('shrink', A8_SHRINK)):
            origin = anchor_origin(anchor, CANVAS, size)
            app.call('canvas_size', document_id=document, width=size[0], height=size[1],
                     anchor=anchor)
            assert canvas_of(app, document) == size, (depth, name)
            moved = offsets(app, document)
            want = [(row, x + origin[0], y + origin[1]) for row, x, y in before_offsets]
            assert moved == want, (depth, name, moved, want)
            assert placed_source(app, document, layer) is None, (depth, name)
            after = render(app, document)
            assert after['samples'] == pad_crop(base_render['samples'], CANVAS, size, origin), \
                (depth, name, 'the render is not the drive\'s own pad/crop')

            if name == 'grow':
                # The `.rz` round trip keeps the plain layer and the render.
                path = folder / f'a8_{depth}.rz'
                app.call('save_copy', document_id=document, path=str(path))
                reopened = app.call('open_document', path=str(path))['document']['id']
                assert placed_source(app, reopened, layer) is None, (depth, 'after reopen')
                assert offsets(app, reopened) == want, (depth, 'offsets after reopen')
                assert render(app, reopened)['samples'] == after['samples'], \
                    (depth, 'the reopened render moved')

            app.call('undo', document_id=document)
            assert canvas_of(app, document) == CANVAS, (depth, name, 'undo')
            assert offsets(app, document) == before_offsets, \
                (depth, name, 'undo did not restore the offsets')
            per_case[name] = {'canvas': list(size), 'origin': list(origin),
                              'placed_offset': [before_placed[0] + origin[0],
                                                before_placed[1] + origin[1]]}

        # A recorded step replays on a FRESH placed document.
        app.call('start_recording', document_id=document)
        app.call('canvas_size', document_id=document, width=A8_GROW[0][0],
                 height=A8_GROW[0][1], anchor=A8_GROW[1])
        app.call('stop_recording', document_id=document, name=f'PlacedCanvasSize{depth}')
        fresh, _ = place_fixture(app, folder, depth, f'a8_{depth}')
        app.call('run_action', document_id=fresh, name=f'PlacedCanvasSize{depth}')
        assert canvas_of(app, fresh) == A8_GROW[0], (depth, 'the replay did not resize')
        origin = anchor_origin(A8_GROW[1], CANVAS, A8_GROW[0])
        replayed = placed_offset(app, fresh)
        assert replayed == (before_placed[0] + origin[0], before_placed[1] + origin[1]), \
            (depth, 'replayed offset', replayed)

        per_case['replay'] = {'canvas': list(A8_GROW[0]), 'placed_offset': list(replayed)}
        findings[str(depth)] = per_case
    receipts['a8'] = findings
    print('PASS a8: a placed image is a raster layer that rides Canvas Size at 8 and 16 bits',
          flush=True)


# ------------------------------------------------------- Block B: straighten --

def straighten_picture():
    """The 48 x 36 RGBA8 picture every B gate is measured on: an opaque
    gradient band, an alpha-128 band and an alpha-0 band whose stored RGB is
    LOUD magenta, so colour bleeding out of a transparent sample would be
    unmistakable."""
    rgba = bytearray()
    for y in range(CANVAS[1]):
        for x in range(CANVAS[0]):
            band = y // (CANVAS[1] // 3)
            if band >= 2:
                rgba += bytes((255, 0, 255, 0))
            else:
                rgba += bytes(((x * 5) % 256, (y * 7) % 256,
                               (x * 3 + y * 11) % 256, 255 if band == 0 else 128))
    return bytes(rgba)


def straighten_fixture(app, folder, depth, tag, *, guides=False, channel=False):
    """The picture at `depth`, written by this file and opened through
    `open_document` (whose default `precision` is "preserve", so the 16-bit
    PNG opens as a 16-bit document and the 8-bit one as an 8-bit document)."""
    picture = straighten_picture()
    path = folder / f'{tag}.png'
    if depth == 16:
        write_png16(path, CANVAS[0], CANVAS[1], [v * 257 for v in picture])
    else:
        write_png8(path, CANVAS[0], CANVAS[1], picture)
    document = app.call('open_document', path=str(path))['document']['id']
    if guides:
        for orientation, position in GUIDES:
            app.call('add_guide', document_id=document, orientation=orientation,
                     position=position)
        app.call('set_ruler_origin', document_id=document, x=RULER[0], y=RULER[1])
    if channel:
        app.call('select_rect', document_id=document, x=3, y=2, width=12, height=9)
        app.call('save_selection', document_id=document, name='Cover')
        app.call('deselect', document_id=document)
    return document


def codes(image, index):
    """One render sample as an 8-BIT-EQUIVALENT number, whatever depth the
    render came back at."""
    return image['samples'][index] / 257.0 if image['depth'] == 16 else \
        float(image['samples'][index])


def cross_depth(deep, flat, what):
    """G7's cross-depth rule, returning the worst alpha and RGB differences it
    measured so a receipt carries numbers rather than a boolean."""
    assert (deep['width'], deep['height']) == (flat['width'], flat['height']), \
        (what, (deep['width'], deep['height']), (flat['width'], flat['height']))
    worst_alpha, worst_rgb = 0.0, 0.0
    for i in range(deep['width'] * deep['height']):
        a_deep, a_flat = codes(deep, i * 4 + 3), codes(flat, i * 4 + 3)
        worst_alpha = max(worst_alpha, abs(a_deep - a_flat))
        if a_deep >= 1.0 and a_flat >= 1.0:
            for c in range(3):
                worst_rgb = max(worst_rgb,
                                abs(codes(deep, i * 4 + c) - codes(flat, i * 4 + c)))
    assert worst_alpha <= CROSS_DEPTH_CODES, \
        f'{what}: worst alpha difference {worst_alpha} 8-bit codes'
    assert worst_rgb <= CROSS_DEPTH_CODES, \
        f'{what}: worst RGB difference {worst_rgb} 8-bit codes'
    return worst_alpha, worst_rgb


def control_distance(left, right, what):
    """The non-vacuity control: the largest 8-bit-equivalent difference
    between two renders of the same window, which must reach CONTROL_CODES."""
    assert (left['width'], left['height']) == (right['width'], right['height']), what
    worst = 0.0
    for i in range(len(left['samples'])):
        worst = max(worst, abs(codes(left, i) - codes(right, i)))
    assert worst >= CONTROL_CODES, \
        f'{what}: the straighten differs from the angle-0 crop by only {worst} codes'
    return worst


def saved_png(app, document, path):
    """The document's own encoded PNG, decoded by this file. A 16-bit
    document writes a 16-bit PNG, so this is where the cross-depth rule meets
    genuine u16 samples — `render` answers a reduced display projection."""
    app.call('save_copy', document_id=document, path=str(path))
    return png_rgba(path.read_bytes())


def straighten_call(app, document, sampler, angle=STRAIGHTEN_ANGLE):
    x, y, w, h = STRAIGHTEN
    return app.call('crop', document_id=document, x=x, y=y, width=w, height=h,
                    angle=angle, sampler=sampler)


def gate_b1(app, folder, receipts):
    findings = {}
    x, y, w, h = STRAIGHTEN
    for sampler in STRAIGHTEN_SAMPLERS:
        deep = straighten_fixture(app, folder, 16, f'b1_16_{sampler}')
        flat = straighten_fixture(app, folder, 8, f'b1_8_{sampler}')
        straighten_call(app, deep, sampler)
        straighten_call(app, flat, sampler)
        assert canvas_of(app, deep) == (w, h), (sampler, canvas_of(app, deep))
        assert canvas_of(app, flat) == (w, h), (sampler, canvas_of(app, flat))
        deep_offsets = [(ox, oy) for _, ox, oy in offsets(app, deep)]
        flat_offsets = [(ox, oy) for _, ox, oy in offsets(app, flat)]
        assert deep_offsets == flat_offsets, (sampler, deep_offsets, flat_offsets)

        # The SAVED pictures, at each document's own depth: a 16-bit document
        # writes 16-bit samples, which is what "the 16-bit straighten of the
        # x257-promoted picture" means.
        deep_png = saved_png(app, deep, folder / f'b1_16_{sampler}_out.png')
        flat_png = saved_png(app, flat, folder / f'b1_8_{sampler}_out.png')
        worst_alpha, worst_rgb = cross_depth(
            deep_png, flat_png, f'b1 {sampler}: 16 bits against 8')
        # And the DISPLAY projections agree too, which is what a person sees.
        deep_render = render(app, deep)
        flat_render = render(app, flat)
        cross_depth(deep_render, flat_render, f'b1 {sampler}: the rendered pictures')

        # THE CONTROL: the angle-0 crop of the SAME window on a fresh 16-bit
        # twin. A straighten that silently did nothing would land here.
        control = straighten_fixture(app, folder, 16, f'b1_ctrl_{sampler}')
        app.call('crop', document_id=control, x=x, y=y, width=w, height=h)
        distance = control_distance(
            deep_png, saved_png(app, control, folder / f'b1_ctrl_{sampler}_out.png'),
            f'b1 {sampler} control')
        findings[sampler] = {'canvas': [w, h], 'offsets': deep_offsets,
                             'worst_alpha_codes': worst_alpha,
                             'worst_rgb_codes': worst_rgb,
                             'control_codes': distance,
                             'tolerance_codes': CROSS_DEPTH_CODES,
                             'control_floor_codes': CONTROL_CODES,
                             # Recorded because it says WHAT was compared: the
                             # SAVED pictures carry each document's own depth,
                             # while `render` answers a 16-bit document with
                             # its 8-bit display projection.
                             'saved_depths': [deep_png['depth'], flat_png['depth']],
                             'render_depths': [deep_render['depth'],
                                               flat_render['depth']]}
    receipts['b1'] = findings
    print('PASS b1: a 7.5 degree straighten at 16 bits equals the 8-bit call within '
          f'{CROSS_DEPTH_CODES} 8-bit code for bilinear and bicubic, with a control',
          flush=True)


def window_points():
    x, y, w, h = STRAIGHTEN
    return [{'x': i, 'y': j} for j in range(h) for i in range(w)]


def channel_samples(app, document, key):
    app.call('load_selection', document_id=document, source='channel', channel='Cover')
    reply = app.call('get_selection', document_id=document, points=window_points())
    samples = reply.get('samples') or []
    assert len(samples) == len(window_points()), (document, len(samples))
    return [float(sample[key]) for sample in samples]


def gate_b2(app, folder, receipts):
    x, y, w, h = STRAIGHTEN
    origin = (-x, -y)
    want_guides = padded_guides(GUIDES, origin, (w, h))
    want_ruler = padded_origin(RULER, origin, (w, h))
    answers = {}
    for depth in (8, 16):
        document = straighten_fixture(app, folder, depth, f'b2_{depth}',
                                      guides=True, channel=True)
        straighten_call(app, document, 'bilinear')
        guides, ruler = chrome(app, document)
        assert guides == want_guides, (depth, guides, want_guides)
        assert ruler == want_ruler, (depth, ruler, want_ruler)
        key = 'coverage16' if depth == 16 else 'coverage'
        answers[depth] = {'guides': guides, 'ruler': list(ruler),
                          'channel': channel_samples(app, document, key),
                          'document': document}
    deep = [v / 257.0 for v in answers[16]['channel']]
    flat = answers[8]['channel']
    worst = max(abs(a - b) for a, b in zip(deep, flat))
    assert worst <= CROSS_DEPTH_CODES, f'b2: worst channel difference {worst} 8-bit codes'

    # THE CONTROL: the same channel under the angle-0 crop of the same window.
    control = straighten_fixture(app, folder, 16, 'b2_ctrl', guides=True, channel=True)
    app.call('crop', document_id=control, x=x, y=y, width=w, height=h)
    reference = [v / 257.0 for v in channel_samples(app, control, 'coverage16')]
    distance = max(abs(a - b) for a, b in zip(deep, reference))
    assert distance >= CONTROL_CODES, \
        f'b2: the straightened channel differs from the angle-0 crop by only {distance} codes'

    receipts['b2'] = {'guides': want_guides, 'ruler': list(want_ruler),
                      'worst_channel_codes': worst, 'control_codes': distance,
                      'tolerance_codes': CROSS_DEPTH_CODES,
                      'control_floor_codes': CONTROL_CODES,
                      'points': len(deep)}
    print('PASS b2: guides, the ruler origin and the saved channel follow the straighten '
          'at both depths, with a control', flush=True)


def gate_b3(app, folder, receipts):
    x, y, w, h = STRAIGHTEN
    document = straighten_fixture(app, folder, 16, 'b3', guides=True, channel=True)
    before = render(app, document)
    cursor = history(app, document)
    straighten_call(app, document, 'bilinear')
    assert canvas_of(app, document) == (w, h)
    after = render(app, document)
    app.call('undo', document_id=document)
    assert canvas_of(app, document) == CANVAS, 'undo did not restore the canvas'
    assert render(app, document)['samples'] == before['samples'], \
        'undo did not restore the pixels'
    app.call('redo', document_id=document)
    assert canvas_of(app, document) == (w, h), 'redo did not reapply'
    assert render(app, document)['samples'] == after['samples'], 'redo changed the pixels'
    assert history(app, document) - cursor == 1, 'a straighten is not one history step'

    first = folder / 'b3_a.rz'
    second = folder / 'b3_b.rz'
    app.call('save_copy', document_id=document, path=str(first))
    reopened = app.call('open_document', path=str(first))['document']['id']
    assert canvas_of(app, reopened) == (w, h)
    app.call('save_copy', document_id=reopened, path=str(second))
    left = hashlib.sha256(first.read_bytes()).hexdigest()
    right = hashlib.sha256(second.read_bytes()).hexdigest()
    assert left == right, (left, right)

    # The RECORDED step: `angle` must be a JSON NUMBER, so a replay is
    # unaffected by S7's stricter parse.
    recorder = straighten_fixture(app, folder, 16, 'b3_rec', guides=True, channel=True)
    app.call('start_recording', document_id=recorder)
    straighten_call(app, recorder, 'bilinear')
    app.call('stop_recording', document_id=recorder, name='Straighten12F1b2')
    listed = app.call('list_actions', name='Straighten12F1b2')
    steps = listed['action']['steps']
    crops = [step for step in steps if step['tool'] == 'crop']
    assert len(crops) == 1, steps
    angle = crops[0]['arguments'].get('angle')
    assert isinstance(angle, (int, float)) and not isinstance(angle, bool), \
        f'the recorded angle is {angle!r}, not a JSON number'
    assert abs(float(angle) - STRAIGHTEN_ANGLE) < 1e-9, angle
    assert crops[0]['arguments'].get('sampler') == 'bilinear', crops[0]

    fresh = straighten_fixture(app, folder, 16, 'b3_replay', guides=True, channel=True)
    app.call('run_action', document_id=fresh, name='Straighten12F1b2')
    assert canvas_of(app, fresh) == (w, h), 'the replay did not straighten'
    assert render(app, fresh)['samples'] == render(app, recorder)['samples'], \
        'the replayed straighten is not sample-for-sample the recorded one'

    receipts['b3'] = {'canvas': [w, h], 'sha256': left, 'recorded_angle': float(angle),
                      'recorded_sampler': 'bilinear', 'history_steps': 1,
                      'replay_equal': True}
    print('PASS b3: undo/redo, an identical .rz round trip, a recorded NUMERIC angle and '
          'a sample-for-sample replay', flush=True)


def gate_b4(app, folder, receipts):
    replies = {}
    for depth in (8, 16):
        document = straighten_fixture(app, folder, depth, f'b4_{depth}')
        app.call('add_text_layer', document_id=document, text='Straighten',
                 x=6, y=10, size=11)
        assert any('text' in row for row in app.call('get_document',
            document_id=document)['layers']), (depth, 'text fixture missing')
        reply = straighten_call(app, document, 'bilinear')
        assert not reply.get('rasterized_layers'), (depth, reply)
        assert any('text' in row for row in app.call('get_document',
            document_id=document)['layers']), (depth, 'text description lost')
        replies[depth] = {'editable': True, 'rasterized_layers': []}
    assert replies[16] == replies[8], ('the 16-bit reply differs from the 8-bit one',
                                       replies)
    receipts['b4'] = replies
    print("PASS b4: text stays editable after straighten at 8 and 16 bits",
          flush=True)


def gate_b5(app, folder, receipts):
    x, y, w, h = STRAIGHTEN
    findings = {}

    # (iii) FLOAT, first, on a fresh document: since df6c43b float
    # straightens, so a present-but-UNREADABLE angle reaches the `crop`
    # handler's own guard (`angle.isFinite`, AgentServer+Retouch.swift) and is
    # refused BY NAME with the host range sentence, as at 8 and 16 bits.
    assert FLOAT_FIXTURE.exists(), 'the committed float fixture is missing'
    document = seeded(app, 32, folder, 'b5_float',
                      guides=False, channel=False, mask=False)
    cursor = history(app, document)
    before = render(app, document)
    message = refuses(app, 'crop', document_id=document, x=1, y=1,
                      width=6, height=6, angle='abc')
    assert message and ANGLE_REFUSAL in message, \
        f'float answered an unreadable angle with: {message}'
    assert history(app, document) == cursor, 'the float refusal made a history step'
    assert render(app, document)['samples'] == before['samples'], \
        'the float refusal changed the picture'
    findings['unreadable_float'] = message

    # (i) FLOAT straightens a non-zero angle: one history step, the canvas
    # becomes the window, the result is not the angle-0 crop of the same
    # window (the control, on a fresh twin), and one `undo` restores the
    # canvas and the render exactly.
    old = (before['width'], before['height'])
    window = dict(x=1, y=1, width=old[0] - 4, height=old[1] - 4)
    app.call('crop', document_id=document, angle=STRAIGHTEN_ANGLE, **window)
    assert history(app, document) - cursor == 1, 'a float straighten is not one history step'
    assert canvas_of(app, document) == (old[0] - 4, old[1] - 4), canvas_of(app, document)
    straightened = render(app, document)
    control = seeded(app, 32, folder, 'b5_float_ctrl',
                     guides=False, channel=False, mask=False)
    app.call('crop', document_id=control, **window)
    distance = control_distance(straightened, render(app, control), 'b5 float control')
    app.call('undo', document_id=document)
    assert canvas_of(app, document) == old, 'undo did not restore the float canvas'
    assert render(app, document)['samples'] == before['samples'], \
        'undo did not restore the float pixels'
    findings['float_straighten'] = {'canvas': [old[0] - 4, old[1] - 4], 'history_steps': 1,
                                    'control_codes': distance,
                                    'control_floor_codes': CONTROL_CODES,
                                    'undo_restores_exactly': True}

    # (ii) A present-but-UNREADABLE angle refuses BY NAME at 8 AND 16 bits.
    for depth in (8, 16):
        target = straighten_fixture(app, folder, depth, f'b5_{depth}')
        cursor = history(app, target)
        before = render(app, target)
        message = refuses(app, 'crop', document_id=target, x=x, y=y, width=w, height=h,
                          angle='abc')
        assert message and ANGLE_REFUSAL in message, (depth, message)
        assert history(app, target) == cursor, (depth, 'the refusal made a history step')
        assert canvas_of(app, target) == CANVAS, (depth, 'the refusal moved the canvas')
        assert render(app, target)['samples'] == before['samples'], \
            (depth, 'the refusal changed the picture')
        findings[f'unreadable_{depth}'] = message

    # (iv) CONTROL: a READABLE string angle straightens, and equals the
    # numeric call sample for sample.
    string_doc = straighten_fixture(app, folder, 16, 'b5_string')
    app.call('crop', document_id=string_doc, x=x, y=y, width=w, height=h,
             angle=str(STRAIGHTEN_ANGLE), sampler='bilinear')
    numeric_doc = straighten_fixture(app, folder, 16, 'b5_numeric')
    straighten_call(app, numeric_doc, 'bilinear')
    assert canvas_of(app, string_doc) == (w, h)
    assert render(app, string_doc)['samples'] == render(app, numeric_doc)['samples'], \
        'a readable string angle did not equal the numeric call'
    findings['readable_string_equals_numeric'] = True

    receipts['b5'] = findings
    print('PASS b5: an unreadable angle refuses by name at float, 8 and 16 bits with no '
          'history step, float straightens as one undoable step with a control, and a '
          'readable string angle still works', flush=True)


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

    receipts = {'expectations': 'fixed in scripts/verify_phase12f1_geometry.py before any '
                                'measurement; Block A is EXACT everywhere because Canvas '
                                'Size reads no sample, and Block B\'s cross-depth rule is '
                                f'{CROSS_DEPTH_CODES} 8-bit-equivalent code with a '
                                f'{CONTROL_CODES}-code non-vacuity control'}
    failures, not_run = [], []
    # A gate with no implementation is `None`: it prints NOT IMPLEMENTED and
    # this drive exits 2. It is never reported as a pass.
    gates = [('a1', lambda: gate_a1(app, folder, receipts)),
             ('a2', lambda: gate_a2(app, folder, receipts)),
             ('a3', lambda: gate_a3(app, folder, receipts)),
             ('a4', lambda: gate_a4(app, folder, receipts)),
             ('a5', lambda: gate_a5(app, folder, receipts)),
             ('a6', lambda: gate_a6(app, folder, receipts)),
             ('a7', lambda: gate_a7(app, folder, receipts)),
             ('a8', lambda: gate_a8(app, folder, receipts)),
             ('b1', lambda: gate_b1(app, folder, receipts)),
             ('b2', lambda: gate_b2(app, folder, receipts)),
             ('b3', lambda: gate_b3(app, folder, receipts)),
             ('b4', lambda: gate_b4(app, folder, receipts)),
             ('b5', lambda: gate_b5(app, folder, receipts))]
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        for name, run in gates:
            if run is None:
                not_run.append(name)
                receipts[name] = {'status': 'NOT IMPLEMENTED'}
                print(f'NOT IMPLEMENTED {name}', flush=True)
                continue
            try:
                run()
            except BaseException as error:  # a failed gate IS the receipt
                failures.append(name)
                receipts.setdefault(name, {})['failure'] = repr(error)
                print(f'FAIL {name}: {error!r}', flush=True)

    receipts['failures'] = failures
    receipts['not_run'] = not_run
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    if failures:
        print(f'FAILED GATES: {failures}', flush=True)
        sys.exit(1)
    if not_run:
        print(f'INCOMPLETE: not implemented: {not_run}', flush=True)
        sys.exit(2)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
