#!/usr/bin/env python3
"""Owned-app Phase 12G typed clipboard drive (12G-1 gates g1–g6, 12G-2 g7–g11,
12G-3 g12–g13, then g14–g19 and g21–g23). `--gates g16,g23` runs only the
named gates (a loop check; the gate is every one).

g23 (g16, g18(b), g18's layers replays and g19's layers into float
rewritten), each sub-case the branch measured against Photoshop, at
8 and 16 bits: (a) Cmd-V never keeps the source position — a copy wholly in
view, cut at 60/40/10 %, or a 60 % x 60 % corner lands centred on the visible
canvas; (b) under a 45-degree view too, centred on the visible bounding box;
(c) in the same document, a same-size and a larger one (Photoshop's (24, 16)
and (92, 67)); (d) Cmd-V centres on a destination selection ((112, 72)),
Shift-Cmd-V ignores it, and both deselect in their one step (Reselect
restores), while a layers paste keeps the selection; (e) Shift-Cmd-V across
canvas sizes lands `relative_in_place` at Photoshop's corners ((42, 42) in
200 x 150 … (2, 2) in 40 x 30; (37, 41) for 15 x 13 at (7, 9)), on the same
size `in_place`, `get_clipboard.source` names the source canvas, a payload
file keeps its coordinates, and a recorded relative paste records `at`; (f)
copied layers centre ((9, 11)) and move by the same relative rule ((61, 40));
(g) `view: 2` centres by view 2 and `view: 9` is refused naming list_views;
(h) a saved-payload `at` step replays at (56, 40) under relative geometry and
(88, 64) under the bottom-right anchor, and a live paste with a mode is
refused.

g22, each sub-case the branch measured against Photoshop,
at 8 and 16 bits: copied layers pasted in place (and, at 8 bits, centred into
a larger canvas) bring no layer comp, alpha channel, guide or colour sampler,
and the destination's own are unchanged; applying a destination comp hides
the pasted layers it has no record of and names them in hidden_unrecorded;
a pasted drop shadow keeps its style and use_global_light, and the
destination keeps its own global light.

g21, each sub-case the branch measured against Photoshop:
a Copy with a group targeted is still the group's raw projection at 8, 16
and float bits, hidden or not (Photoshop's lands opaque white over the
selection, not the group's content: not built, a maintainer decision); Copy
Merged with a group targeted is the whole composite; Cut on a group with a
selection, and Cut with no selection, are refused with no history step;
Paste Into and Paste Outside with copied layers are refused; and New from
Clipboard with copied layers makes a document of their content box at their
depth, profile and PPI, the layers by name over a white Background.

g19: copied LAYERS paste into a 32-bit float document — a
16-bit or 8-bit layers copy lands by name as linear light (the g17 oracle),
Display P3 layers convert or, under `profile: "preserve"`, keep their
numbers, a text layer lands as text with an empty `rasterized` and a placed
image lands as its pixels, a default (Pass Through) group lands Pass Through
with an empty report whether its children are Normal or blended, a blend
mode measured as reset at 32 bits lands Normal (`blend_reset`), a layer
style and Blend If land with an Outer Glow in Screen landing Linear Dodge
(`effect_blend_reset`), a Levels layer lands with an empty report, and a
layer the float mode cannot hold (an effect mode outside the float set, a
Vibrance layer) refuses the whole paste naming the layer and the feature.

g18 (g10, g13, g14, g16, g17 keep their saved-payload coverage through
`record_payload`): a recorded paste is LIVE by default — the step
stores no pixels and no position and a replay pastes whatever the clipboard
holds then, pixels or copied layers — and `copy`, `copy_merged` and `cut` are
recorded steps that copy from the replaying document, so a recorded Copy →
Paste replays on any document.
`copy {mode: "auto"}` (what a UI Copy records) re-decides pixels or layers
from the replaying document's selection. A paste recorded with
`record_payload: true` (or `payload_path`) is the saved-payload step of 12G-2:
the landed RZCB as a file, `at` with the landed offset for an auto paste.

g17 (g2, g8, g12, g13 rewritten): a paste converts into the
destination's depth and profile with no refusal — a float copy is
tone-mapped into 16 and 8 bits, a 16-bit or 8-bit copy and an image from
another app are promoted into 32-bit float (linear light), another profile is
converted — and `profile: "preserve"` (the Paste Profile Mismatch alert's
Don't Convert) keeps the numbers. Paste in Place of an image from another app
lands like Paste. g17 checks the float promotion against an oracle written
here (the IEC 61966-2.1 decode and the primaries' matrix), Preserve, recording
and replay, and the layers paste's Don't Convert.

g16 (rewritten with g23): a default paste (placement auto, Edit ▸
Paste's rule) lands centred on a destination selection, deselecting it, and
otherwise centred on the visible canvas (floor/ceil of `get_view`'s
`visible`) even when the whole source rect is in view; layers are centred the
same way and keep the selection, and an auto paste recorded with
`record_payload` replays at its landed offset (`placement: "at"`).

New from Clipboard trims a typed payload to its content box (every
pixel with alpha > 0; a fully transparent payload and an external image keep
their size) and rebuilds the document around a copied text, shape or
adjustment layer's editable record; g9 carries the trim's expectations
for the plain payloads, g14 the editable restores and g15 the trim itself.

PASTEBOARD ISOLATION: before the owned app starts, this sets
DR_PASTEBOARD_NAME=com.kgalligan.doctored.verify-12g.<pid>, so every image
Copy/Cut/Paste in the owned instance reads and writes a PRIVATE named
pasteboard (`Clipboard.pasteboard`). The user's general pasteboard is never
read or written. The owned app releases that pasteboard when it quits
(`Clipboard.releasePrivatePasteboard`, 12G-2). g7's public-image reference
and g13's public-TIFF-beside-a-future-RZCB item write the same private
pasteboard through tiny `xcrun swift` helpers.

Every expected value is computed here from the drive's own inputs: the
pre-copy raw codes read back through `sample_pixel {source: "layer"}`, the
placement formula ((W - w) // 2, (H - h) // 2), an f64 straight source-over
oracle for Copy Merged, and the float layers' raw DRDC bytes. A failure exits
nonzero and leaves a JSON receipt in --receipts.
"""
import argparse
import base64
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8
from support import HostApp

FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
FW, FH = 48, 32
W, H = 40, 30
QUICK_MASK_REFUSAL = 'Exit Quick Mask before editing layer pixels'


# ------------------------------------------------------------- plumbing --

def refusal(app, name, **arguments):
    reply = app.client.rpc('tools/call', {'name': name, 'arguments': arguments})
    assert reply.get('isError'), (name, reply)
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def render(app, document):
    reply = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document}})
    assert not reply.get('isError'), reply
    encoded = next(row['data'] for row in reply['content'] if row['type'] == 'image')
    decoded = png_rgba(base64.b64decode(encoded))
    return (decoded['width'], decoded['height'], decoded['depth'], bytes(decoded['samples']))


def codes(app, document, layer, x, y):
    reply = app.call('sample_pixel', document_id=document, source='layer',
                     layer=layer, x=x, y=y)
    return list(reply['center_rgba_codes'])


def window_codes(app, document, layer, x0, y0, w, h):
    return [[codes(app, document, layer, x0 + px, y0 + py) for px in range(w)]
            for py in range(h)]


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def offset(row):
    if 'offset' in row:
        return list(row['offset'])
    return [row['offset_x'], row['offset_y']]


def new16(app, profile='srgb', width=W, height=H):
    return app.call('new_document', width=width, height=height, depth=16,
                    profile=profile, background='transparent')['document']['id']


def painted16(app, profile='srgb'):
    document = new16(app, profile)
    app.call('brush_stroke', document_id=document, layer=0,
             points=[[4, 5], [30, 18], [36, 26]], size=9, rgba=[0.2, 0.4, 0.6, 1])
    return document


def open_float(app):
    return app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']


def float_records(app, document, folder, label, width, height):
    """Raw little-endian f32 RGBA of every float raster record of this size."""
    path = folder / (label + '.rz')
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    encoded = path.read_bytes()
    header = struct.pack('<IIQ', width, height, width * height * 16)
    records = []
    at = 0
    while (at := encoded.find(header, at)) >= 0:
        body = encoded[at + len(header):at + len(header) + width * height * 16]
        assert len(body) == width * height * 16
        records.append(struct.unpack('<' + 'f' * (width * height * 4), body))
        at += len(header) + len(body)
    return records


def resave_identical(app, document, folder, label):
    first = folder / (label + '-a.rz')
    second = folder / (label + '-b.rz')
    app.call('save_copy', document_id=document, path=str(first), format='rz')
    reopened = app.call('open_document', path=str(first))['document']['id']
    app.call('save_copy', document_id=reopened, path=str(second), format='rz')
    a = hashlib.sha256(first.read_bytes()).hexdigest()
    b = hashlib.sha256(second.read_bytes()).hexdigest()
    assert a == b, (label, 'resave differs', a, b)
    return a


def over(top, opacity, bottom):
    """Straight source-over in f64; RGB is None where the result is empty."""
    at = top[3] * opacity
    ab = bottom[3]
    ao = at + ab * (1.0 - at)
    if ao == 0.0:
        return [None, None, None, 0.0]
    return [(top[c] * at + bottom[c] * ab * (1.0 - at)) / ao for c in range(3)] + [ao]


# ------------------------------------------------------------------ gates --

def g1(app):
    document = painted16(app)
    x0, y0, w, h = 5, 4, 12, 9
    before = window_codes(app, document, 0, x0, y0, w, h)
    assert any(pixel[3] not in (0, 65535) for row in before for pixel in row), \
        'the stroke should leave soft-edge alpha in the window'
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    copied = app.call('copy', document_id=document)
    assert copied['typed'] and copied['depth'] == 16, copied
    assert [copied['width'], copied['height']] == [w, h] and copied['offset'] == [x0, y0]
    clip = app.call('get_clipboard')
    assert clip['typed'] and clip['info']['has_coverage'], clip
    app.call('deselect', document_id=document)
    placed = {}
    for placement, expected in (('centered', [(W - w) // 2, (H - h) // 2]),
                                ('in_place', [x0, y0])):
        reply = app.call('paste', document_id=document, placement=placement)
        assert reply['typed'] and reply['offset'] == expected, (placement, reply)
        row = layers(app, document)[reply['layer']]
        assert offset(row) == expected, (placement, row)
        got = window_codes(app, document, reply['layer'], expected[0], expected[1], w, h)
        assert got == before, (placement, 'pasted codes differ from the pre-copy codes')
        placed[placement] = {'layer': reply['layer'], 'offset': expected}
    return {'window': [x0, y0, w, h], 'placed': placed, 'clipboard': clip['info']}


def g2(app, folder):
    """Depth and profile convert, each as one undo step. The float
    target is a document of its own: g4 reads the shared fixture document's
    composite, which a promoted paste would change."""
    out = {}
    float_doc = fresh_float(app, folder, 'g2-float')
    target16 = new16(app, width=FW, height=FH)
    app.call('copy', document_id=float_doc)
    history = cursor(app, target16)
    reply = app.call('paste', document_id=target16)
    assert reply['typed'] and reply['converted'] and reply['narrowed'], reply
    assert reply['depth'] == 16 and reply['origin_depth'] == 32, reply
    assert cursor(app, target16) == history + 1
    out['float_to_16'] = reply
    source16 = painted16(app)
    app.call('copy', document_id=source16)
    history = cursor(app, float_doc)
    reply = app.call('paste', document_id=float_doc)
    assert reply['typed'] and reply['converted'] and not reply['narrowed'], reply
    assert reply['depth'] == 32 and reply['origin_depth'] == 16, reply
    assert cursor(app, float_doc) == history + 1
    out['16_to_float'] = reply
    p3 = painted16(app, profile='display_p3')
    app.call('copy', document_id=p3)
    history = cursor(app, source16)
    reply = app.call('paste', document_id=source16)
    assert reply['converted'] and reply['profile'] == 'converted', reply
    assert cursor(app, source16) == history + 1
    out['p3_to_srgb'] = reply
    return out


def g3(app, folder):
    document = painted16(app)
    x0, y0, w, h = 6, 4, 14, 10
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    copy_path = folder / 'g3-copy.tiff'
    copied = app.call('copy', document_id=document, public_copy_path=str(copy_path))
    copied_info = app.call('get_clipboard')['info']
    history = cursor(app, document)
    cut_path = folder / 'g3-cut.tiff'
    cut = app.call('cut', document_id=document, public_copy_path=str(cut_path))
    assert cursor(app, document) == history + 1, 'cut is not one step'
    assert app.call('get_clipboard')['info'] == copied_info
    assert cut['typed_bytes'] == copied['typed_bytes']
    assert copy_path.read_bytes() == cut_path.read_bytes(), 'cut payload differs from copy'
    inside = window_codes(app, document, 0, x0, y0, w, h)
    assert all(pixel[3] == 0 for row in inside for pixel in row), 'alpha left inside'
    app.call('undo', document_id=document)

    # Pixels lock: refused by name, clipboard unchanged.
    app.call('select_rect', document_id=document, x=1, y=1, width=3, height=3)
    app.call('copy', document_id=document)
    marker = app.call('get_clipboard')['info']
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    app.call('set_layer_lock', document_id=document, layer=0, locks=['pixels'])
    locked = refusal(app, 'cut', document_id=document)
    assert 'lock' in locked.lower(), locked
    assert app.call('get_clipboard')['info'] == marker, 'a refused cut changed the clipboard'

    # Transparency lock: the frozen-alpha sentence, no step, the payload kept.
    app.call('set_layer_lock', document_id=document, layer=0, locks=['transparency'])
    history = cursor(app, document)
    before = render(app, document)
    frozen = refusal(app, 'cut', document_id=document)
    assert 'transparency locked' in frozen and 'no undo step' in frozen, frozen
    assert cursor(app, document) == history and render(app, document) == before
    after_frozen = app.call('get_clipboard')['info']
    app.call('copy', document_id=document)
    assert after_frozen == app.call('get_clipboard')['info'], 'frozen cut payload differs'
    app.call('set_layer_lock', document_id=document, layer=0, locks=[])

    # Quick Mask: the gate's refusal, layer unchanged.
    app.call('set_quick_mask', document_id=document, active=True)
    before = render(app, document)
    quick = refusal(app, 'cut', document_id=document)
    assert QUICK_MASK_REFUSAL in quick, quick
    app.call('set_quick_mask', document_id=document, active=False)
    assert render(app, document) == before

    # 12G-2 admits the float Cut; g11 gates it against its own oracle.
    return {'locked': locked, 'frozen': frozen, 'quick_mask': quick,
            'typed_bytes': cut['typed_bytes']}


def g4_16(app):
    document = painted16(app)
    app.call('new_layer', document_id=document, name='Top')
    app.call('brush_stroke', document_id=document, layer=1,
             points=[[2, 20], [22, 6], [38, 12]], size=11, rgba=[0.9, 0.15, 0.3, 0.8])
    app.call('set_layer_properties', document_id=document, index=1, opacity=0.6)
    x0, y0, w, h = 3, 3, 20, 14
    bottom = window_codes(app, document, 0, x0, y0, w, h)
    top = window_codes(app, document, 1, x0, y0, w, h)
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    merged = app.call('copy_merged', document_id=document)
    assert merged['typed'] and merged['offset'] == [x0, y0], merged
    fresh = new16(app)
    reply = app.call('paste', document_id=fresh, placement='in_place')
    got = window_codes(app, fresh, reply['layer'], x0, y0, w, h)
    worst = 0.0
    for py in range(h):
        for px in range(w):
            want = over([v / 65535 for v in top[py][px]], 0.6,
                        [v / 65535 for v in bottom[py][px]])
            pixel = got[py][px]
            worst = max(worst, abs(pixel[3] - want[3] * 65535))
            assert abs(pixel[3] - want[3] * 65535) <= 1, (px, py, pixel, want)
            if want[3] > 0:
                for c in range(3):
                    error = abs(pixel[c] - want[c] * 65535)
                    worst = max(worst, error)
                    assert error <= 1, (px, py, c, pixel, want)
    return {'window': [x0, y0, w, h], 'worst_code_error': worst}


def g4_float(app, folder):
    document = open_float(app)
    base = float_records(app, document, folder, 'g4f-base', FW, FH)
    assert len(base) == 1, len(base)
    bottom = base[0]
    x0, y0, w, h = 2, 3, 20, 12
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    app.call('copy', document_id=document)
    app.call('deselect', document_id=document)
    placed = app.call('paste', document_id=document, placement='centered')
    ox, oy = (FW - w) // 2, (FH - h) // 2
    assert placed['offset'] == [ox, oy], placed
    app.call('set_layer_properties', document_id=document, index=placed['layer'], opacity=0.6)
    mx, my, mw, mh = 6, 5, 30, 20
    app.call('select_rect', document_id=document, x=mx, y=my, width=mw, height=mh)
    merged = app.call('copy_merged', document_id=document)
    assert merged['typed'] and merged['depth'] == 32, merged
    fresh = open_float(app)
    app.call('paste', document_id=fresh, placement='in_place')
    records = float_records(app, fresh, folder, 'g4f-merged', mw, mh)
    assert len(records) == 1, len(records)
    got = records[0]

    def sample(buffer, width, x, y):
        i = (y * width + x) * 4
        return list(buffer[i:i + 4])

    worst = 0.0
    for py in range(mh):
        for px in range(mw):
            cx, cy = mx + px, my + py
            below = sample(bottom, FW, cx, cy)
            tx, ty = cx - ox, cy - oy
            above = (sample(bottom, FW, tx + x0, ty + y0)
                     if 0 <= tx < w and 0 <= ty < h else [0.0, 0.0, 0.0, 0.0])
            want = over(above, 0.6, below)
            pixel = sample(got, mw, px, py)
            for c in range(4):
                if want[c] is None:
                    continue
                error = abs(pixel[c] - want[c]) / max(abs(want[c]), 1.0)
                worst = max(worst, error)
                assert error <= 1e-5, (px, py, c, pixel, want)
    return {'window': [mx, my, mw, mh], 'worst_relative_error': worst}


def g5(app, folder):
    out = {}
    document = painted16(app)
    app.call('select_rect', document_id=document, x=4, y=4, width=16, height=12)
    app.call('copy', document_id=document)
    before = render(app, document)
    app.call('paste', document_id=document, placement='centered')
    pasted = render(app, document)
    app.call('undo', document_id=document)
    assert render(app, document) == before, 'paste undo'
    app.call('redo', document_id=document)
    assert render(app, document) == pasted, 'paste redo'
    app.call('select_rect', document_id=document, x=4, y=4, width=16, height=12)
    prior = render(app, document)
    app.call('cut', document_id=document, layer=0)
    cut = render(app, document)
    assert cut != prior
    app.call('undo', document_id=document)
    assert render(app, document) == prior, 'cut undo'
    app.call('redo', document_id=document)
    assert render(app, document) == cut, 'cut redo'
    out['rz16_sha256'] = resave_identical(app, document, folder, 'g5-16')
    float_doc = open_float(app)
    app.call('select_rect', document_id=float_doc, x=3, y=3, width=10, height=10)
    app.call('copy', document_id=float_doc)
    app.call('paste', document_id=float_doc, placement='in_place')
    out['rz32_sha256'] = resave_identical(app, float_doc, folder, 'g5-32')
    return out


def g6(app, folder):
    document = painted16(app)
    app.call('select_rect', document_id=document, x=2, y=3, width=17, height=11)
    path = folder / 'g6-public.tiff'
    app.call('copy', document_id=document, public_copy_path=str(path))
    output = subprocess.run(['sips', '-g', 'pixelWidth', '-g', 'pixelHeight', str(path)],
                            capture_output=True, text=True, check=True).stdout
    assert 'pixelWidth: 17' in output and 'pixelHeight: 11' in output, output
    # 8 bits: copy → paste equals the byte layer through render.
    width, height = 12, 9
    source = bytearray()
    for y in range(height):
        for x in range(width):
            source += bytes(((x * 37 + y * 11) % 256, (x * 5 + y * 41) % 256,
                             (x * 23 + y * 7) % 256, 255))
    png = folder / 'g6-source.png'
    write_png8(png, width, height, bytes(source))
    byte_doc = app.call('open_document', path=str(png))['document']['id']
    copied = app.call('copy', document_id=byte_doc)
    # 12G-2: an 8-bit copy writes the byte-origin typed payload too.
    assert copied['typed'] and copied['depth'] == 8 and copied['typed_bytes'] > 0, copied
    fresh = app.call('new_document', width=width, height=height, depth=8,
                     background='transparent')['document']['id']
    pasted = app.call('paste', document_id=fresh)
    assert pasted['typed'] and pasted['offset'] == [0, 0], pasted
    assert pasted['origin_depth'] == 8 and not pasted['converted'], pasted
    rendered = render(app, fresh)
    assert rendered[:3] == (width, height, 8) and rendered[3] == bytes(source), \
        '8-bit copy/paste differs from the byte layer'
    fresh = app.call('new_document', width=width, height=height, depth=8,
                     background='transparent')['document']['id']
    in_place = app.call('paste', document_id=fresh, placement='in_place')
    assert in_place['offset'] == [0, 0], in_place
    assert render(app, fresh)[3] == bytes(source), '8-bit paste in place differs'
    return {'sips': output.strip().splitlines()[1:], 'byte_round_trip': True,
            'byte_in_place': in_place}


# ------------------------------------------------------- 12G-2 gates --

PUBLIC_WRITER = r"""
import AppKit
let arguments = CommandLine.arguments
let pasteboard = NSPasteboard(name: NSPasteboard.Name(arguments[1]))
let data = try! Data(contentsOf: URL(fileURLWithPath: arguments[2]))
pasteboard.clearContents()
let item = NSPasteboardItem()
item.setData(data, forType: .tiff)
exit(pasteboard.writeObjects([item]) ? 0 : 1)
"""


def write_public_tiff(folder, tiff):
    """Puts ONLY a public TIFF on the drive's private pasteboard — the
    pre-12G-2 shape of every external image."""
    script = folder / 'public-writer.swift'
    script.write_text(PUBLIC_WRITER)
    subprocess.run(['xcrun', 'swift', str(script), os.environ['DR_PASTEBOARD_NAME'],
                    str(tiff)], check=True, capture_output=True)


def fresh_float(app, folder, label):
    """A float document of its OWN: `open_document` answers an already-open
    path with the document that has it, which earlier gates have edited."""
    path = folder / (label + '.tiff')
    path.write_bytes(FLOAT_FIXTURE.read_bytes())
    return app.call('open_document', path=str(path))['document']['id']


def byte_pattern(width, height, seed=0):
    out = bytearray()
    for y in range(height):
        for x in range(width):
            out += bytes(((x * 37 + y * 11 + seed) % 256, (x * 5 + y * 41) % 256,
                          (x * 23 + y * 7 + seed * 3) % 256, 255))
    return bytes(out)


def open_png(app, folder, label, width, height, seed=0):
    source = byte_pattern(width, height, seed)
    png = folder / (label + '.png')
    write_png8(png, width, height, source)
    return app.call('open_document', path=str(png))['document']['id'], source


def source_px(source, width, x, y):
    i = (y * width + x) * 4
    return list(source[i:i + 4])


def layer_grid(app, document, layer, width, height):
    return [codes(app, document, layer, x, y) for y in range(height) for x in range(width)]


def g7(app, folder):
    out = {}
    document, source = open_png(app, folder, 'g7-source', W, H)
    x0, y0, w, h = 5, 4, 12, 9
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    copied = app.call('copy', document_id=document)
    assert copied['typed'] and copied['typed_bytes'] > 0 and copied['depth'] == 8, copied
    info = app.call('get_clipboard')['info']
    assert info['origin_depth'] == 8 and info['version'] == 3, info
    app.call('deselect', document_id=document)
    for placement, expected in (('in_place', [x0, y0]),
                                ('centered', [(W - w) // 2, (H - h) // 2])):
        reply = app.call('paste', document_id=document, placement=placement)
        assert reply['offset'] == expected and reply['typed'], (placement, reply)
        assert not reply['converted'] and reply['origin_depth'] == 8, (placement, reply)
        assert offset(layers(app, document)[reply['layer']]) == expected, placement
        got = window_codes(app, document, reply['layer'], expected[0], expected[1], w, h)
        want = [[source_px(source, W, x0 + px, y0 + py) for px in range(w)] for py in range(h)]
        assert got == want, (placement, 'pasted bytes differ from the copied window')
        out[placement] = reply
    # An 8-bit Display P3 copy into an 8-bit sRGB document converts (in the
    # core); the public image of the same TIFF is the reference.
    p3, _ = open_png(app, folder, 'g7-p3', 12, 9, seed=5)
    app.call('assign_profile', document_id=p3, profile='display_p3')
    tiff = folder / 'g7-p3.tiff'
    app.call('copy', document_id=p3, public_copy_path=str(tiff))
    typed_target = app.call('new_document', width=12, height=9, depth=8, profile='srgb',
                            background='transparent')['document']['id']
    converted = app.call('paste', document_id=typed_target)
    assert converted['typed'] and converted['converted'], converted
    write_public_tiff(folder, tiff)
    assert not app.call('get_clipboard')['typed'], 'the reference pasteboard is not public-only'
    public_target = app.call('new_document', width=12, height=9, depth=8, profile='srgb',
                             background='transparent')['document']['id']
    public = app.call('paste', document_id=public_target)
    assert public['external'] and not public['typed'], public
    # The typed paste converts in the core (`rgba8(in:profile:)`),
    # the public one through CoreGraphics; the two conversions of the same
    # pixels agree within 2 codes (alpha exactly).
    typed_grid = layer_grid(app, typed_target, converted['layer'], 12, 9)
    public_grid = layer_grid(app, public_target, public['layer'], 12, 9)
    worst = 0
    for index, (got, want) in enumerate(zip(typed_grid, public_grid)):
        assert got[3] == want[3], ('alpha', index, got, want)
        worst = max(worst, max(abs(g - w) for g, w in zip(got[:3], want[:3])))
    assert worst <= 2, ('the typed P3 paste differs from the public-image paste by', worst)
    out['p3_into_srgb'] = converted
    out['p3_into_srgb_worst_code_error'] = worst
    return out


def g8(app, folder):
    out = {}
    # 8-bit copy → 16-bit paste: exact ×257, no arm.
    byte_doc, source = open_png(app, folder, 'g8-byte', W, H, seed=9)
    app.call('copy', document_id=byte_doc)
    target = new16(app)
    reply = app.call('paste', document_id=target)
    assert reply['typed'] and reply['origin_depth'] == 8 and reply['depth'] == 16, reply
    for y in range(0, H, 3):
        for x in range(0, W, 3):
            want = [v * 257 for v in source_px(source, W, x, y)]
            assert codes(app, target, reply['layer'], x, y) == want, (x, y)
    out['byte_to_16'] = reply
    # 16-bit sRGB → P3: the paste converts with no argument, through the
    # convert_profile kernel.
    source16 = painted16(app)
    app.call('copy', document_id=source16)
    p3 = new16(app, profile='display_p3')
    landed = app.call('paste', document_id=p3)
    assert landed['converted'] and landed['profile'] == 'converted', landed
    app.call('convert_profile', document_id=source16, profile='display_p3')
    assert layer_grid(app, p3, landed['layer'], W, H) == layer_grid(app, source16, 0, W, H), \
        'the converted paste differs from convert_profile on the same pixels'
    out['srgb_to_p3'] = {'landed': landed}
    # Float → 16: the payload's carried map, bit-exact against
    # convert_precision rgba16 of the same one-layer float document.
    float_doc = fresh_float(app, folder, 'g7-11-float-1')
    app.call('set_tone_map', document_id=float_doc, operator='reinhard')
    app.call('copy', document_id=float_doc)
    reference = fresh_float(app, folder, 'g7-11-float-2')
    app.call('set_tone_map', document_id=reference, operator='reinhard')
    app.call('convert_precision', document_id=reference, mode='rgba16', flatten=True)
    mapped = app.call('paste', document_id=reference, placement='in_place')
    assert mapped['converted'] and mapped['narrowed'], mapped
    reference16 = layer_grid(app, reference, 0, FW, FH)
    assert layer_grid(app, reference, mapped['layer'], FW, FH) == reference16, \
        'float → 16 convert differs from convert_precision rgba16'
    # …and the same float copy into an 8-bit document in its own profile is
    # that result's rgba8. The 8-bit path converts the typed payload in
    # the core (`rgba8(in:profile:)`: the tone map to u16, then the u16 → 8
    # read), so the target is an 8-bit document carrying the float fixture's
    # own profile (a float document narrowed by convert_precision) and the
    # reference is bit-exact; the former CoreGraphics decode needed 1 code.
    app.call('convert_precision', document_id=reference, mode='rgba8', flatten=True,
             dither='none')
    byte_target = fresh_float(app, folder, 'g8-byte-target')
    app.call('convert_precision', document_id=byte_target, mode='rgba16', flatten=True)
    app.call('convert_precision', document_id=byte_target, mode='rgba8', flatten=True,
             dither='none')
    narrowed = app.call('paste', document_id=byte_target, placement='in_place')
    assert narrowed['typed'] and narrowed['narrowed'] and narrowed['depth'] == 8, narrowed
    assert narrowed['profile'] == 'same', narrowed
    byte_reference = fresh_float(app, folder, 'g7-11-float-3')
    app.call('set_tone_map', document_id=byte_reference, operator='reinhard')
    app.call('convert_precision', document_id=byte_reference, mode='rgba16', flatten=True)
    app.call('convert_precision', document_id=byte_reference, mode='rgba8', flatten=True,
             dither='none')
    got8 = layer_grid(app, byte_target, narrowed['layer'], FW, FH)
    want8 = layer_grid(app, byte_reference, 0, FW, FH)
    worst = 0
    for index, (got, want) in enumerate(zip(got8, want8)):
        error = max(abs(g - w) for g, w in zip(got, want))
        worst = max(worst, error)
        assert error == 0, (index, got, want)
    out['float_to_8_worst_code_error'] = worst
    out['float_to_16'] = mapped
    out['float_to_8'] = narrowed
    # 16 → float: the paste promotes, one step (g17 checks the values).
    app.call('copy', document_id=painted16(app))
    target = fresh_float(app, folder, 'g7-11-float-4')
    history = cursor(app, target)
    promoted = app.call('paste', document_id=target)
    assert promoted['converted'] and promoted['depth'] == 32, promoted
    assert cursor(app, target) == history + 1
    out['16_to_float'] = promoted
    return out


def alpha_box(grid, width, height):
    """(x, y, w, h) of every pixel whose alpha is nonzero in a row-major
    grid of RGBA values; None when there is none."""
    xs = [i % width for i, pixel in enumerate(grid) if pixel[3] != 0]
    ys = [i // width for i, pixel in enumerate(grid) if pixel[3] != 0]
    if not xs:
        return None
    return (min(xs), min(ys), max(xs) - min(xs) + 1, max(ys) - min(ys) + 1)


def windowed(grid, width, box):
    x0, y0, w, h = box
    return [grid[(y0 + y) * width + x0 + x] for y in range(h) for x in range(w)]


def history_untouched(app, document):
    """New from Clipboard adds no undo step, and the document is unsaved."""
    history = app.call('list_history', document_id=document)
    assert history['undo_count'] == 0, history
    summary = next(row for row in app.call('list_documents')['documents']
                   if row['id'] == document)
    assert summary['unsaved_changes'], summary
    message = refusal(app, 'undo', document_id=document)
    assert 'Nothing to undo' in message, message
    return summary


def g9(app, folder):
    out = {}
    p3 = painted16(app, profile='display_p3')
    source = layer_grid(app, p3, 0, W, H)
    box = alpha_box(source, W, H)
    app.call('copy', document_id=p3)
    made = app.call('new_from_clipboard')
    assert made['typed'] and made['depth'] == 16 and made['origin_depth'] == 16, made
    assert 'P3' in made['profile_description'], made
    assert made['ppi'] == [300, 300], made
    assert made['editable'] is None, made
    assert [made['width'], made['height']] == [box[2], box[3]], (made, box)
    assert made['source_offset'] == [box[0], box[1]], (made, box)
    assert made['trimmed'] == (box != (0, 0, W, H)), (made, box)
    document = made['document']['id']
    assert layer_grid(app, document, 0, box[2], box[3]) == windowed(source, W, box), \
        'codes differ'
    out['u16'] = made
    float_doc = fresh_float(app, folder, 'g7-11-float-5')
    app.call('set_tone_map', document_id=float_doc, operator='reinhard')
    app.call('copy', document_id=float_doc)
    made = app.call('new_from_clipboard')
    assert made['depth'] == 32 and made['tone_map']['operator'] == 'reinhard', made
    assert made['editable'] is None, made
    assert made['trimmed'] == ([made['width'], made['height']] != [FW, FH]), made
    tone = app.call('get_tone_map', document_id=made['document']['id'])
    assert tone['tone_map']['operator'] == 'reinhard', tone
    out['float'] = made
    byte_doc, source = open_png(app, folder, 'g9-byte', 12, 9, seed=2)
    tiff = folder / 'g9-byte.tiff'
    app.call('copy', document_id=byte_doc, public_copy_path=str(tiff))
    made = app.call('new_from_clipboard')
    assert made['depth'] == 8 and made['origin_depth'] == 8 and made['typed'], made
    assert made['editable'] is None, made
    assert made['trimmed'] == ([made['width'], made['height']] != [12, 9]), made
    rendered = render(app, made['document']['id'])
    assert rendered[:3] == (12, 9, 8) and rendered[3] == source, '8-bit bytes differ'
    out['byte'] = made
    write_public_tiff(folder, tiff)
    made = app.call('new_from_clipboard')
    assert not made['typed'] and made['editable'] is None, made
    assert not made['trimmed'] and made['source_offset'] is None, made
    out['external'] = made
    return out


def g10(app, folder):
    source = painted16(app)
    app.call('copy', document_id=source)
    target = new16(app)
    app.call('start_recording', discard=True)
    pasted = app.call('paste', document_id=target, record_payload=True)
    stopped = app.call('stop_recording')
    steps = [step for step in stopped['steps'] if step['tool'] == 'paste']
    assert len(steps) == 1, stopped['steps']
    path = Path(steps[0]['arguments']['payload_path'])
    store = (folder / 'owner' / 'clipboard-payloads').resolve()
    assert path.resolve().parent == store and path.suffix == '.drcb' and path.is_file(), path
    app.call('save_action', name='g10 paste', steps=stopped['steps'], overwrite=True)
    bundles = folder / 'bundles'
    bundles.mkdir(exist_ok=True)
    packaged = app.call('package_action', name='g10 paste', destination=str(bundles),
                        overwrite=True)
    bundle = Path(packaged['path'])
    action = json.loads((bundle / 'action.json').read_text())
    step = next(step for step in action['steps'] if step['tool'] == 'paste')
    asset = step['arguments']['payload_path']
    assert isinstance(asset, dict) and asset.get('$asset', '').endswith('.drcb'), step
    assert (bundle / 'assets' / asset['$asset']).is_file(), asset
    app.call('import_action', path=str(bundle), name='g10 imported', overwrite=True)
    fresh = new16(app)
    app.call('run_action', name='g10 imported', document_id=fresh)
    count = len(layers(app, fresh))
    assert count == 2, count
    assert layer_grid(app, fresh, 1, W, H) == layer_grid(app, target, pasted['layer'], W, H), \
        'the replayed paste differs from the recorded one'
    return {'payload_path': str(path), 'asset': asset, 'bundle': str(bundle)}


def g11(app, folder):
    out = {}
    document = fresh_float(app, folder, 'g7-11-float-6')
    base = float_records(app, document, folder, 'g11-base', FW, FH)[0]
    x0, y0, w, h = 4, 5, 14, 9
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    copy_path = folder / 'g11-copy.tiff'
    copied = app.call('copy', document_id=document, public_copy_path=str(copy_path))
    copied_info = app.call('get_clipboard')['info']
    history = cursor(app, document)
    cut_path = folder / 'g11-cut.tiff'
    cut = app.call('cut', document_id=document, public_copy_path=str(cut_path))
    assert cursor(app, document) == history + 1, 'float cut is not one step'
    assert app.call('get_clipboard')['info'] == copied_info
    assert cut['typed_bytes'] == copied['typed_bytes']
    assert copy_path.read_bytes() == cut_path.read_bytes(), 'float cut payload differs from copy'
    after = float_records(app, document, folder, 'g11-cut', FW, FH)[0]
    for y in range(FH):
        for x in range(FW):
            i = (y * FW + x) * 4
            assert after[i:i + 3] == base[i:i + 3], ('RGB changed', x, y)
            inside = x0 <= x < x0 + w and y0 <= y < y0 + h
            want = 0.0 if inside else base[i + 3]
            assert after[i + 3] == want, ('alpha', x, y, after[i + 3], want)
    out['cut'] = cut
    app.call('undo', document_id=document)
    app.call('set_layer_lock', document_id=document, layer=0, locks=['transparency'])
    history = cursor(app, document)
    frozen = refusal(app, 'cut', document_id=document)
    assert 'transparency locked' in frozen, frozen
    assert cursor(app, document) == history, 'a frozen float cut added a step'
    out['frozen'] = frozen
    # PPI mismatch disclosure: a 300-ppi copy into a 72-ppi document.
    source = painted16(app)
    app.call('copy', document_id=source)
    target = app.call('new_document', width=W, height=H, depth=16, ppi=72,
                      background='transparent')['document']['id']
    reply = app.call('paste', document_id=target)
    assert reply['ppi']['match'] is False, reply
    assert reply['ppi']['payload'] == [300, 300] and reply['ppi']['document'] == [72, 72], reply
    listing = json.dumps(app.call('list_history', document_id=target), ensure_ascii=False)
    assert 'Paste Layer (300 → 72 ppi)' in listing, listing
    out['ppi'] = reply
    return out


# ------------------------------------------------------- 12G-3 gates --

def row_named(app, document, name):
    matches = [row for row in layers(app, document) if row.get('name') == name]
    assert len(matches) == 1, (name, layers(app, document))
    return matches[0]


def adjustment_rows(app, document, folder, label, typed=True):
    """(op, params) of every adjustment layer, bottom first. A typed
    document's rows carry no params, so its stored descriptions are read out
    of a native save (the UTF-8 JSON each typed record holds)."""
    if not typed:
        return [(row['adjustment']['op'], row['adjustment']['params'])
                for row in layers(app, document) if row.get('is_adjustment')]
    path = folder / (label + '.rz')
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    data = path.read_bytes()
    decoder = json.JSONDecoder()
    found, at = [], 0
    while (at := data.find(b'{"evaluation":"precision_v1"', at)) >= 0:
        text = data[at:at + 65536].decode('utf-8', errors='ignore')
        value, length = decoder.raw_decode(text)
        found.append((value['op'], value['params']))
        at += length
    return found


def new8(app, width=W, height=H):
    return app.call('new_document', width=width, height=height, depth=8,
                    background='transparent')['document']['id']


def g12(app, folder):
    out = {}
    source = new16(app)
    added = app.call('add_text_layer', document_id=source, text='Title', x=5, y=6, size=20)
    text_layer = added['layer']
    text_row = layers(app, source)[text_layer]
    assert text_row['name'] == 'Title' and 'text' in text_row, text_row
    copied = app.call('copy', document_id=source)
    assert copied['editable'] == 'text', copied
    info = app.call('get_clipboard')['info']
    editable = info['editable']
    assert editable['type'] == 'text' and editable['name'] == 'Title', editable
    assert editable['anchor'] == [5, 6] and editable['record_bytes'] > 0, editable
    out['clipboard'] = editable

    fresh16 = new16(app)
    pasted = app.call('paste', document_id=fresh16, placement='in_place')
    assert pasted['editable'] == {'type': 'text', 'restored': True}, pasted
    assert pasted['name'] == 'Title', pasted
    row = row_named(app, fresh16, 'Title')
    assert 'text' in row, row
    assert row['text']['string'] == text_row['text']['string'], row
    assert row['text']['origin'] == text_row['text']['origin'], (row, text_row)
    out['into_16'] = pasted

    fresh8 = new8(app)
    pasted = app.call('paste', document_id=fresh8)
    assert pasted['editable']['restored'] and pasted['name'] == 'Title', pasted
    row = row_named(app, fresh8, 'Title')
    assert 'text' in row and not row.get('is_adjustment', False), row
    renamed = app.call('paste', document_id=fresh8, name='Renamed')
    assert renamed['name'] == 'Renamed' and renamed['editable']['restored'], renamed
    assert 'text' in row_named(app, fresh8, 'Renamed')
    out['into_8'] = pasted

    shape = app.call('add_shape_layer', document_id=source, kind='rect', x=4, y=4, w=12,
                     h=8, fill='#cc3322')
    copied = app.call('copy', document_id=source, layer=shape['layer'])
    assert copied['editable'] == 'shape', copied
    shape8 = new8(app)
    pasted = app.call('paste', document_id=shape8)
    assert pasted['editable'] == {'type': 'shape', 'restored': True}, pasted
    assert 'shape' in layers(app, shape8)[pasted['layer']], layers(app, shape8)
    out['shape'] = pasted

    # A 32-bit float document evaluates Levels, so levels restores at 8, 16
    # and 32 bits; an invert copy (refused at 32 bits; the vibrance copy this
    # gate named before is admitted) is refused by the core's own sentence;
    # exposure carries the rest of the float half.
    params = {'black': 0.2, 'white': 0.8, 'gamma': 1.5}
    levels = app.call('add_adjustment_layer', document_id=source, op='levels',
                      params=params, name='Levels')
    copied = app.call('copy', document_id=source, layer=levels['layer'])
    assert copied['editable'] == 'adjust', copied
    float_doc = fresh_float(app, folder, 'g12-float')
    for label, target in (('8', new8(app)), ('16', new16(app))):
        pasted = app.call('paste', document_id=target)
        assert pasted['editable'] == {'type': 'adjust', 'restored': True}, (label, pasted)
        assert pasted['name'] == 'Levels', pasted
        restored = adjustment_rows(app, target, folder, 'g12-levels-' + label,
                                   typed=label != '8')
        assert len(restored) == 1 and restored[0][0] == 'levels', (label, restored)
        assert all(restored[0][1][key] == params[key] for key in params), (label, restored)
        out['levels_' + label] = pasted
    levels_float = fresh_float(app, folder, 'g12-levels-float')
    pasted = app.call('paste', document_id=levels_float)
    assert pasted['editable'] == {'type': 'adjust', 'restored': True}, pasted
    restored = adjustment_rows(app, levels_float, folder, 'g12-levels-float')
    assert len(restored) == 1 and restored[0][0] == 'levels', restored
    assert all(restored[0][1][key] == params[key] for key in params), restored
    out['levels_float'] = pasted
    # Invert is one of the adjustments a float document still refuses
    # (Vibrance, which this gate named before, is admitted).
    inverted = app.call('add_adjustment_layer', document_id=source, op='invert',
                        params={}, name='Invert')
    copied = app.call('copy', document_id=source, layer=inverted['layer'])
    assert copied['editable'] == 'adjust', copied
    history = cursor(app, float_doc)
    message = refusal(app, 'paste', document_id=float_doc)
    assert 'the invert adjustment is unavailable in a 32-bit float document' in message, \
        message
    assert cursor(app, float_doc) == history
    out['invert_float'] = message
    exposure = {'exposure': 0.75, 'evaluation': 'document_v2'}
    added = app.call('add_adjustment_layer', document_id=source, op='exposure',
                     params=exposure, name='Exposure')
    copied = app.call('copy', document_id=source, layer=added['layer'])
    assert copied['editable'] == 'adjust', copied
    pasted = app.call('paste', document_id=float_doc)
    assert pasted['editable'] == {'type': 'adjust', 'restored': True}, pasted
    restored = adjustment_rows(app, float_doc, folder, 'g12-exposure-float')
    assert len(restored) == 1 and restored[0][0] == 'exposure', restored
    assert all(restored[0][1][key] == exposure[key] for key in exposure), restored
    out['exposure_float'] = pasted

    # Objection 4: an adjustment copied IN the float document (a u16
    # byte-origin transparent payload) pastes back into it.
    copied = app.call('copy', document_id=float_doc, layer=pasted['layer'])
    assert copied['editable'] == 'adjust' and copied['typed'] and copied['depth'] == 16, copied
    again = app.call('paste', document_id=float_doc)
    assert again['editable']['restored'], again
    restored = adjustment_rows(app, float_doc, folder, 'g12-exposure-float-2')
    assert [op for op, _ in restored] == ['exposure', 'exposure'], restored
    out['exposure_in_float'] = again

    # Review finding 1: an 8-bit whole-layer adjustment Copy carries the
    # transparent canvas-sized payload (`transparentPayload()`'s rule) with
    # the record, and pastes back as the adjustment.
    byte_doc, _ = open_png(app, folder, 'g12-adjust8', W, H, seed=2)
    added = app.call('add_adjustment_layer', document_id=byte_doc, op='levels',
                     params=params, name='Levels 8')
    tiff = folder / 'g12-adjust8.tiff'
    copied = app.call('copy', document_id=byte_doc, layer=added['layer'],
                      public_copy_path=str(tiff))
    assert copied['typed'] and copied['depth'] == 8 and copied['editable'] == 'adjust', copied
    assert [copied['width'], copied['height'], copied['offset']] == [W, H, [0, 0]], copied
    png = folder / 'g12-adjust8.png'
    subprocess.run(['sips', '-s', 'format', 'png', str(tiff), '--out', str(png)],
                   check=True, capture_output=True)
    public = png_rgba(png.read_bytes())
    assert (public['width'], public['height']) == (W, H), public['width']
    assert not any(public['samples'][3::4]), 'the adjustment copy is not transparent'
    target = new8(app)
    pasted = app.call('paste', document_id=target)
    assert pasted['editable'] == {'type': 'adjust', 'restored': True}, pasted
    assert pasted['name'] == 'Levels 8', pasted
    restored = adjustment_rows(app, target, folder, 'g12-levels-8-from-8', typed=False)
    assert len(restored) == 1 and restored[0][0] == 'levels', restored
    assert all(restored[0][1][key] == params[key] for key in params), restored
    out['adjust_from_8'] = pasted

    app.call('select_rect', document_id=source, x=2, y=2, width=20, height=14)
    copied = app.call('copy', document_id=source, layer=text_layer)
    assert copied['editable'] is None, copied
    assert app.call('get_clipboard')['info']['editable'] is None
    raster = new16(app)
    pasted = app.call('paste', document_id=raster)
    assert pasted['editable'] is None and pasted['name'] == 'Pasted Layer', pasted
    assert 'text' not in layers(app, raster)[pasted['layer']], layers(app, raster)
    app.call('deselect', document_id=source)

    # A float document restores the editable artwork too (it
    # refused it before), the text re-rendered in linear light — one layer,
    # one step.
    app.call('copy', document_id=source, layer=text_layer)
    history = cursor(app, float_doc)
    count = len(layers(app, float_doc))
    pasted = app.call('paste', document_id=float_doc)
    assert pasted['editable'] == {'type': 'text', 'restored': True}, pasted
    assert pasted['depth'] == 32, pasted
    assert len(layers(app, float_doc)) == count + 1
    assert 'text' in layers(app, float_doc)[pasted['layer']], layers(app, float_doc)
    assert cursor(app, float_doc) == history + 1
    out['text_into_float'] = pasted
    return out


MIXED_WRITER = r"""
import AppKit
let arguments = CommandLine.arguments
let pasteboard = NSPasteboard(name: NSPasteboard.Name(arguments[1]))
let tiff = try! Data(contentsOf: URL(fileURLWithPath: arguments[2]))
let typed = try! Data(contentsOf: URL(fileURLWithPath: arguments[3]))
pasteboard.clearContents()
let item = NSPasteboardItem()
item.setData(typed, forType: NSPasteboard.PasteboardType("com.kgalligan.doctored.typed-clipboard"))
item.setData(tiff, forType: .tiff)
exit(pasteboard.writeObjects([item]) ? 0 : 1)
"""


def g13(app, folder):
    out = {}
    source = new16(app)
    added = app.call('add_text_layer', document_id=source, text='Title', x=5, y=6, size=20)
    source_row = layers(app, source)[added['layer']]
    app.call('copy', document_id=source)
    target = new16(app)
    app.call('start_recording', discard=True)
    pasted = app.call('paste', document_id=target, record_payload=True)
    stopped = app.call('stop_recording')
    assert pasted['editable']['restored'] and pasted['name'] == 'Title', pasted
    steps = [step for step in stopped['steps'] if step['tool'] == 'paste']
    assert len(steps) == 1 and steps[0]['arguments']['name'] == 'Title', stopped['steps']
    payload = Path(steps[0]['arguments']['payload_path'])
    app.call('save_action', name='g13 paste', steps=stopped['steps'], overwrite=True)
    bundles = folder / 'bundles'
    bundles.mkdir(exist_ok=True)
    packaged = app.call('package_action', name='g13 paste', destination=str(bundles),
                        overwrite=True)
    app.call('import_action', path=packaged['path'], name='g13 imported', overwrite=True)
    fresh = new16(app)
    app.call('run_action', name='g13 imported', document_id=fresh)
    row = row_named(app, fresh, 'Title')
    assert 'text' in row, row
    assert row['text']['string'] == source_row['text']['string'], row
    assert row['text']['origin'] == source_row['text']['origin'], (row, source_row)
    out['replay'] = {'step_name': steps[0]['arguments']['name'], 'row': row['name']}

    # Nit 3: a public TIFF beside an RZCB this build cannot read (version 99)
    # pastes the public image into an 8-bit document.
    byte_doc, _ = open_png(app, folder, 'g13-public', 12, 9, seed=4)
    tiff = folder / 'g13-public.tiff'
    app.call('copy', document_id=byte_doc, public_copy_path=str(tiff))
    future = bytearray(payload.read_bytes())
    future[4:8] = struct.pack('<I', 99)
    future_path = folder / 'g13-future.drcb'
    future_path.write_bytes(bytes(future))
    script = folder / 'mixed-writer.swift'
    script.write_text(MIXED_WRITER)
    subprocess.run(['xcrun', 'swift', str(script), os.environ['DR_PASTEBOARD_NAME'],
                    str(tiff), str(future_path)], check=True, capture_output=True)
    target8 = new8(app, 12, 9)
    public = app.call('paste', document_id=target8)
    assert public['external'] and not public['typed'], public
    out['future_beside_public'] = public

    # Objection 2: an unreadable payload_path refuses with its own decode
    # error — the pasteboard's public image is never pasted in its place.
    truncated = folder / 'g13-truncated.drcb'
    truncated.write_bytes(payload.read_bytes()[:-7])
    target8 = new8(app, 12, 9)
    history = cursor(app, target8)
    count = len(layers(app, target8))
    message = refusal(app, 'paste', document_id=target8, payload_path=str(truncated))
    assert 'clipboard payload' in message, message
    assert cursor(app, target8) == history and len(layers(app, target8)) == count
    out['truncated_payload_path'] = message

    # A public-only image has no canvas offset, so Paste in Place
    # lands it like Paste (the view, or the canvas centre with no window).
    write_public_tiff(folder, tiff)
    target16 = new16(app, width=12, height=9)
    history = cursor(app, target16)
    count = len(layers(app, target16))
    landed = app.call('paste', document_id=target16, placement='in_place')
    assert landed['external'] and landed['placement'] in ('view', 'centered'), landed
    assert len(layers(app, target16)) == count + 1
    assert cursor(app, target16) == history + 1
    out['public_in_place_16'] = landed
    return out


# --------------------------------------------------- g14 / g15 gates --

def g14(app, folder):
    out = {}
    # (a) A 16-bit text layer opens as that text layer, trimmed to its ink.
    source = new16(app)
    added = app.call('add_text_layer', document_id=source, text='Title', x=5, y=6, size=20)
    text_layer = added['layer']
    text_row = layers(app, source)[text_layer]
    grid = layer_grid(app, source, text_layer, W, H)
    box = alpha_box(grid, W, H)
    app.call('copy', document_id=source)
    made = app.call('new_from_clipboard')
    assert made['depth'] == 16, made
    assert made['editable'] == {'type': 'text', 'restored': True}, made
    assert [made['width'], made['height']] == [box[2], box[3]], (made, box)
    assert made['source_offset'] == [box[0], box[1]], (made, box)
    document = made['document']['id']
    rows = layers(app, document)
    assert len(rows) == 1 and 'text' in rows[0], rows
    assert rows[0]['text']['string'] == text_row['text']['string'], rows
    origin = text_row['text']['origin']
    want = {'x': origin['x'] - box[0], 'y': origin['y'] - box[1]}
    assert rows[0]['text']['origin'] == want, (rows[0]['text'], want)
    assert layer_grid(app, document, 0, box[2], box[3]) == windowed(grid, W, box), \
        'the restored text renders differently'
    history_untouched(app, document)
    assert len(layers(app, document)) == 1 and 'text' in layers(app, document)[0]
    out['text'] = made

    # (b) An 8-bit rect shape opens as that shape layer.
    source8 = new8(app)
    shape = app.call('add_shape_layer', document_id=source8, kind='rect', x=4, y=4, w=12,
                     h=8, fill='#cc3322')
    box = alpha_box(layer_grid(app, source8, shape['layer'], W, H), W, H)
    rendered = render(app, source8)
    app.call('copy', document_id=source8, layer=shape['layer'])
    made = app.call('new_from_clipboard')
    assert made['depth'] == 8 and made['editable'] == {'type': 'shape', 'restored': True}, made
    assert [made['width'], made['height']] == [box[2], box[3]], (made, box)
    document = made['document']['id']
    rows = layers(app, document)
    assert len(rows) == 1 and 'shape' in rows[0], rows
    got = render(app, document)
    pixels = [list(rendered[3][i:i + 4]) for i in range(0, len(rendered[3]), 4)]
    want = bytes(v for pixel in windowed(pixels, W, box) for v in pixel)
    assert got[:3] == (box[2], box[3], 8) and got[3] == want, 'the restored shape differs'
    out['shape'] = made

    # (c) Levels copied from 16 bits: an 8-bit document, the adjustment above
    # a transparent Background, never trimmed.
    params = {'black': 0.2, 'white': 0.8, 'gamma': 1.5}
    source16 = new16(app)
    levels = app.call('add_adjustment_layer', document_id=source16, op='levels',
                      params=params, name='Levels')
    tiff = folder / 'g14-levels.tiff'
    app.call('copy', document_id=source16, layer=levels['layer'], public_copy_path=str(tiff))
    made = app.call('new_from_clipboard')
    assert made['depth'] == 8 and made['origin_depth'] == 8, made
    assert [made['width'], made['height']] == [W, H] and not made['trimmed'], made
    assert made['editable'] == {'type': 'adjust', 'restored': True}, made
    document = made['document']['id']
    rows = layers(app, document)
    assert len(rows) == 2 and rows[1]['name'] == 'Levels', rows
    assert made['document']['active_layer'] == 1, made['document']
    restored = adjustment_rows(app, document, folder, 'g14-levels', typed=False)
    assert len(restored) == 1 and restored[0][0] == 'levels', restored
    assert all(restored[0][1][key] == params[key] for key in params), restored
    history_untouched(app, document)
    assert len(layers(app, document)) == 2
    out['levels'] = made

    # (d) Exposure pasted into a float document and copied back out of it.
    exposure = {'exposure': 0.75, 'evaluation': 'document_v2'}
    added = app.call('add_adjustment_layer', document_id=source16, op='exposure',
                     params=exposure, name='Exposure')
    app.call('copy', document_id=source16, layer=added['layer'])
    float_doc = fresh_float(app, folder, 'g14-float')
    pasted = app.call('paste', document_id=float_doc)
    assert pasted['editable'] == {'type': 'adjust', 'restored': True}, pasted
    app.call('copy', document_id=float_doc, layer=pasted['layer'])
    made = app.call('new_from_clipboard')
    assert made['depth'] == 8 and made['editable'] == {'type': 'adjust', 'restored': True}, made
    assert made['document']['projection_error'] is None, made['document']
    restored = adjustment_rows(app, made['document']['id'], folder, 'g14-exposure',
                               typed=False)
    assert len(restored) == 1 and restored[0][0] == 'exposure', restored
    assert all(restored[0][1][key] == exposure[key] for key in exposure), restored
    out['exposure_from_float'] = made

    # (e) A selection copy of the text layer carries no record.
    app.call('select_rect', document_id=source, x=2, y=2, width=20, height=14)
    app.call('copy', document_id=source, layer=text_layer)
    app.call('deselect', document_id=source)
    made = app.call('new_from_clipboard')
    assert made['editable'] is None, made
    rows = layers(app, made['document']['id'])
    assert len(rows) == 1 and rows[0]['name'] == 'Background' and 'text' not in rows[0], rows
    out['selection_copy'] = made

    # (f) A record this build cannot restore opens as pixels, with its reason.
    app.call('copy', document_id=source16, layer=levels['layer'])
    target = new16(app)
    app.call('start_recording', discard=True)
    app.call('paste', document_id=target, record_payload=True)
    stopped = app.call('stop_recording')
    step = next(step for step in stopped['steps'] if step['tool'] == 'paste')
    payload = bytearray(Path(step['arguments']['payload_path']).read_bytes())
    at = payload.rfind(b'levels')
    assert at >= 0, 'the recorded payload carries no levels record'
    payload[at:at + 6] = b'levelz'
    patched = folder / 'g14-levelz.drcb'
    patched.write_bytes(bytes(payload))
    script = folder / 'mixed-writer.swift'
    script.write_text(MIXED_WRITER)
    subprocess.run(['xcrun', 'swift', str(script), os.environ['DR_PASTEBOARD_NAME'],
                    str(tiff), str(patched)], check=True, capture_output=True)
    made = app.call('new_from_clipboard')
    assert made['editable']['type'] == 'adjust' and not made['editable']['restored'], made
    assert 'not one this build knows' in made['editable']['reason'], made
    assert len(layers(app, made['document']['id'])) == 1, made
    out['unknown_record'] = made
    return out


def g15(app, folder):
    out = {}
    source = new16(app)
    app.call('brush_stroke', document_id=source, layer=0, points=[[20, 12], [24, 15]], size=5,
             rgba=[0.7, 0.2, 0.1, 1])
    grid = layer_grid(app, source, 0, W, H)
    box = alpha_box(grid, W, H)
    assert box != (0, 0, W, H), box
    app.call('copy', document_id=source)
    made = app.call('new_from_clipboard')
    assert made['trimmed'] and made['editable'] is None, made
    assert [made['width'], made['height']] == [box[2], box[3]], (made, box)
    assert made['source_offset'] == [box[0], box[1]], (made, box)
    assert layer_grid(app, made['document']['id'], 0, box[2], box[3]) == \
        windowed(grid, W, box), 'the trimmed codes differ'
    out['u16'] = made

    float_doc = fresh_float(app, folder, 'g15-float')
    app.call('select_rect', document_id=float_doc, x=0, y=0, width=10, height=FH)
    app.call('cut', document_id=float_doc)
    app.call('deselect', document_id=float_doc)
    records = float_records(app, float_doc, folder, 'g15-source', FW, FH)
    assert len(records) == 1, len(records)
    record = records[0]
    pixels = [record[i:i + 4] for i in range(0, len(record), 4)]
    box = alpha_box([[0, 0, 0, 1 if p[3] > 0 else 0] for p in pixels], FW, FH)
    app.call('copy', document_id=float_doc)
    made = app.call('new_from_clipboard')
    assert made['depth'] == 32 and made['trimmed'], made
    assert [made['width'], made['height']] == [box[2], box[3]], (made, box)
    assert made['source_offset'] == [box[0], box[1]], (made, box)
    got = float_records(app, made['document']['id'], folder, 'g15-trimmed', box[2], box[3])
    assert len(got) == 1, len(got)
    want = [v for pixel in windowed(pixels, FW, box) for v in pixel]
    assert list(got[0]) == want, 'the trimmed float record differs'
    out['float'] = made
    return out


# ---------------------------------------------------------- g16 gate --

PASTE_RECT = (300, 220, 40, 30)
PW, PH = 400, 300


def visible_of(view):
    v = view['visible']
    return (v['x'], v['y'], v['width'], v['height'])


def get_visible(app, document):
    return visible_of(app.call('get_view', document_id=document))


def set_visible(app, document, **arguments):
    return visible_of(app.call('set_view', document_id=document, **arguments))


def settled_view(app, document, view):
    """`view`'s visible rect once two reads 0.2 s apart agree (at most 5 s).
    A view window opened a moment ago may still be settling its layout, and a
    one-point change in its height moves a Cmd-V's centring by a pixel."""
    last = visible_of(app.call('get_view', document_id=document, view=view))
    for _ in range(25):
        time.sleep(0.2)
        now = visible_of(app.call('get_view', document_id=document, view=view))
        if now == last:
            return now
        last = now
    return last


def contains(visible, rect):
    vx, vy, vw, vh = visible
    x, y, w, h = rect
    return x >= vx and y >= vy and x + w <= vx + vw and y + h <= vy + vh


def intersects(visible, rect):
    vx, vy, vw, vh = visible
    x, y, w, h = rect
    return x < vx + vw and vx < x + w and y < vy + vh and vy < y + h


def view_frame(visible):
    """`visible.integral`: the min edges floored, the max edges ceiled."""
    vx, vy, vw, vh = visible
    x0, y0 = math.floor(vx), math.floor(vy)
    return (x0, y0, math.ceil(vx + vw) - x0, math.ceil(vy + vh) - y0)


def selection_of(app, document):
    """The document's selection bounds [x, y, w, h], or None."""
    selection = app.call('get_document', document_id=document).get('selection')
    if not selection:
        return None
    return [selection['x'], selection['y'], selection['width'], selection['height']]


def framed(frame, w, h):
    """The core's `div_euclid` centring of a w × h window on a frame."""
    return [frame[0] + (frame[2] - w) // 2, frame[1] + (frame[3] - h) // 2]


def cutting_view(app, document, rect):
    """A zoom and centre whose visible rect cuts `rect` (intersects it
    without containing it)."""
    x, y, w, h = rect
    for zoom in (8, 16, 32):
        probe = set_visible(app, document, zoom=zoom, center={'x': PW / 2, 'y': PH / 2})
        vw, vh = probe[2], probe[3]
        for cx, cy in ((x + w / 2 - vw / 2, y + h / 2 - vh / 2),
                       (x + w / 2 + vw / 2, y + h / 2 + vh / 2),
                       (x + w / 2, y + h / 2)):
            if not (0 <= cx <= PW and 0 <= cy <= PH):
                continue
            visible = set_visible(app, document, zoom=zoom, center={'x': cx, 'y': cy})
            if intersects(visible, rect) and not contains(visible, rect):
                return visible
    raise AssertionError('no view cuts the source rect')


def g16_depth(app, folder, depth):
    out = {}
    x0, y0, w, h = PASTE_RECT
    if depth == 8:
        document, _ = open_png(app, folder, 'g16-source', PW, PH, seed=3)
    else:
        document = new16(app, width=PW, height=PH)
        app.call('brush_stroke', document_id=document, layer=0,
                 points=[[305, 225], [335, 245]], size=9, rgba=[0.2, 0.4, 0.6, 1])
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    copied = app.call('copy', document_id=document)
    assert copied['offset'] == [x0, y0] and [copied['width'], copied['height']] == [w, h], copied
    app.call('deselect', document_id=document)

    # (a) The whole source rect is visible: still centred on the view —
    # ⌘V never keeps a source position.
    visible = set_visible(app, document, fit=True)
    assert contains(visible, PASTE_RECT), visible
    want = framed(view_frame(visible), w, h)
    assert want != [x0, y0], want
    reply = app.call('paste', document_id=document)
    assert reply['placement'] == 'view' and reply['offset'] == want, (reply, visible, want)
    assert offset(layers(app, document)[reply['layer']]) == want, reply
    out['a_visible'] = reply

    # (b) Zoomed away from it: centred on the visible canvas.
    visible = set_visible(app, document, zoom=8, center={'x': 40, 'y': 30})
    assert not contains(visible, PASTE_RECT), visible
    want = framed(view_frame(visible), w, h)
    recording = depth == 16
    if recording:
        app.call('start_recording', discard=True)
    # The saved-payload step (`record_payload`) records `at`.
    reply = app.call('paste', document_id=document,
                     **({'record_payload': True} if recording else {}))
    assert reply['placement'] == 'view' and reply['offset'] == want, (reply, visible, want)
    assert offset(layers(app, document)[reply['layer']]) == want, reply
    out['b_view'] = {'reply': reply, 'visible': visible}
    if recording:
        stopped = app.call('stop_recording')
        steps = [step for step in stopped['steps'] if step['tool'] == 'paste']
        assert len(steps) == 1, stopped['steps']
        arguments = steps[0]['arguments']
        assert arguments['placement'] == 'at', arguments
        assert [arguments['x'], arguments['y']] == reply['offset'], (arguments, reply)
        app.call('save_action', name='g16 paste', steps=stopped['steps'], overwrite=True)
        fresh = new16(app, width=PW, height=PH)
        set_visible(app, fresh, fit=True)
        app.call('run_action', name='g16 paste', document_id=fresh)
        rows = layers(app, fresh)
        assert len(rows) == 2 and offset(rows[1]) == reply['offset'], rows
        old = new16(app, width=PW, height=PH)
        set_visible(app, old, fit=True)
        replayed = app.call('paste', document_id=old, payload_path=arguments['payload_path'],
                            placement='centered')
        centre = [(PW - w) // 2, (PH - h) // 2]
        assert replayed['placement'] == 'centered' and replayed['offset'] == centre, replayed
        out['recorded'] = {'step': arguments, 'replayed': rows[1], 'old_step': replayed}

    # (c) A view that cuts the rect: the whole rect is not visible.
    visible = cutting_view(app, document, PASTE_RECT)
    want = framed(view_frame(visible), w, h)
    reply = app.call('paste', document_id=document)
    assert reply['placement'] == 'view' and reply['offset'] == want, (reply, visible, want)
    out['c_cut'] = {'reply': reply, 'visible': visible}

    # (d) A destination selection: centred on it, unmasked, and (measured
    # against Photoshop) deselected in the same step;
    # Reselect restores it.
    app.call('select_rect', document_id=document, x=10, y=10, width=100, height=80)
    set_visible(app, document, fit=True)
    history = cursor(app, document)
    reply = app.call('paste', document_id=document)
    assert reply['placement'] == 'selection' and reply['offset'] == [40, 35], reply
    assert reply['mask'] is None, reply
    assert selection_of(app, document) is None, 'a pixel paste kept the selection'
    assert cursor(app, document) == history + 1, 'the paste is not one step'
    app.call('reselect', document_id=document)
    assert selection_of(app, document) == [10, 10, 100, 80], selection_of(app, document)
    app.call('deselect', document_id=document)
    out['d_selection'] = reply

    # (e) Other documents: centred on the view whether it shows the source
    # rect (a larger one) or not (a smaller one).
    larger = (new8(app, 1000, 800) if depth == 8
              else new16(app, width=1000, height=800))
    visible = set_visible(app, larger, fit=True)
    assert contains(visible, PASTE_RECT), visible
    want = framed(view_frame(visible), w, h)
    reply = app.call('paste', document_id=larger)
    assert reply['placement'] == 'view' and reply['offset'] == want, (reply, visible, want)
    out['e_larger'] = reply
    smaller = new8(app, 200, 150) if depth == 8 else new16(app, width=200, height=150)
    visible = set_visible(app, smaller, fit=True)
    assert not contains(visible, PASTE_RECT), visible
    frame = view_frame(visible)
    assert frame == (0, 0, 200, 150), frame
    reply = app.call('paste', document_id=smaller)
    want = framed(frame, w, h)
    assert want == [(200 - w) // 2, (150 - h) // 2], want
    assert reply['placement'] == 'view' and reply['offset'] == want, reply
    out['e_smaller'] = reply

    # (f) An explicit centered paste is the canvas centre, as before.
    reply = app.call('paste', document_id=document, placement='centered')
    centre = [(PW - w) // 2, (PH - h) // 2]
    assert reply['placement'] == 'centered' and reply['offset'] == centre, reply
    out['f_centered'] = reply
    return out


def g16_layers(app):
    source = new16(app, width=PW, height=PH)
    app.call('brush_stroke', document_id=source, layer=0,
             points=[[305, 225], [335, 245]], size=9, rgba=[0.7, 0.2, 0.1, 1])
    stroke = (295, 215, 50, 40)
    copied = app.call('copy', document_id=source, mode='layers', layers=[0])
    assert copied['mode'] == 'layers', copied
    target = new16(app, width=PW, height=PH)
    visible = set_visible(app, target, fit=True)
    assert contains(visible, (0, 0, PW, PH)), visible
    # Copied layers are centred on the view too, never in place.
    fit = app.call('paste', document_id=target)
    assert fit['mode'] == 'layers', fit
    assert fit['placement'] == 'view' and fit['offset'] != {'x': 0, 'y': 0}, fit
    visible = set_visible(app, target, zoom=8, center={'x': 40, 'y': 30})
    assert not intersects(visible, stroke), visible
    away = app.call('paste', document_id=target)
    assert away['mode'] == 'layers' and away['placement'] == 'view', away
    app.call('select_rect', document_id=target, x=10, y=10, width=100, height=80)
    set_visible(app, target, fit=True)
    selected = app.call('paste', document_id=target)
    assert selected['mode'] == 'layers' and selected['placement'] == 'selection', selected
    # A layers paste keeps the selection.
    assert selection_of(app, target) == [10, 10, 100, 80], selection_of(app, target)
    app.call('deselect', document_id=target)
    return {'fit': fit, 'away': away, 'selection': selected}


def g16(app, folder):
    return {'8': g16_depth(app, folder, 8), '16': g16_depth(app, folder, 16),
            'layers': g16_layers(app)}


# ---------------------------------------------------------- g17 gate --

# The colour chain g17's oracle uses, written here from the standards: the
# IEC 61966-2.1 decode (Display P3 shares the sRGB transfer) and the RGB →
# XYZ matrices of the two profiles' primaries at their common D65 white —
# which is all the profile descriptions name, so the D50 adaptation an ICC
# profile carries cancels between them.
SRGB_PRIMARIES = ((0.64, 0.33), (0.30, 0.60), (0.15, 0.06))
P3_PRIMARIES = ((0.680, 0.320), (0.265, 0.690), (0.150, 0.060))
D65 = (0.3127, 0.3290)
FLOAT_TOLERANCE = 1e-4


def srgb_decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def inverse3(m):
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def matmul(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def apply3(m, v):
    return [sum(m[r][k] * v[k] for k in range(3)) for r in range(3)]


def rgb_to_xyz(primaries, white):
    def xyz(x, y):
        return [x / y, 1.0, (1.0 - x - y) / y]
    columns = [xyz(*p) for p in primaries]
    m = [[columns[c][r] for c in range(3)] for r in range(3)]
    scale = apply3(inverse3(m), xyz(*white))
    return [[m[r][c] * scale[c] for c in range(3)] for r in range(3)]


P3_TO_SRGB = matmul(inverse3(rgb_to_xyz(SRGB_PRIMARIES, D65)), rgb_to_xyz(P3_PRIMARIES, D65))


def promoted_oracle(app, folder, source, label, matrix):
    """`source`'s whole 16-bit layer pasted into a float sRGB document:
    every linear component within FLOAT_TOLERANCE of the analytic decode
    (then `matrix`), alpha code / 65535."""
    grid = layer_grid(app, source, 0, W, H)
    app.call('copy', document_id=source)
    target = fresh_float(app, folder, label)
    history = cursor(app, target)
    reply = app.call('paste', document_id=target)
    assert reply['depth'] == 32 and reply['typed'] and reply['converted'], reply
    assert cursor(app, target) == history + 1
    records = float_records(app, target, folder, label, W, H)
    assert len(records) == 1, len(records)
    got = records[0]
    worst = 0.0
    for i, code in enumerate(grid):
        linear = [srgb_decode(code[c] / 65535.0) for c in range(3)]
        if matrix is not None:
            linear = apply3(matrix, linear)
        for c in range(3):
            worst = max(worst, abs(got[4 * i + c] - linear[c]))
        assert abs(got[4 * i + 3] - code[3] / 65535.0) <= 1e-7, (i, got[4 * i + 3], code)
    assert worst <= FLOAT_TOLERANCE, (label, worst)
    return {'reply': reply, 'worst': worst}


def recorded_paste(app, document, **arguments):
    """One paste under a recording: (reply, the step's arguments, steps)."""
    app.call('start_recording', discard=True)
    reply = app.call('paste', document_id=document, **arguments)
    stopped = app.call('stop_recording')
    steps = [step for step in stopped['steps'] if step['tool'] == 'paste']
    assert len(steps) == 1, stopped['steps']
    return reply, steps[0]['arguments'], stopped['steps']


def g17(app, folder):
    out = {}
    # (a) 16-bit sRGB and Display P3 copies promoted into float.
    out['srgb_into_float'] = promoted_oracle(app, folder, painted16(app), 'g17-srgb-float',
                                             None)
    p3 = painted16(app, profile='display_p3')
    out['p3_into_float'] = promoted_oracle(app, folder, p3, 'g17-p3-float', P3_TO_SRGB)

    # (b) An 8-bit image from another app into float: no typed payload, a
    # public TIFF; the result is linear light.
    byte_doc, source = open_png(app, folder, 'g17-external', 12, 9, seed=6)
    tiff = folder / 'g17-external.tiff'
    app.call('copy', document_id=byte_doc, public_copy_path=str(tiff))
    write_public_tiff(folder, tiff)
    clip = app.call('get_clipboard')
    assert not clip['typed'] and 'tiff' in clip['public_types'], clip
    target = fresh_float(app, folder, 'g17-external-float')
    reply = app.call('paste', document_id=target)
    assert reply['external'] and reply['depth'] == 32, reply
    got = float_records(app, target, folder, 'g17-external-float', 12, 9)
    assert len(got) == 1, len(got)
    worst = 0.0
    for i in range(12 * 9):
        for c in range(3):
            worst = max(worst, abs(got[0][4 * i + c] - srgb_decode(source[4 * i + c] / 255.0)))
        assert got[0][4 * i + 3] == source[4 * i + 3] / 255.0, (i, got[0][4 * i + 3])
    assert worst <= FLOAT_TOLERANCE, worst
    out['external_into_float'] = {'reply': reply, 'worst': worst}

    # (c) Preserve keeps the numbers: 16-bit P3 into 16-bit sRGB …
    codes = layer_grid(app, p3, 0, W, H)
    app.call('copy', document_id=p3)
    target = new16(app)
    kept = app.call('paste', document_id=target, profile='preserve', placement='in_place')
    assert kept['profile'] == 'preserved', kept
    assert layer_grid(app, target, kept['layer'], W, H) == codes, 'preserve changed the codes'
    out['preserve_16'] = kept
    # … and an external P3-tagged TIFF into 8-bit sRGB, which Convert converts.
    p3_8, p3_source = open_png(app, folder, 'g17-p3-8', 12, 9, seed=7)
    app.call('assign_profile', document_id=p3_8, profile='display_p3')
    tiff = folder / 'g17-p3-8.tiff'
    app.call('copy', document_id=p3_8, public_copy_path=str(tiff))
    write_public_tiff(folder, tiff)
    want = [list(p3_source[i:i + 4]) for i in range(0, len(p3_source), 4)]
    target = new8(app, 12, 9)
    kept = app.call('paste', document_id=target, profile='preserve')
    assert kept['external'] and kept['profile'] == 'preserved', kept
    assert layer_grid(app, target, kept['layer'], 12, 9) == want, 'preserve changed the bytes'
    target = new8(app, 12, 9)
    converted = app.call('paste', document_id=target)
    assert converted['external'] and converted['profile'] == 'converted', converted
    assert layer_grid(app, target, converted['layer'], 12, 9) != want, 'convert kept the bytes'
    out['external_p3_into_8'] = {'preserved': kept, 'converted': converted}

    # (d) A recorded paste (live) writes no `convert` and, with
    # the same copy still on the clipboard, replays the same layer; Don't
    # Convert is recorded as profile "preserve".
    app.call('copy', document_id=p3)
    target = new16(app)
    reply, arguments, steps = recorded_paste(app, target)
    assert 'convert' not in arguments and 'profile' not in arguments, arguments
    assert 'payload_path' not in arguments, arguments
    app.call('save_action', name='g17 paste', steps=steps, overwrite=True)
    fresh = new16(app)
    app.call('run_action', name='g17 paste', document_id=fresh)
    assert len(layers(app, fresh)) == 2
    assert layer_grid(app, fresh, 1, W, H) == layer_grid(app, target, reply['layer'], W, H), \
        'the replayed paste differs from the recorded one'
    target = new16(app)
    reply, arguments, steps = recorded_paste(app, target, profile='preserve')
    assert arguments.get('profile') == 'preserve' and 'convert' not in arguments, arguments
    app.call('save_action', name='g17 preserve', steps=steps, overwrite=True)
    fresh = new16(app)
    app.call('run_action', name='g17 preserve', document_id=fresh)
    assert layer_grid(app, fresh, 1, W, H) == codes, 'the replayed Don\'t Convert differs'
    out['recorded'] = {'convert': 'absent', 'preserve': arguments.get('profile')}

    # (e) Replays into 8 bits: a paste recorded in a 16-bit Display P3
    # document converts into 8-bit sRGB; a float payload file pastes into 8
    # bits; Preserve pastes a P3 payload file's numbers.
    app.call('copy', document_id=p3)
    target = new16(app, profile='display_p3')
    _, arguments, steps = recorded_paste(app, target, record_payload=True)
    p3_file = arguments['payload_path']
    app.call('save_action', name='g17 p3 paste', steps=steps, overwrite=True)
    fresh8 = new8(app)
    app.call('run_action', name='g17 p3 paste', document_id=fresh8)
    assert len(layers(app, fresh8)) == 2, layers(app, fresh8)
    target8 = new8(app)
    replayed = app.call('paste', document_id=target8, payload_path=p3_file)
    assert replayed['typed'] and replayed['converted'], replayed
    assert replayed['profile'] == 'converted', replayed
    assert layer_grid(app, fresh8, 1, W, H) == layer_grid(app, target8, replayed['layer'], W, H)
    float_doc = fresh_float(app, folder, 'g17-float-file')
    app.call('copy', document_id=float_doc)
    float_target = fresh_float(app, folder, 'g17-float-file-2')
    _, arguments, _ = recorded_paste(app, float_target, record_payload=True)
    target8 = new8(app, FW, FH)
    narrowed = app.call('paste', document_id=target8, payload_path=arguments['payload_path'])
    assert narrowed['typed'] and narrowed['narrowed'] and narrowed['depth'] == 8, narrowed
    target8 = new8(app)
    kept = app.call('paste', document_id=target8, payload_path=p3_file, profile='preserve',
                    placement='in_place')
    assert kept['profile'] == 'preserved', kept
    want = [[(v + 128) // 257 for v in pixel] for pixel in codes]
    assert layer_grid(app, target8, kept['layer'], W, H) == want, 'preserve changed the numbers'
    out['replay_into_8'] = {'p3': replayed, 'float': narrowed, 'preserve': kept}

    # (f) Layers: P3 → sRGB at 16 bits converts with no argument and keeps
    # the numbers under preserve; a clip holding a placed image keeps its
    # codes under preserve and converts with no argument.
    app.call('copy', document_id=p3, mode='layers', layer=0)
    target = new16(app)
    converted = app.call('paste', document_id=target)
    assert converted['mode'] == 'layers' and converted['profile'] == 'converted', converted
    target = new16(app)
    kept = app.call('paste', document_id=target, profile='preserve')
    assert kept['mode'] == 'layers' and kept['profile'] == 'preserved', kept
    assert layer_grid(app, target, kept['layer'], W, H) == codes, 'layers preserve changed codes'
    placed_doc = new16(app, profile='display_p3')
    png = folder / 'g17-placed.png'
    write_png8(png, W, H, byte_pattern(W, H, seed=8))
    placed = app.call('place_image', document_id=placed_doc, path=str(png), name='Placed',
                      x=0, y=0)
    app.call('copy', document_id=placed_doc, mode='layers', layer=placed['layer'])
    target = new16(app)
    history = cursor(app, target)
    kept = app.call('paste', document_id=target, profile='preserve', placement='in_place')
    assert kept['profile'] == 'preserved' and not kept['profile_converted'], kept
    assert cursor(app, target) == history + 1
    assert layer_grid(app, target, kept['layer'], W, H) == \
        layer_grid(app, placed_doc, placed['layer'], W, H), 'placed preserve changed codes'
    target = new16(app)
    history = cursor(app, target)
    converted = app.call('paste', document_id=target)
    assert converted['profile'] == 'converted', converted
    assert cursor(app, target) == history + 1
    out['layers'] = {'placed_preserved': kept, 'placed_converted': converted}
    return out


# ---------------------------------------------------------- g18 gate --

PASTEBOARD_CLEARER = """
import AppKit
let pasteboard = NSPasteboard(name: NSPasteboard.Name(CommandLine.arguments[1]))
pasteboard.clearContents()
exit(0)
"""
LIVE_PASTE = {'placement': 'auto', 'layer': {'$layer': 'active'}}
RECORD_PAYLOAD_REFUSAL = 'record_payload saves a pixel payload; pass mode "pixels".'
NO_IMAGE = 'The clipboard holds no image.'


def clear_pasteboard(folder):
    """Empties the drive's private pasteboard (`write_public_tiff`'s pattern)."""
    script = folder / 'pasteboard-clearer.swift'
    script.write_text(PASTEBOARD_CLEARER)
    subprocess.run(['xcrun', 'swift', str(script), os.environ['DR_PASTEBOARD_NAME']],
                   check=True, capture_output=True)


def stroked16(app, points, rgba, width=W, height=H):
    document = new16(app, width=width, height=height)
    app.call('brush_stroke', document_id=document, layer=0, points=points, size=7, rgba=rgba)
    return document


def recorded(app, calls):
    """`calls` ((tool, arguments) pairs) under one recording: (replies, steps)."""
    app.call('start_recording', discard=True)
    replies = [app.call(tool, **arguments) for tool, arguments in calls]
    return replies, app.call('stop_recording')['steps']


def played(app, name, steps, document):
    app.call('save_action', name=name, steps=steps, overwrite=True)
    report = app.call('run_action', name=name, document_id=document)
    assert report['ok'], report
    return report


def g18_pixels(app, folder):
    """(a) the live step, and (e) the empty clipboard."""
    store = folder / 'owner' / 'clipboard-payloads'

    def stored():
        return sorted(p.name for p in store.glob('*.drcb')) if store.is_dir() else []

    a = stroked16(app, [[4, 5], [30, 18]], [0.2, 0.4, 0.6, 1])
    b = stroked16(app, [[6, 24], [34, 6]], [0.9, 0.3, 0.1, 1])
    assert layer_grid(app, a, 0, W, H) != layer_grid(app, b, 0, W, H)
    app.call('copy', document_id=a)
    target = new16(app)
    before = stored()
    reply, arguments, steps = recorded_paste(app, target)
    assert arguments == LIVE_PASTE, arguments
    assert not any(key in arguments for key in ('payload_path', 'x', 'y', 'name', 'mode'))
    assert stored() == before, 'a live paste wrote a payload file'
    app.call('save_action', name='g18 live paste', steps=steps, overwrite=True)
    bundles = folder / 'bundles'
    bundles.mkdir(exist_ok=True)
    bundle = Path(app.call('package_action', name='g18 live paste', destination=str(bundles),
                           overwrite=True)['path'])
    action = json.loads((bundle / 'action.json').read_text())
    step = next(step for step in action['steps'] if step['tool'] == 'paste')
    assert 'payload_path' not in step['arguments'], step
    assets = bundle / 'assets'
    assert not assets.exists() or not any(assets.iterdir()), 'a live paste packaged an asset'
    # A DIFFERENT copy on the clipboard: the replay pastes it, not the recorded one.
    app.call('copy', document_id=b)
    fresh = new16(app)
    app.call('run_action', name='g18 live paste', document_id=fresh)
    direct = new16(app)
    pasted = app.call('paste', document_id=direct)
    assert len(layers(app, fresh)) == 2, layers(app, fresh)
    grid = layer_grid(app, fresh, 1, W, H)
    assert grid == layer_grid(app, direct, pasted['layer'], W, H), \
        'the replay differs from a direct paste of the clipboard'
    assert grid != layer_grid(app, target, reply['layer'], W, H), \
        'the replay pasted the recorded pixels'

    # (e) Nothing on the clipboard: the step fails by name, nothing changes.
    clear_pasteboard(folder)
    clip = app.call('get_clipboard')
    assert not clip['typed'] and clip['public_types'] == [] and clip['layers'] is None, clip
    empty = new16(app)
    history = cursor(app, empty)
    count = len(layers(app, empty))
    report = app.call('run_action', name='g18 live paste', document_id=empty)
    assert report['ok'] is False, report
    failure = report['failures'][0]
    assert failure['tool'] == 'paste' and NO_IMAGE in failure['reason'], report
    assert cursor(app, empty) == history and len(layers(app, empty)) == count
    return {'step': arguments, 'bundle': str(bundle), 'empty': failure}


def g18_placement(app):
    """(b) a live step re-decides its landing; (c) requests record themselves."""
    out = {}
    x0, y0, w, h = PASTE_RECT
    document = stroked16(app, [[305, 225], [335, 245]], [0.2, 0.4, 0.6, 1], PW, PH)
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    app.call('copy', document_id=document)
    app.call('deselect', document_id=document)
    visible = set_visible(app, document, zoom=8, center={'x': 40, 'y': 30})
    assert not contains(visible, PASTE_RECT), visible
    reply, arguments, steps = recorded_paste(app, document)
    assert reply['placement'] == 'view', reply
    assert arguments == LIVE_PASTE, arguments
    fresh = new16(app, width=PW, height=PH)
    visible = set_visible(app, fresh, fit=True)
    assert contains(visible, PASTE_RECT), visible
    played(app, 'g18 placement', steps, fresh)
    rows = layers(app, fresh)
    # The replay lands by the replaying view — its centre.
    want = framed(view_frame(visible), w, h)
    assert len(rows) == 2 and offset(rows[1]) == want, (rows, want)
    assert offset(rows[1]) != reply['offset'], (rows[1], reply)
    out['b'] = {'recorded_at': reply['offset'], 'replayed_at': offset(rows[1])}

    target = new16(app, width=PW, height=PH)
    _, arguments, _ = recorded_paste(app, target, placement='in_place')
    assert arguments == dict(LIVE_PASTE, placement='in_place'), arguments
    app.call('select_rect', document_id=target, x=10, y=10, width=100, height=80)
    _, arguments, _ = recorded_paste(app, target, placement='into')
    assert arguments == dict(LIVE_PASTE, placement='into'), arguments
    _, arguments, _ = recorded_paste(app, target, profile='preserve')
    assert arguments == dict(LIVE_PASTE, profile='preserve'), arguments
    _, arguments, _ = recorded_paste(app, target, placement='at', x=3, y=4, name='Named',
                                     mode='pixels')
    assert arguments == dict(LIVE_PASTE, placement='at', x=3, y=4, name='Named',
                             mode='pixels'), arguments
    out['c'] = arguments
    return out


def g18_layers(app):
    """(d) a layers paste is the same live step; (f) the refusal."""
    source = stroked16(app, [[4, 5], [30, 18]], [0.2, 0.4, 0.6, 1])
    app.call('new_layer', document_id=source, name='Top')
    app.call('brush_stroke', document_id=source, layer=1, points=[[2, 20], [22, 6]], size=7,
             rgba=[0.9, 0.15, 0.3, 1])
    copied = app.call('copy', document_id=source, mode='layers', layers=[0, 1])
    assert copied['mode'] == 'layers', copied
    target = new16(app)
    reply, arguments, steps = recorded_paste(app, target)
    assert reply['mode'] == 'layers', reply
    assert arguments == LIVE_PASTE, arguments
    assert all(step['tool'] != '__unrecorded' for step in steps), steps
    # (f) asked while the layers copy is held.
    message = refusal(app, 'paste', document_id=target, mode='layers', record_payload=True)
    assert RECORD_PAYLOAD_REFUSAL in message, message
    fresh = new16(app)
    set_visible(app, fresh, fit=True)
    played(app, 'g18 layers paste', steps, fresh)
    names = [row['name'] for row in layers(app, fresh)]
    assert len(names) == 3 and 'Top' in names, names
    # The same step after a pixel copy pastes one pixel layer.
    app.call('copy', document_id=source, layer=1)
    assert app.call('get_clipboard')['layers'] is None
    pixel = new16(app)
    played(app, 'g18 layers paste', steps, pixel)
    assert len(layers(app, pixel)) == 2, layers(app, pixel)
    return {'step': arguments, 'refusal': message, 'layers_replay': names}


def g18_documents(app, folder, depth):
    """(g) Select All → Copy → Paste replays on ANOTHER document."""
    def make(label, seed, points, rgba):
        if depth == 8:
            return open_png(app, folder, f'g18-{label}', W, H, seed=seed)[0]
        return stroked16(app, points, rgba)

    a = make('a', 11, [[4, 5], [30, 18]], [0.2, 0.4, 0.6, 1])
    b = make('b', 12, [[6, 24], [34, 6]], [0.9, 0.3, 0.1, 1])
    c = make('c', 13, [[20, 2], [20, 28]], [0.1, 0.8, 0.2, 1])
    grids = {name: layer_grid(app, document, 0, W, H)
             for name, document in (('a', a), ('b', b), ('c', c))}
    assert grids['a'] != grids['b'] != grids['c'] != grids['a']
    _, steps = recorded(app, [('select_all', {'document_id': a}), ('copy', {'document_id': a}),
                              ('paste', {'document_id': a})])
    assert [step['tool'] for step in steps] == ['select_all', 'copy', 'paste'], steps
    copy_step = steps[1]['arguments']
    assert set(copy_step) <= {'mode', 'layer', 'layers', 'index'}, copy_step
    assert 'payload_path' not in steps[2]['arguments'], steps[2]
    app.call('copy', document_id=c)
    played(app, f'g18 copy paste {depth}', steps, b)
    rows = layers(app, b)
    assert len(rows) == 2, rows
    grid = layer_grid(app, b, 1, W, H)
    assert grid == grids['b'], 'the replayed paste is not the replaying document\'s own pixels'
    assert grid != grids['a'] and grid != grids['c']
    return {'steps': [step['tool'] for step in steps], 'copy': copy_step}


def g18_copies(app):
    """(h) copy_merged and the layers copy are steps."""
    a = stroked16(app, [[4, 5], [30, 18]], [0.2, 0.4, 0.6, 1])
    b = stroked16(app, [[6, 24], [34, 6]], [0.9, 0.3, 0.1, 1])
    replies, steps = recorded(app, [('copy_merged', {'document_id': a}),
                                    ('copy', {'document_id': a, 'mode': 'layers'}),
                                    ('paste', {'document_id': a})])
    assert [step['tool'] for step in steps] == ['copy_merged', 'copy', 'paste'], steps
    assert steps[0]['arguments'] == {} and steps[1]['arguments'] == {'mode': 'layers'}, steps
    assert replies[2]['mode'] == 'layers', replies[2]
    want = layer_grid(app, b, 0, W, H)
    set_visible(app, b, fit=True)
    delta = centred_layer_delta(app, b, want)
    played(app, 'g18 copies', steps, b)
    rows = layers(app, b)
    assert len(rows) == 2, rows
    assert moved_grid(layer_grid(app, b, 1, W, H), want, delta), \
        'the pasted layer is not the document\'s own, centred on the view'
    return {'steps': [step['tool'] for step in steps], 'delta': delta}


def centred_layer_delta(app, document, grid):
    """How far a layers ⌘V of the W × H layer `grid` moves in
    `document` — its content box (alpha > 0) centred on the visible canvas."""
    box = alpha_box(grid, W, H)
    at = framed(view_frame(get_visible(app, document)), box[2], box[3])
    return (at[0] - box[0], at[1] - box[1])


def moved_grid(got, want, delta):
    """`got` (W × H, row-major) holds every pixel of `want` moved by `delta`
    that stays on the canvas."""
    dx, dy = delta
    for y in range(H):
        for x in range(W):
            tx, ty = x + dx, y + dy
            if 0 <= tx < W and 0 <= ty < H and want[y * W + x][3] != 0:
                if got[ty * W + tx] != want[y * W + x]:
                    return False
    return True


def g18_auto(app, folder, depth):
    """(i) `copy {mode: "auto"}`, the step Edit ▸ Copy records, re-decides on
    the replaying document: recorded with a selection and replayed without
    one it copies layers, and the reverse copies the selected pixels."""
    def make(label, seed, points, rgba):
        if depth == 8:
            return open_png(app, folder, f'g18-auto-{label}', W, H, seed=seed)[0]
        return stroked16(app, points, rgba)

    x0, y0, w, h = 5, 4, 12, 9
    out = {}
    # Every document first: a read straight after an 8-bit open can find the
    # worker still busy with it.
    a = make('a', 21, [[4, 5], [30, 18]], [0.2, 0.4, 0.6, 1])
    b = make('b', 22, [[6, 24], [34, 6]], [0.9, 0.3, 0.1, 1])
    c = make('c', 23, [[20, 2], [20, 28]], [0.1, 0.8, 0.2, 1])
    d = make('d', 24, [[4, 5], [30, 18]], [0.5, 0.2, 0.7, 1])
    # Recorded WITH a selection: a pixel copy.
    app.call('select_rect', document_id=a, x=x0, y=y0, width=w, height=h)
    replies, steps = recorded(app, [('copy', {'document_id': a, 'mode': 'auto'}),
                                    ('paste', {'document_id': a})])
    assert replies[0].get('mode') != 'layers' and replies[0]['offset'] == [x0, y0], replies[0]
    assert [replies[0]['width'], replies[0]['height']] == [w, h], replies[0]
    assert app.call('get_clipboard')['layers'] is None
    assert [step['tool'] for step in steps] == ['copy', 'paste'], steps
    assert steps[0]['arguments'] == {'mode': 'auto'}, steps[0]
    assert steps[1]['arguments'] == LIVE_PASTE, steps[1]
    # Replayed with NO selection: the replaying document's layers.
    want = layer_grid(app, b, 0, W, H)
    set_visible(app, b, fit=True)
    delta = centred_layer_delta(app, b, want)
    played(app, f'g18 auto selected {depth}', steps, b)
    held = app.call('get_clipboard')['layers']
    assert held is not None and held['records'] == 1, held
    rows = layers(app, b)
    assert len(rows) == 2, rows
    assert moved_grid(layer_grid(app, b, 1, W, H), want, delta), \
        'the layers replay is not the document\'s own, centred on the view'
    out['selected_then_none'] = held

    # Recorded with NO selection: a layers copy.
    replies, steps = recorded(app, [('copy', {'document_id': c, 'mode': 'auto'}),
                                    ('paste', {'document_id': c})])
    assert replies[0]['mode'] == 'layers' and replies[1]['mode'] == 'layers', replies
    assert steps[0]['arguments'] == {'mode': 'auto'}, steps[0]
    assert steps[1]['arguments'] == LIVE_PASTE, steps[1]
    # Replayed WITH a selection: the selected pixels of the active layer.
    window = window_codes(app, d, 0, x0, y0, w, h)
    app.call('select_rect', document_id=d, x=x0, y=y0, width=w, height=h)
    played(app, f'g18 auto none {depth}', steps, d)
    clip = app.call('get_clipboard')
    assert clip['layers'] is None and clip['typed'], clip
    assert [clip['info']['width'], clip['info']['height']] == [w, h], clip
    assert clip['info']['offset'] == [x0, y0], clip
    rows = layers(app, d)
    assert len(rows) == 2 and offset(rows[1]) == [x0, y0], rows
    assert window_codes(app, d, 1, x0, y0, w, h) == window, 'the pixel replay differs'
    out['none_then_selected'] = clip['info']
    return out


def g18_auto_float(app, folder):
    """`copy {mode: "auto"}` asks the capability family of the mode it
    resolves to: at float it answers exactly as the explicit mode does."""
    def outcome(document, **arguments):
        reply = app.client.rpc('tools/call', {
            'name': 'copy', 'arguments': dict(arguments, document_id=document)})
        text = ' '.join(row.get('text', '') for row in reply.get('content', []))
        if reply.get('isError'):
            return ('refused', text)
        body = json.loads(text)
        return ('layers' if body.get('mode') == 'layers' else 'pixels',
                [body.get('width'), body.get('height')])

    document = fresh_float(app, folder, 'g18-float')
    none = (outcome(document, mode='auto'), outcome(document, mode='layers'))
    assert none[0] == none[1] and none[0][0] != 'pixels', none
    app.call('select_rect', document_id=document, x=2, y=3, width=20, height=12)
    selected = (outcome(document, mode='auto'), outcome(document, mode='pixels'))
    assert selected[0] == selected[1] == ('pixels', [20, 12]), selected
    app.call('deselect', document_id=document)
    return {'no_selection': none[0], 'selection': selected[0]}


def g18(app, folder):
    out = {'layers': g18_layers(app), 'placement': g18_placement(app)}
    for depth in (8, 16):
        out[f'documents_{depth}'] = g18_documents(app, folder, depth)
        out[f'auto_{depth}'] = g18_auto(app, folder, depth)
    out['copies'] = g18_copies(app)
    out['auto_float'] = g18_auto_float(app, folder)
    # Last: it ends with the private pasteboard cleared.
    out['pixels'] = g18_pixels(app, folder)
    return out


# ---------------------------------------------------------- g19 gate --

FLOAT_SENTENCE = 'cannot be pasted into a 32-bit float document'
EMPTY_FLOAT_REPORT = {'rasterized': [], 'blend_reset': [], 'effect_blend_reset': []}


def float_report(reply):
    return {key: reply[key] for key in EMPTY_FLOAT_REPORT}


def projection_errors(value):
    """Every `projection_error` in a `get_document` reply."""
    found = []
    if isinstance(value, dict):
        for key, inner in value.items():
            if key == 'projection_error':
                found.append(inner)
            elif key != 'layers':
                found += projection_errors(inner)
    return found


def relative_corner(x, w, source, canvas):
    """Paste in Place across canvas sizes: the copy's centre keeps its
    fraction of the canvas, the corner floored."""
    return ((2 * x + w) * canvas - w * source) // (2 * source)


def relative_at(rect, source, canvas):
    x, y, w, h = rect
    return [relative_corner(x, w, source[0], canvas[0]),
            relative_corner(y, h, source[1], canvas[1])]


def union_box(boxes):
    x0 = min(b[0] for b in boxes)
    y0 = min(b[1] for b in boxes)
    x1 = max(b[0] + b[2] for b in boxes)
    y1 = max(b[1] + b[3] for b in boxes)
    return (x0, y0, x1 - x0, y1 - y0)


def layers_into_float(app, folder, source, roots, label, union=None, **arguments):
    """Copies `roots` of `source` (W × H) as layers and pastes them in place
    into a fresh float document (FW × FH): (target, reply), one history step,
    no projection error. Across the two canvas sizes the paste lands
    `relative_in_place`, by the rule over `union` (the roots'
    content box) when the caller knows it."""
    copied = app.call('copy', document_id=source, mode='layers', layers=roots)
    assert copied['mode'] == 'layers', copied
    target = fresh_float(app, folder, label)
    history = cursor(app, target)
    reply = app.call('paste', document_id=target, mode='layers', placement='in_place',
                     **arguments)
    assert reply['mode'] == 'layers' and reply['depth'] == 32, reply
    assert reply['placement'] == 'relative_in_place', reply
    if union is not None:
        at = relative_at(union, (W, H), (FW, FH))
        assert reply['offset'] == {'x': at[0] - union[0], 'y': at[1] - union[1]}, \
            (reply, union, at)
    assert cursor(app, target) == history + 1, 'a layers paste is one history step'
    errors = projection_errors(app.call('get_document', document_id=target))
    assert errors and all(error is None for error in errors), errors
    return target, reply


def float_refusal(app, folder, source, roots, label, layer, feature):
    """The same paste REFUSED: the sentence names `layer` and `feature`, and
    the float document keeps its history cursor and its layers."""
    app.call('copy', document_id=source, mode='layers', layers=roots)
    target = fresh_float(app, folder, label)
    history, count = cursor(app, target), len(layers(app, target))
    message = refusal(app, 'paste', document_id=target, mode='layers')
    assert f'the copied layer “{layer}” {FLOAT_SENTENCE}: {feature}' in message, message
    assert cursor(app, target) == history and len(layers(app, target)) == count
    return message


def decoded_records(app, folder, target, label, grids, scale, matrix=None):
    """The W × H float records of `target` against the analytic decode of
    `grids` (one code grid per pasted raster, bottom first): the worst error."""
    records = float_records(app, target, folder, label, W, H)
    assert len(records) == len(grids), (label, len(records), len(grids))
    worst = 0.0
    for got, grid in zip(records, grids):
        for i, code in enumerate(grid):
            linear = [srgb_decode(code[c] / scale) for c in range(3)]
            if matrix is not None:
                linear = apply3(matrix, linear)
            for c in range(3):
                worst = max(worst, abs(got[4 * i + c] - linear[c]))
            assert abs(got[4 * i + 3] - code[3] / scale) <= 1e-7, (label, i, code)
    assert worst <= FLOAT_TOLERANCE, (label, worst)
    return worst


def two_roots(app, document, typed):
    """`document`'s layer 0 painted, plus a default (Pass Through) group
    "Folder" holding a painted, masked raster "Inner": the two roots."""
    # Numeric rgba ink is a 16-bit argument; an 8-bit stroke takes a hex colour.
    first = {'rgba': [0.2, 0.4, 0.6, 1]} if typed else {'color': '#336699'}
    second = {'rgba': [0.8, 0.3, 0.1, 1]} if typed else {'color': '#cc4d1a'}
    app.call('brush_stroke', document_id=document, layer=0,
             points=[[4, 5], [30, 18], [36, 26]], size=9, **first)
    inner = app.call('new_layer', document_id=document, name='Inner')['layer']
    app.call('brush_stroke', document_id=document, layer=inner,
             points=[[6, 22], [20, 8], [34, 12]], size=7, **second)
    app.call('add_layer_mask', document_id=document, layer=inner, kind='reveal_all')
    app.call('group_layers', document_id=document, layers=[inner], name='Folder')
    rows = layers(app, document)
    folder_index = row_named(app, document, 'Folder')
    index = rows.index(folder_index)
    return [0, index], [rows[0]['name'], 'Inner', 'Folder']


def g19(app, folder):
    out = {}
    # (a) A 16-bit two-root clip, and (b) the same from 8 bits.
    for label, source, scale in (('16', new16(app), 65535.0), ('8', new8(app), 255.0)):
        roots, names = two_roots(app, source, label == '16')
        inner = layers(app, source).index(row_named(app, source, 'Inner'))
        grids = [layer_grid(app, source, 0, W, H), layer_grid(app, source, inner, W, H)]
        union = union_box([alpha_box(grid, W, H) for grid in grids])
        target, reply = layers_into_float(app, folder, source, roots, 'g19-' + label,
                                          union=union)
        assert reply['origin_depth'] == int(label) and reply['converted'], reply
        assert reply['profile'] == 'same' and not reply['narrowed'], reply
        landed = layers(app, target)
        assert [row['name'] for row in landed[1:]] == names, landed
        assert reply['layers'] == [1, 3] and reply['records'] == 3, reply
        assert float_report(reply) == EMPTY_FLOAT_REPORT, reply
        assert 'isolated_groups' not in reply, reply
        assert landed[2]['has_mask'] and landed[3]['kind'] == 'group', landed
        assert landed[3]['blend_mode'] == 'Pass Through', landed
        worst = decoded_records(app, folder, target, 'g19-' + label, grids, scale)
        out['layers_' + label] = {'reply': reply, 'worst': worst}

    # (c) Display P3 at 16 bits: Convert, and Don't Convert keeping the numbers.
    p3 = painted16(app, profile='display_p3')
    grid = [layer_grid(app, p3, 0, W, H)]
    target, reply = layers_into_float(app, folder, p3, [0], 'g19-p3',
                                      union=alpha_box(grid[0], W, H))
    assert reply['profile'] == 'converted' and reply['profile_converted'], reply
    worst = decoded_records(app, folder, target, 'g19-p3', grid, 65535.0, P3_TO_SRGB)
    out['p3_convert'] = {'reply': reply, 'worst': worst}
    target, reply = layers_into_float(app, folder, p3, [0], 'g19-p3-keep', profile='preserve')
    assert reply['profile'] == 'preserved' and not reply['profile_converted'], reply
    worst = decoded_records(app, folder, target, 'g19-p3-keep', grid, 65535.0)
    out['p3_preserve'] = {'reply': reply, 'worst': worst}

    # (d) A text layer lands as text; a placed image lands as its pixels.
    source = new16(app)
    text = app.call('add_text_layer', document_id=source, text='Title', x=5, y=6, size=20)
    png = folder / 'g19-placed.png'
    write_png8(png, 16, 12, byte_pattern(16, 12, seed=9))
    placed = app.call('place_image', document_id=source, path=str(png), name='Placed',
                      x=4, y=4)
    assert 'text' in layers(app, source)[text['layer']]
    target, reply = layers_into_float(app, folder, source, [text['layer'], placed['layer']],
                                      'g19-held')
    assert float_report(reply) == EMPTY_FLOAT_REPORT, reply
    assert 'text' in row_named(app, target, 'Title'), row_named(app, target, 'Title')
    out['held'] = reply

    # (e) A layer style and Blend If land, an Outer Glow in Screen landing
    # Linear Dodge …
    styled = painted16(app)
    app.call('set_layer_properties', document_id=styled, index=0, name='Shadowed')
    app.call('set_layer_style', document_id=styled, layer=0,
             style={'effects': [{'type': 'drop_shadow', 'distance': 4, 'size': 3},
                                {'type': 'outer_glow', 'size': 2}]})
    target, reply = layers_into_float(app, folder, styled, [0], 'g19-style')
    assert float_report(reply) == dict(EMPTY_FLOAT_REPORT, effect_blend_reset=[
        {'layer': 'Shadowed', 'effect': 'outer_glow', 'from': 'screen',
         'to': 'linear_dodge'}]), reply
    landed = row_named(app, target, 'Shadowed')
    kinds = {e['type']: e for e in (landed.get('style') or {}).get('effects') or []}
    assert set(kinds) == {'drop_shadow', 'outer_glow'}, landed
    assert kinds['outer_glow']['blend'] == 'linear_dodge', landed
    assert kinds['drop_shadow']['blend'] == 'multiply', landed
    out['style'] = reply
    ranged = painted16(app)
    app.call('set_layer_properties', document_id=ranged, index=0, name='Ranged')
    app.call('set_layer_style', document_id=ranged, layer=0, style={
        'fill_opacity': 1, 'effects': [],
        'blend_if': {'channel': 'gray', 'this_layer': [0, 0, 200, 200],
                     'underlying': [0, 0, 255, 255]}})
    target, reply = layers_into_float(app, folder, ranged, [0], 'g19-blend-if')
    assert float_report(reply) == EMPTY_FLOAT_REPORT, reply
    landed = row_named(app, target, 'Ranged')
    assert (landed.get('style') or {}).get('blend_if', {}).get('this_layer') == [0, 0, 200, 200], \
        landed
    out['blend_if'] = reply
    # … and what the float mode cannot hold refuses by layer and feature …
    refusals = {}
    overlaid_glow = painted16(app)
    app.call('set_layer_properties', document_id=overlaid_glow, index=0, name='Glowing')
    app.call('set_layer_style', document_id=overlaid_glow, layer=0,
             style={'effects': [{'type': 'outer_glow', 'blend': 'overlay', 'size': 2}]})
    refusals['effect_mode'] = float_refusal(
        app, folder, overlaid_glow, [0], 'g19-effect-mode', 'Glowing',
        'the overlay blend mode on the outer_glow layer effect')
    adjusted = painted16(app)
    # Invert: the Vibrance this gate named before is admitted.
    inverted = app.call('add_adjustment_layer', document_id=adjusted, op='invert',
                        params={}, name='Invert')
    refusals['invert'] = float_refusal(app, folder, adjusted, [0, inverted['layer']],
                                       'g19-invert', 'Invert', 'the invert adjustment')
    out['refusals'] = refusals
    # … a default group holding a blended layer lands Pass Through …
    grouped = painted16(app)
    top = app.call('new_layer', document_id=grouped, name='Multiplied')['layer']
    app.call('brush_stroke', document_id=grouped, layer=top,
             points=[[6, 22], [34, 12]], size=7, rgba=[0.8, 0.3, 0.1, 1])
    app.call('set_layer_properties', document_id=grouped, index=top, blend_mode='Multiply')
    app.call('group_layers', document_id=grouped, layers=[0, top], name='Blended')
    index = layers(app, grouped).index(row_named(app, grouped, 'Blended'))
    target, reply = layers_into_float(app, folder, grouped, [index], 'g19-pass-through')
    assert float_report(reply) == EMPTY_FLOAT_REPORT, reply
    landed = row_named(app, target, 'Blended')
    assert landed['kind'] == 'group' and landed['blend_mode'] == 'Pass Through', landed
    assert row_named(app, target, 'Multiplied')['blend_mode'] == 'Multiply', \
        row_named(app, target, 'Multiplied')
    out['pass_through'] = reply
    # … an Overlay layer lands Normal (`mode_32`), and a calibrated Exposure
    # carries.
    overlaid = painted16(app)
    app.call('set_layer_properties', document_id=overlaid, index=0, name='Overlaid',
             blend_mode='Overlay')
    exposure = {'exposure': 0.75, 'evaluation': 'document_v2'}
    added = app.call('add_adjustment_layer', document_id=overlaid, op='exposure',
                     params=exposure, name='Exposure')
    target, reply = layers_into_float(app, folder, overlaid, [0, added['layer']], 'g19-reset')
    assert float_report(reply) == dict(EMPTY_FLOAT_REPORT, blend_reset=[
        {'layer': 'Overlaid', 'from': 'overlay'}]), reply
    assert row_named(app, target, 'Overlaid')['blend_mode'] == 'Normal', \
        row_named(app, target, 'Overlaid')
    restored = adjustment_rows(app, target, folder, 'g19-reset-rows')
    assert len(restored) == 1 and restored[0][0] == 'exposure', restored
    assert all(restored[0][1][key] == exposure[key] for key in exposure), restored
    out['reset'] = reply
    # … and a Levels copy lands with an empty float report.
    leveled = painted16(app)
    levels_params = {'black': 0.2, 'white': 0.8, 'gamma': 1.5}
    levels = app.call('add_adjustment_layer', document_id=leveled, op='levels',
                      params=levels_params, name='Levels')
    target, reply = layers_into_float(app, folder, leveled, [0, levels['layer']], 'g19-levels')
    assert float_report(reply) == EMPTY_FLOAT_REPORT, reply
    restored = adjustment_rows(app, target, folder, 'g19-levels-rows')
    assert len(restored) == 1 and restored[0][0] == 'levels', restored
    assert all(restored[0][1][key] == levels_params[key] for key in levels_params), restored
    out['levels'] = reply

    # (h) A 16-bit layers paste answers the four arrays empty.
    source = painted16(app)
    app.call('copy', document_id=source, mode='layers', layers=[0])
    target = new16(app)
    reply = app.call('paste', document_id=target, mode='layers')
    assert reply['depth'] == 16 and float_report(reply) == EMPTY_FLOAT_REPORT, reply
    out['sixteen'] = float_report(reply)
    return out


# ----------------------------------------------- layers paste helper --


def pasted_layers(app, source, roots, target, **arguments):
    """Copies `roots` of `source` as layers and pastes them in place into
    `target`: the reply, one history step."""
    copied = app.call('copy', document_id=source, mode='layers', layers=roots)
    assert copied['mode'] == 'layers', copied
    history = cursor(app, target)
    reply = app.call('paste', document_id=target, mode='layers', placement='in_place',
                     **arguments)
    assert reply['mode'] == 'layers' and reply['offset'] == {'x': 0, 'y': 0}, reply
    assert cursor(app, target) == history + 1, 'a layers paste is one history step'
    return reply


# ---------------------------------------------------------- g21 gate --
#
# Branches measured in Photoshop 26.10.0 (paste-layers-470c2/take3; G and H
# re-driven in take4 over a sentinel clipboard with the composite channel
# targeted): G and H drop (Photoshop's Copy with a group targeted replaces the
# clipboard with opaque white over the selection, whatever the group's
# properties, not its content; nothing is built, so today's raw projection is
# pinned), M none, C none, L none, P refuse, N build_content_box over a white
# Background.

MASKED_PASTE_REFUSAL = 'is unavailable while the clipboard holds copied layers'


def grouped(app, document, layer, name='Folder'):
    """Groups `layer` as `name`; answers (group index, child index)."""
    member = layers(app, document)[layer]['name']
    app.call('group_layers', document_id=document, layers=[layer], name=name)
    rows = layers(app, document)
    return (rows.index(row_named(app, document, name)),
            rows.index(row_named(app, document, member)))


def group_source(app, folder, depth, label):
    """An opaque `depth`-bit document (8: the byte pattern; 16: white with a
    stroke; 32: the float fixture) whose one layer is grouped as "Folder" at
    opacity 0.5 with a mask revealing the top half. Answers (document, group
    index, child index)."""
    if depth == 8:
        document = open_png(app, folder, label, W, H, seed=21)[0]
        width, height = W, H
    elif depth == 16:
        document = app.call('new_document', width=W, height=H, depth=16, profile='srgb',
                            background='white')['document']['id']
        app.call('brush_stroke', document_id=document, layer=0,
                 points=[[4, 5], [30, 18], [36, 26]], size=9, rgba=[0.2, 0.4, 0.6, 1])
        width, height = W, H
    else:
        document = fresh_float(app, folder, label)
        width, height = FW, FH
    group, child = grouped(app, document, 0)
    app.call('set_layer_properties', document_id=document, index=group, opacity=0.5)
    app.call('select_rect', document_id=document, x=0, y=0, width=width, height=height // 2)
    app.call('add_layer_mask', document_id=document, layer=group, kind='from_selection')
    app.call('deselect', document_id=document)
    return document, group, child


def group_copy_lands(app, folder, depth, label, hidden=False):
    """(a) Select All, copy with the group targeted, paste in place into a
    fresh document: today's raw projection — the child's own samples."""
    document, group, child = group_source(app, folder, depth, label)
    if hidden:
        app.call('set_layer_properties', document_id=document, index=group, visible=False)
    app.call('select_all', document_id=document)
    copied = app.call('copy', document_id=document, layer=group)
    app.call('deselect', document_id=document)
    if depth == 32:
        target = fresh_float(app, folder, label + '-target')
        reply = app.call('paste', document_id=target, mode='pixels', placement='in_place')
        records = float_records(app, target, folder, label + '-target', FW, FH)
        assert len(records) == 2, (label, len(records))
        assert records[1] == records[0], (label, 'the pasted group copy is not the raw child')
        return {'copy': copied, 'paste': reply}
    target = new16(app) if depth == 16 else new8(app)
    reply = app.call('paste', document_id=target, mode='pixels', placement='in_place')
    got = layer_grid(app, target, reply['layer'], W, H)
    want = layer_grid(app, document, child, W, H)
    assert got == want, (label, 'the pasted group copy is not the raw child')
    top = 65535 if depth == 16 else 255
    assert all(pixel[3] == top for pixel in got), (label, 'raw copy alpha is not opaque')
    return {'copy': copied, 'paste': reply}


def g21_copy_merged(app, folder):
    """(b) Copy Merged with the group targeted is the whole composite."""
    document = open_png(app, folder, 'g21-merged', W, H, seed=22)[0]
    top = app.call('new_layer', document_id=document, name='Child')['layer']
    app.call('brush_stroke', document_id=document, layer=top,
             points=[[4, 5], [30, 18], [36, 26]], size=9, color='#cc4d1a')
    group, _ = grouped(app, document, top)
    app.call('set_active_layer', document_id=document, index=group)
    app.call('select_all', document_id=document)
    app.call('copy_merged', document_id=document)
    app.call('deselect', document_id=document)
    target = new8(app)
    reply = app.call('paste', document_id=target, mode='pixels', placement='in_place')
    got = layer_grid(app, target, reply['layer'], W, H)
    width, height, depth, samples = render(app, document)
    assert (width, height, depth) == (W, H, 8), (width, height, depth)
    want = [list(samples[i:i + 4]) for i in range(0, len(samples), 4)]
    assert got == want, 'Copy Merged with a group targeted is not the composite'
    return reply


def g21_cuts(app, folder):
    """(c) Cut on a group with a selection and (d) Cut with no selection are
    refused; no history step either way."""
    out = {}
    for depth in (8, 16):
        document, group, child = group_source(app, folder, depth, f'g21-cut-{depth}')
        app.call('select_rect', document_id=document, x=0, y=0, width=W, height=H // 2)
        history = cursor(app, document)
        out[f'group_{depth}'] = refusal(app, 'cut', document_id=document, layer=group)
        assert cursor(app, document) == history, (depth, 'a refused cut added history')
        app.call('deselect', document_id=document)
        history = cursor(app, document)
        out[f'none_{depth}'] = refusal(app, 'cut', document_id=document, layer=child)
        assert cursor(app, document) == history, (depth, 'a refused cut added history')
    return out


def g21_masked_paste(app):
    """(e) Paste Into / Outside with copied layers are refused."""
    out = {}
    for depth in (8, 16):
        source = new16(app) if depth == 16 else new8(app)
        ink = {'rgba': [0.2, 0.4, 0.6, 1]} if depth == 16 else {'color': '#336699'}
        app.call('brush_stroke', document_id=source, layer=0, points=[[4, 5], [30, 18]],
                 size=7, **ink)
        app.call('copy', document_id=source, mode='layers', layers=[0])
        target = new16(app) if depth == 16 else new8(app)
        app.call('select_rect', document_id=target, x=10, y=8, width=20, height=12)
        history = cursor(app, target)
        for placement in ('into', 'outside'):
            message = refusal(app, 'paste', document_id=target, placement=placement)
            command = 'Paste Outside' if placement == 'outside' else 'Paste Into'
            assert command in message and MASKED_PASTE_REFUSAL in message, message
            out[f'{placement}_{depth}'] = message
        assert cursor(app, target) == history, (depth, 'a refused paste added history')
        assert len(layers(app, target)) == 1, layers(app, target)
    return out


def g21_new_from_clipboard(app):
    """(f) New from Clipboard with copied layers: the content box at the
    source's depth, profile and PPI, the layers by name over a white
    Background."""
    source = app.call('new_document', width=W, height=H, depth=16, profile='display_p3',
                      ppi=300, background='transparent')['document']['id']
    one = app.call('new_layer', document_id=source, name='One')['layer']
    app.call('brush_stroke', document_id=source, layer=one, points=[[8, 8], [12, 10]], size=5,
             rgba=[0.8, 0.15, 0.35, 1])
    two = app.call('new_layer', document_id=source, name='Two')['layer']
    app.call('brush_stroke', document_id=source, layer=two, points=[[24, 18], [30, 22]],
             size=5, rgba=[0.05, 0.5, 0.85, 1])
    grids = {k: layer_grid(app, source, k, W, H) for k in (one, two)}
    boxes = [alpha_box(grids[k], W, H) for k in (one, two)]
    x0 = min(b[0] for b in boxes)
    y0 = min(b[1] for b in boxes)
    x1 = max(b[0] + b[2] for b in boxes)
    y1 = max(b[1] + b[3] for b in boxes)
    app.call('copy', document_id=source, mode='layers', layers=[one, two])
    made = app.call('new_from_clipboard')
    assert made['layers'] == 2 and made['depth'] == 16 and made['origin_depth'] == 16, made
    assert made['typed'] and made['trimmed'], made
    assert [made['width'], made['height']] == [x1 - x0, y1 - y0], (made, (x0, y0, x1, y1))
    assert made['source_offset'] == [x0, y0], made
    assert list(made['ppi']) == [300, 300], made
    assert 'P3' in made['profile_description'], made
    document = made['document']['id']
    rows = layers(app, document)
    assert [row['name'] for row in rows] == ['Background', 'One', 'Two'], rows
    width, height = x1 - x0, y1 - y0
    background = layer_grid(app, document, 0, width, height)
    assert all(pixel == [65535, 65535, 65535, 65535] for pixel in background), 'base not white'
    for k, index in ((one, 1), (two, 2)):
        got = layer_grid(app, document, index, width, height)
        want = windowed(grids[k], W, (x0, y0, width, height))
        assert got == want, (k, 'a landed layer differs from its source')
    return made


def g21(app, folder):
    out = {'copy': {}}
    for depth in (8, 16, 32):
        out['copy'][depth] = group_copy_lands(app, folder, depth, f'g21-copy-{depth}')
    out['copy']['hidden'] = group_copy_lands(app, folder, 8, 'g21-hidden', hidden=True)
    out['copy_merged'] = g21_copy_merged(app, folder)
    out['cut'] = g21_cuts(app, folder)
    out['masked_paste'] = g21_masked_paste(app)
    out['new_from_clipboard'] = g21_new_from_clipboard(app)
    return out


# ---------------------------------------------------------- g22 gate --
#
# Branches measured in Photoshop 26.10.0 (paste-layers-470c3/take1): comps
# phase2 (Photoshop hides a pasted layer a comp does not know when the comp is
# applied; that hide is built, so it is pinned), channels, guides,
# samplers and global light all none — the copied layers bring no
# comp, channel, guide or sampler, and a pasted style keeps use_global_light and
# casts by the destination's light.

LIT_SHADOW = {'effects': [{'type': 'drop_shadow', 'use_global_light': True, 'distance': 4,
                           'size': 0, 'spread': 0, 'opacity': 1, 'color': '#000000',
                           'blend': 'normal'}]}
LOCAL_SHADOW = {'effects': [{'type': 'drop_shadow', 'use_global_light': False, 'angle': 90,
                             'distance': 4, 'size': 0, 'spread': 0, 'opacity': 1,
                             'color': '#000000', 'blend': 'normal'}]}


def blank(app, depth, width=W, height=H):
    return new16(app, width=width, height=height) if depth == 16 \
        else new8(app, width=width, height=height)


def stroked_layer(app, document, depth, name, points):
    """A new layer `name` holding one stroke; answers its index."""
    layer = app.call('new_layer', document_id=document, name=name)['layer']
    ink = {'rgba': [0.9, 0.9, 0.9, 1]} if depth == 16 else {'color': '#e6e6e6'}
    app.call('brush_stroke', document_id=document, layer=layer, points=points, size=5, **ink)
    return layer


def chrome_lists(app, document):
    """The chrome a layers paste must leave alone: comps (each comp's derived
    `unrecorded` list aside, which names the pasted layers), channels, guides
    and the sampler positions (their readings follow the pixels)."""
    samplers = app.call('get_color_samplers', document_id=document)['samplers']
    comps = [{key: value for key, value in comp.items() if key != 'unrecorded'}
             for comp in app.call('list_layer_comps', document_id=document)['comps']]
    return {'comps': comps,
            'channels': app.call('list_channels', document_id=document)['channels'],
            'guides': app.call('list_guides', document_id=document)['guides'],
            'samplers': [(s['x'], s['y'], s['sample_size']) for s in samplers]}


def g22_chrome(app, depth):
    """(a) Comps, channels, guides and samplers: the copied layers bring none
    and the destination keeps its own; each destination comp lists the pasted
    layers as `unrecorded`, and applying one hides them and names them in
    `hidden_unrecorded`."""
    source = blank(app, depth)
    a = stroked_layer(app, source, depth, 'PT_a', [[4, 4], [30, 6]])
    b = stroked_layer(app, source, depth, 'PT_b', [[4, 18], [30, 20]])
    app.call('add_channel', document_id=source, name='Alpha PT')
    app.call('add_guide', document_id=source, orientation='horizontal', position=4)
    app.call('add_guide', document_id=source, orientation='vertical', position=20)
    app.call('set_color_samplers', document_id=source,
             samplers=[{'x': 5, 'y': 5}, {'x': 25, 'y': 12}])
    app.call('set_layer_properties', document_id=source, index=a, visible=False)
    app.call('save_layer_comp', document_id=source, name='One')
    app.call('set_layer_properties', document_id=source, index=a, visible=True)
    app.call('set_layer_properties', document_id=source, index=b, visible=False)
    app.call('save_layer_comp', document_id=source, name='Two')
    app.call('set_layer_properties', document_id=source, index=b, visible=True)
    target = blank(app, depth)
    app.call('add_guide', document_id=target, orientation='vertical', position=35)
    app.call('save_layer_comp', document_id=target, name='One')
    app.call('save_layer_comp', document_id=target, name='Dest')
    before = chrome_lists(app, target)
    reply = pasted_layers(app, source, [a, b], target)
    landed = reply['layers']
    assert len(landed) == 2, reply
    assert chrome_lists(app, target) == before, (depth, 'the paste changed the chrome')
    for comp in app.call('list_layer_comps', document_id=target)['comps']:
        assert [row['layer'] for row in comp['unrecorded']] == sorted(landed), (depth, comp)
    names = [layers(app, target)[k]['name'] for k in sorted(landed)]
    for comp in ('Dest', 'One'):
        applied = app.call('apply_layer_comp', document_id=target, name=comp)
        assert applied['hidden_unrecorded'] == names, (depth, comp, applied)
        rows = layers(app, target)
        assert not any(rows[k]['visible'] for k in landed), (depth, comp, rows)
    out = {'paste': reply, 'chrome': before}
    if depth == 8:
        # Centred into a larger canvas: still nothing of the source's.
        wide = blank(app, 8, 2 * W, 2 * H)
        empty = chrome_lists(app, wide)
        app.call('copy', document_id=source, mode='layers', layers=[a, b])
        centred = app.call('paste', document_id=wide, mode='layers', placement='centered')
        assert centred['offset'] != {'x': 0, 'y': 0}, centred
        assert chrome_lists(app, wide) == empty, 'a centred paste brought chrome'
        out['centred'] = centred
    return out


def g22_light(app, depth):
    """(b) A pasted style keeps use_global_light and the destination keeps
    its own light, so the shadow casts by the destination's."""
    source = blank(app, depth)
    app.call('set_global_light', document_id=source, angle=30, altitude=45)
    lit = stroked_layer(app, source, depth, 'PG_lit', [[8, 8], [14, 10]])
    local = stroked_layer(app, source, depth, 'PG_local', [[26, 8], [32, 10]])
    app.call('set_layer_style', document_id=source, layer=lit, style=LIT_SHADOW)
    app.call('set_layer_style', document_id=source, layer=local, style=LOCAL_SHADOW)
    target = blank(app, depth)
    app.call('set_global_light', document_id=target, angle=150, altitude=20)
    reply = pasted_layers(app, source, [lit, local], target)
    rows, own = layers(app, target), layers(app, source)
    for k, layer in zip(reply['layers'], (lit, local)):
        assert rows[k].get('style') == own[layer].get('style'), (depth, rows[k], own[layer])
    assert rows[reply['layers'][0]]['style']['effects'][0]['use_global_light'] is True, rows
    light = app.call('get_document', document_id=target)['global_light']
    assert light == {'angle': 150, 'altitude': 20}, (depth, light)
    return {'paste': reply, 'global_light': light}


def g22(app, folder):
    out = {}
    for depth in (8, 16):
        out[f'chrome_{depth}'] = g22_chrome(app, depth)
        out[f'light_{depth}'] = g22_light(app, depth)
    return out


# ---------------------------------------------------------- g23 gate --

# Photoshop 26.10.0's measured landings (`scripts/photoshop-probe/
# paste_placement.py`, take1): with the whole canvas in view, a 16 x 16
# copy at (8, 8) of 64 x 48 lands centred — (24, 16) in 64 x 48, (92, 67)
# in 200 x 150 — and pasted in place across sizes at these corners.
PS_IN_PLACE = {(200, 150): [42, 42], (100, 100): [17, 25], (70, 51): [9, 9],
               (71, 50): [9, 8], (74, 48): [10, 8], (40, 30): [2, 2], (64, 48): [8, 8]}
PATCH = (8, 8, 16, 16)
PS_SELECTION = (100, 60, 40, 40)
# The two layers `paste_placement.py` copies (PA_one, PA_two): union (4, 4, 46, 26).
PS_LAYER_RECTS = ((4, 4, 12, 12), (30, 20, 20, 10))
BIG = (2000, 1500)


def depth_doc(app, depth, width, height, background='white'):
    return app.call('new_document', width=width, height=height, depth=depth, profile='srgb',
                    background=background)['document']['id']


def copy_rect(app, document, rect):
    """Copies `rect` of layer 0 and deselects."""
    x, y, w, h = rect
    app.call('select_rect', document_id=document, x=x, y=y, width=w, height=h)
    copied = app.call('copy', document_id=document, layer=0)
    assert copied['offset'] == [x, y] and [copied['width'], copied['height']] == [w, h], copied
    app.call('deselect', document_id=document)
    return copied


def patch_layers(app, depth, width=64, height=48):
    """A transparent `width` x `height` document holding PA_one and PA_two
    (filled rects), both copied as layers: (document, the copy)."""
    document = depth_doc(app, depth, width, height, background='transparent')
    made = []
    for name, (x, y, w, h), colour in zip(('PA_one', 'PA_two'), PS_LAYER_RECTS,
                                          ('#c8285a', '#0a78dc')):
        layer = app.call('new_layer', document_id=document, name=name)['layer']
        app.call('select_rect', document_id=document, x=x, y=y, width=w, height=h)
        app.call('fill_selection', document_id=document, layer=layer, color=colour)
        app.call('deselect', document_id=document)
        made.append(layer)
    copied = app.call('copy', document_id=document, mode='layers', layers=made)
    assert copied['mode'] == 'layers', copied
    return document, copied


def g23_visibility(app, depth):
    """(a) V/F: however much of the copy is in view — all of it, 60 %, 40 %,
    10 %, or a 60 % x 60 % corner — Cmd-V centres it on the visible canvas;
    (b) R: rotated 45 degrees, a copy in the bounding box's corner and one at
    its centre both centre on the visible bounding box."""
    out = {}
    document = depth_doc(app, depth, *BIG)
    visible = set_visible(app, document, zoom=2, center={'x': 1100, 'y': 800})
    vx, vy, vw, vh = visible
    rw, rh = 200, 100
    assert vx >= rw and vy >= rh and vx + vw < BIG[0] and vy + vh < BIG[1], visible
    left, top = math.ceil(vx), math.ceil(vy)
    middle = round(vy + vh / 2 - rh / 2)
    frame = view_frame(visible)
    cases = {'whole': (left, middle), 'cut_60': (left - 80, middle),
             'cut_40': (left - 120, middle), 'cut_10': (left - 180, middle),
             'corner_60': (left - 80, top - 40)}
    for name, (x, y) in cases.items():
        rect = (x, y, rw, rh)
        copy_rect(app, document, rect)
        assert intersects(visible, rect) and (name == 'whole') == contains(visible, rect), name
        want = framed(frame, rw, rh)
        assert want != [x, y], (name, want)
        reply = app.call('paste', document_id=document)
        assert reply['placement'] == 'view' and reply['offset'] == want, (name, reply, want)
        out[name] = reply['offset']
    rotated = set_visible(app, document, rotation=45)
    frame = view_frame(rotated)
    corner = (math.ceil(rotated[0]) + 2, math.ceil(rotated[1]) + 2, 40, 40)
    inside = (round(rotated[0] + rotated[2] / 2) - 20, round(rotated[1] + rotated[3] / 2) - 20,
              40, 40)
    for name, rect in (('rotated_corner', corner), ('rotated_inside', inside)):
        copy_rect(app, document, rect)
        want = framed(frame, 40, 40)
        reply = app.call('paste', document_id=document)
        assert reply['placement'] == 'view' and reply['offset'] == want, (name, reply, want)
        out[name] = reply['offset']
    set_visible(app, document, rotation=0)
    return out


def g23_documents(app, depth):
    """(c) X: the same document, a same-size one and a larger one — each
    centred (Photoshop's (24, 16) and (92, 67) with the whole canvas in
    view), never in place."""
    out = {}
    source = depth_doc(app, depth, 64, 48)
    copy_rect(app, source, PATCH)
    for name, size, ps in (('same_doc', None, [24, 16]), ('same_size', (64, 48), [24, 16]),
                           ('larger', (200, 150), [92, 67])):
        target = source if size is None else depth_doc(app, depth, *size)
        visible = set_visible(app, target, fit=True)
        want = framed(view_frame(visible), 16, 16)
        reply = app.call('paste', document_id=target)
        assert reply['placement'] == 'view' and reply['offset'] == want, (name, reply, want)
        canvas = (64, 48) if size is None else size
        if view_frame(visible) == (0, 0) + canvas:
            assert want == ps, (name, want, ps)
        out[name] = reply['offset']
    return out


def consumed(app, document, history, rect):
    """The paste deselected in its one step, and Reselect restores `rect`."""
    assert selection_of(app, document) is None, 'the paste kept the selection'
    assert cursor(app, document) == history + 1, 'the paste is not one step'
    app.call('reselect', document_id=document)
    assert selection_of(app, document) == list(rect), selection_of(app, document)
    app.call('deselect', document_id=document)


def select(app, document, rect):
    x, y, w, h = rect
    app.call('select_rect', document_id=document, x=x, y=y, width=w, height=h)
    return cursor(app, document)


def g23_selection(app, depth):
    """(d) O/S and the deselect verdicts: Cmd-V of pixels centres on a
    destination selection and deselects (same document and another);
    Shift-Cmd-V of pixels ignores it (in place, or at the relative location)
    and deselects; Cmd-V and Shift-Cmd-V of copied layers keep it. First the
    everyday Cmd-C, Cmd-V (Photoshop's x_same_doc_selected): Copy leaves its
    marquee active, so the paste centres on it — the copy's own spot, (8, 8)
    where the view would give (24, 16) — and deselects."""
    out = {}
    own = depth_doc(app, depth, 64, 48)
    x, y, w, h = PATCH
    app.call('select_rect', document_id=own, x=x, y=y, width=w, height=h)
    copied = app.call('copy', document_id=own, layer=0)
    assert copied['offset'] == [x, y], copied
    assert selection_of(app, own) == list(PATCH), 'copy deselected'
    history = cursor(app, own)
    reply = app.call('paste', document_id=own)
    assert reply['placement'] == 'selection' and reply['offset'] == [x, y], reply
    consumed(app, own, history, PATCH)
    out['pixels_copied_selection'] = reply['offset']
    source = depth_doc(app, depth, 200, 150)
    copy_rect(app, source, PATCH)
    history = select(app, source, PS_SELECTION)
    reply = app.call('paste', document_id=source)
    assert reply['placement'] == 'selection' and reply['offset'] == [112, 72], reply
    consumed(app, source, history, PS_SELECTION)
    out['pixels_same_doc'] = reply['offset']
    small = depth_doc(app, depth, 64, 48)
    copy_rect(app, small, PATCH)
    target = depth_doc(app, depth, 200, 150)
    history = select(app, target, PS_SELECTION)
    reply = app.call('paste', document_id=target)
    assert reply['placement'] == 'selection' and reply['offset'] == [112, 72], reply
    consumed(app, target, history, PS_SELECTION)
    out['pixels_other_doc'] = reply['offset']
    own = (40, 20, 16, 16)
    history = select(app, small, own)
    reply = app.call('paste', document_id=small, placement='in_place')
    assert reply['placement'] == 'in_place' and reply['offset'] == [8, 8], reply
    consumed(app, small, history, own)
    out['in_place_same_doc'] = reply['offset']
    history = select(app, target, PS_SELECTION)
    reply = app.call('paste', document_id=target, placement='in_place')
    assert reply['placement'] == 'relative_in_place' and reply['offset'] == [42, 42], reply
    consumed(app, target, history, PS_SELECTION)
    out['in_place_other_size'] = reply['offset']
    patch_layers(app, depth)
    for placement, want in (('auto', ('selection', {'x': 93, 'y': 63})),
                            ('in_place', ('relative_in_place', {'x': 57, 'y': 36}))):
        layered = depth_doc(app, depth, 200, 150)
        history = select(app, layered, PS_SELECTION)
        reply = app.call('paste', document_id=layered, placement=placement)
        assert reply['mode'] == 'layers', reply
        assert (reply['placement'], reply['offset']) == want, (placement, reply)
        assert selection_of(app, layered) == list(PS_SELECTION), 'a layers paste deselected'
        assert cursor(app, layered) == history + 1, 'the layers paste is not one step'
        out['layers_' + placement] = reply['offset']
    return out


def g23_in_place(app, folder, depth):
    """(e) P: Paste in Place keeps the copy's centre at the same fraction of
    another canvas (Photoshop's corners), its coordinates on the source's
    size; get_clipboard names the source canvas; a payload file has no
    source canvas and keeps its coordinates; a recorded relative paste
    records at."""
    out = {}
    source = depth_doc(app, depth, 64, 48)
    copy_rect(app, source, PATCH)
    clip = app.call('get_clipboard')
    assert clip['source'] == {'width': 64, 'height': 48}, clip
    for size, want in PS_IN_PLACE.items():
        target = depth_doc(app, depth, *size)
        reply = app.call('paste', document_id=target, placement='in_place')
        landed = 'in_place' if size == (64, 48) else 'relative_in_place'
        assert reply['placement'] == landed and reply['offset'] == want, (size, reply, want)
        assert want == relative_at(PATCH, (64, 48), size), (size, want)
        out['x'.join(map(str, size))] = reply['offset']
    copy_rect(app, source, (7, 9, 15, 13))
    target = depth_doc(app, depth, 200, 150)
    reply = app.call('paste', document_id=target, placement='in_place')
    assert reply['placement'] == 'relative_in_place' and reply['offset'] == [37, 41], reply
    out['odd'] = reply['offset']
    copy_rect(app, source, PATCH)
    target = depth_doc(app, depth, 200, 150)
    reply, arguments, _ = recorded_paste(app, target, placement='in_place', record_payload=True)
    assert reply['placement'] == 'relative_in_place' and reply['offset'] == [42, 42], reply
    assert arguments['placement'] == 'at' and [arguments['x'], arguments['y']] == [42, 42], \
        arguments
    out['recorded'] = arguments['placement']
    # A payload recorded on the source's own size (it lands at (8, 8)), then
    # pasted in place from its file into 200 x 150: no source canvas, so its
    # own coordinates.
    same = depth_doc(app, depth, 64, 48)
    _, arguments, _ = recorded_paste(app, same, placement='in_place', record_payload=True)
    target = depth_doc(app, depth, 200, 150)
    kept = app.call('paste', document_id=target, placement='in_place',
                    payload_path=arguments['payload_path'])
    assert kept['placement'] == 'in_place' and kept['offset'] == [8, 8], kept
    return out


def g23_layers(app, depth):
    """(f) L/PL: copied layers pasted with Cmd-V are centred on the view —
    Photoshop's (9, 11) with the whole canvas in view — in their own
    document too; Shift-Cmd-V keeps them on a same-size canvas and moves
    them to the relative location on another."""
    out = {}
    union = union_box(PS_LAYER_RECTS)
    source, _ = patch_layers(app, depth)
    for name, size in (('same_doc', None), ('same_size', (64, 48))):
        target = source if size is None else depth_doc(app, depth, *size, 'transparent')
        visible = set_visible(app, target, fit=True)
        at = framed(view_frame(visible), union[2], union[3])
        reply = app.call('paste', document_id=target)
        assert reply['mode'] == 'layers' and reply['placement'] == 'view', (name, reply)
        assert reply['offset'] == {'x': at[0] - union[0], 'y': at[1] - union[1]}, \
            (name, reply, at)
        if view_frame(visible) == (0, 0, 64, 48):
            assert at == [9, 11], (name, at)
        out[name] = reply['offset']
    for size, want in (((200, 150), ('relative_in_place', [61, 40])),
                       ((64, 48), ('in_place', [4, 4]))):
        target = depth_doc(app, depth, *size, 'transparent')
        reply = app.call('paste', document_id=target, placement='in_place')
        assert reply['placement'] == want[0], (size, reply)
        assert reply['offset'] == {'x': want[1][0] - union[0], 'y': want[1][1] - union[1]}, \
            (size, reply)
        out['in_place_' + 'x'.join(map(str, size))] = reply['offset']
    return out


def g23_views(app, depth):
    """(g) `view` picks the view whose visible canvas a Cmd-V centres on;
    a view the document does not have is refused naming list_views."""
    document = depth_doc(app, depth, *BIG)
    copy_rect(app, document, (100, 100, 40, 30))
    first = set_visible(app, document, zoom=2, center={'x': 600, 'y': 400})
    app.call('open_second_view', document_id=document, zoom=2,
             center={'x': 1500, 'y': 1100})
    second = settled_view(app, document, 2)
    assert view_frame(first) != view_frame(second), (first, second)
    reply = app.call('paste', document_id=document, view=2)
    assert reply['offset'] == framed(view_frame(second), 40, 30), (reply, second)
    reply = app.call('paste', document_id=document)
    assert reply['offset'] == framed(view_frame(first), 40, 30), (reply, first)
    message = refusal(app, 'paste', document_id=document, view=9)
    assert 'list_views' in message, message
    return {'view_2': framed(view_frame(second), 40, 30), 'refusal': message}


GEOMETRY_REFUSAL = ('"paste" has no canvas coordinates unless its placement is at — a live '
                    'paste lands by the replaying document\'s own placement rule')


def g23_geometry(app, depth):
    """(h) A saved-payload paste at (24, 16) of a 16 x 16 payload, recorded
    on 64 x 48, replays on 128 x 96 at (56, 40) under relative geometry (its
    centre keeps its fraction) and at (88, 64) under the bottom-right anchor
    (its distance from the corner kept); a live paste with a mode is refused."""
    source = depth_doc(app, depth, 64, 48)
    copy_rect(app, source, (24, 16, 16, 16))
    target = depth_doc(app, depth, 64, 48)
    _, arguments, _ = recorded_paste(app, target, placement='at', x=24, y=16,
                                     record_payload=True)
    assert arguments['placement'] == 'at' and [arguments['x'], arguments['y']] == [24, 16], \
        arguments
    out = {}
    for label, geometry, want in (('relative', {'mode': 'relative'}, [56, 40]),
                                  ('anchor', {'mode': 'anchor', 'anchor': 'bottom-right'},
                                   [88, 64])):
        name = f'g23 {label} {depth}'
        app.call('save_action', name=name, overwrite=True,
                 steps=[{'tool': 'paste', 'arguments': arguments, 'geometry': geometry}],
                 recorded_canvas={'width': 64, 'height': 48})
        fresh = depth_doc(app, depth, 128, 96)
        report = app.call('run_action', name=name, document_id=fresh)
        assert report['ok'], report
        rows = layers(app, fresh)
        assert len(rows) == 2 and offset(rows[1]) == want, (label, rows, want)
        out[label] = offset(rows[1])
    reply = app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
        'name': f'g23 live {depth}', 'overwrite': True,
        'steps': [{'tool': 'paste', 'arguments': dict(LIVE_PASTE),
                   'geometry': {'mode': 'relative'}}],
        'recorded_canvas': {'width': 64, 'height': 48}}})
    assert reply.get('isError'), reply
    message = ' '.join(row.get('text', '') for row in reply.get('content', []))
    assert GEOMETRY_REFUSAL in message, message
    out['live_refusal'] = message
    return out


def g23(app, folder):
    out = {}
    for depth in (8, 16):
        out[f'visibility_{depth}'] = g23_visibility(app, depth)
        out[f'documents_{depth}'] = g23_documents(app, depth)
        out[f'selection_{depth}'] = g23_selection(app, depth)
        out[f'in_place_{depth}'] = g23_in_place(app, folder, depth)
        out[f'layers_{depth}'] = g23_layers(app, depth)
        out[f'views_{depth}'] = g23_views(app, depth)
        out[f'geometry_{depth}'] = g23_geometry(app, depth)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    parser.add_argument('--gates', default=None,
                        help='comma-separated gates to run (default: g1–g23, the gate)')
    args = parser.parse_args()
    folder = Path(args.receipts)
    folder.mkdir(parents=True, exist_ok=True)
    pasteboard = f'com.kgalligan.doctored.verify-12g.{os.getpid()}'
    os.environ['DR_PASTEBOARD_NAME'] = pasteboard
    receipts = {'pasteboard': pasteboard}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    host = HostApp(folder / 'owner', folder / 'recovery', str(executable))
    gates = [
        ('g1', lambda app: g1(app), 'typed copy/paste round trip at 16 bits'),
        ('g2', lambda app: g2(app, folder), 'depth and profile convert'),
        ('g3', lambda app: g3(app, folder), 'cut, locks, Quick Mask, float'),
        ('g4', lambda app: {'u16': g4_16(app), 'float': g4_float(app, folder)},
         'copy merged vs f64 oracle at 16 and float'),
        ('g5', lambda app: g5(app, folder), 'undo/redo and native resave'),
        ('g6', lambda app: g6(app, folder), 'public TIFF and 8-bit round trip'),
        ('g7', lambda app: g7(app, folder), '8-bit typed copy, placement and profile conversion'),
        ('g8', lambda app: g8(app, folder), 'promotion and conversion into 8, 16 and float'),
        ('g9', lambda app: g9(app, folder), 'new from clipboard at depth'),
        ('g10', lambda app: g10(app, folder), 'recorded paste replays from its $asset'),
        ('g11', lambda app: g11(app, folder), 'float cut, frozen alpha and PPI disclosure'),
        ('g12', lambda app: g12(app, folder), 'editable text, shape and adjustment records'),
        ('g13', lambda app: g13(app, folder), 'editable replay and the carried boundary fixes'),
        ('g14', lambda app: g14(app, folder), 'new from clipboard restores editable records'),
        ('g15', lambda app: g15(app, folder), 'new from clipboard trims to content'),
        ('g16', lambda app: g16(app, folder),
         'paste centres on the selection or the view, never in place'),
        ('g17', lambda app: g17(app, folder), 'promotion into float, Preserve, replay and layers'),
        ('g18', lambda app: g18(app, folder), 'recorded copy and paste replay live'),
        ('g19', lambda app: g19(app, folder), 'layers into float'),
        ('g21', lambda app: g21(app, folder),
         'group copy, cut and masked paste with copied layers'),
        ('g22', lambda app: g22(app, folder),
         'comps, channels, guides, samplers and global light with copied layers'),
        ('g23', lambda app: g23(app, folder), 'paste placement matches Photoshop'),
    ]
    known = [name for name, _, _ in gates]
    wanted = known if not args.gates else [g.strip() for g in args.gates.split(',') if g.strip()]
    unknown = [name for name in wanted if name not in known]
    assert not unknown, f'unknown gates: {unknown}'
    try:
        with host as app:
            for name, run, message in gates:
                if name not in wanted:
                    continue
                receipts[name] = run(app)
                print(f'PASS {name} {message}', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))


if __name__ == '__main__':
    main()
