#!/usr/bin/env python3
"""MCP drive for Photoshop Gradient Fill layers (`GdFl`) through the app.

The script owns its app instance: the Debug app (`phase5_support.App`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s. It terminates only that
process and never launches Photoshop. The files are the take's own
(`core/tests/fixtures/psd_photoshop/gradient_fill_layer`, take1 of
`photoshop-probe/fill_layer_672_take.py`); every expected number is the file's
own `GdFl` (read here from its bytes) or this file's Display P3 oracle (the
published primaries, both D65), never read back from the app.

Cases:
- OPEN (GL8, GR8, GA8, GF8, GL16, GC8, GM8): inspect_psd observes the `GdFl`;
  open_document psd_mode layers holds layer 1 as a gradient fill
  (`fill.kind` gradient) with the file's style, angle 30, scale 0.8, stop
  positions 0 / 0.5 / 1, opacities 1 / 1 / 0.4 and hexes (the file's `RGBC`
  rounded; GC8: each stop's `ink` within 0.01 % of the file's `CMYC`), and
  `align_with_layer` the file's `Algn` (absent = true; GM8 included).
- PSD (each file): export_psd layers, then inspect_psd and open_document
  psd_mode layers again: the same fill.
- REFIT (GL8): canvas_size grows the canvas 10 x 6 about its centre; the fill
  covers the new canvas (`fill.canvas` 74 x 54) and is still a gradient.
- EDIT (GL8): edit_fill_layer gradient angle 45 takes one step; undo puts the
  angle back to 30.
- WIDE: a 16-bit Display P3 document's add_fill_layer gradient (#FF0000 at 0,
  #00FF00 at 0.25 and 0.75, #0000FF at 1, angle 0), exported, its two green
  stops' `RGBC` patched to the document's (0, 255, 0) and reopened: the
  observation names wide colours and the centre pixel is P3 green within 1
  code; edit_fill_layer angle 180 keeps it (one step); the SAME stops (as
  get_document reports them; the stored hex is the sRGB clip #00ff00) add no
  step and keep it; a CHANGED stop list (the second green at 0.7) takes one
  step and drops the wide stops — the centre pixel is then the Classic ramp
  (per segment the uniform Catmull-Rom, written here) of the four stops'
  Display P3 numbers at t = 1/2 (the oracle; the ramp interpolates in the
  document's space), more than 50 codes from P3 green in red; undo restores
  P3 green.
- SMOOTH-ALIGN (GL8): edit_fill_layer gradient {smoothness 0.5,
  align_with_layer true} is one step and the report shows both; a save_copy
  to .rz writes words 105 then 103 after 45 and reopens with both; undo
  restores smoothness 1; a set_layer_style gradient_overlay with smoothness
  0.5 reads back from get_document.
- GM8-HOST: GM8's imported layer sampled on a grid inside its mask box
  (8, 6, 40, 30); edit_fill_layer {smoothness 0.5} then {smoothness 1}: the
  grid ends within 1 code of the imported render (the host's box is the
  import's).
- IN-APP-ALIGN (GL8, GM8's gradient): select_rect (8, 6, 40, 30),
  add_layer_mask from_selection, edit_fill_layer {align_with_layer true} (one
  step, changed true): the same grid within 1 code of GM8's imported layer.
- MIDPOINT (an 8-bit 256 x 8 sRGB document): add_fill_layer linear
  angle 0 black to white with stop 0's midpoint 0.25 is one step and the
  report shows it; red at x = 16, 48, ..., 240 is within 1 code of
  255 S(g(x / 256, 0.25)) (S the two-stop Classic ramp (f + 3f^2 - 2f^3) / 2,
  g the take-2 description's piecewise-linear remap, both written here);
  export_psd holds `Mdpn` 50 then 25 on the `Clrt` stops (Photoshop stop
  i + 1 carries our stop i's; 50 on both `TrnS`)
  and reopens with 0.25; save_copy to .rz writes words [45, 104] and reopens
  with it; set_layer_style gradient_overlay with midpoint 0.3 and
  opacity_midpoint 0.7 reads back; two undos drop the style and the layer;
  the `gradient` tool (0, 4) -> (256, 4) with the same stops on an 8-bit and a
  16-bit document is within 1 code of 255 S(g((x + 0.5) / 256, 0.25)).
- MIDPOINT-SEGMENTS (an 8-bit 256 x 8 sRGB document): add_fill_layer
  linear angle 0 smoothness 0 with three opaque stops, black at 0 (midpoint
  0.25, opacity_midpoint 0.3), white at 0.5 (0.75, 0.8) and black at 1, is
  one step and get_document shows 0.25 / 0.75 / none and 0.3 / 0.8 / none;
  red at x = 16, 48, ..., 240 and at 32 and 224 is within 1 code of 255
  lerp(g(f, m_i)) (t = x / 256, stop i's midpoint governing segment
  i..i + 1; x = 32 and 224 are 127.5); export_psd's `Mdpn` are [50, 25, 75] on `Clrt` then [50, 30, 80] on
  `TrnS` (Photoshop's segment i reads stop i + 1's `Mdpn`) and the
  reopened PSD shows the same midpoints; edit_fill_layer with
  only stop 1's midpoint 0.4 is one step and reads back 0.4; undo reads 0.75.
- TOOL-SMOOTHNESS (8-bit and 16-bit 256 x 8 white documents): the
  `gradient` tool (0, 4) -> (256, 4) linear with stops black at 0, white at
  0.3 and #404040 at 1 and `smoothness` 0.5 paints red at x = 16, 48, ...,
  240 within 1 code of 255 clamp((1 - k) lerp + k C) (t = (x + 0.5) / 256,
  k = 0.5, lerp the linear ramp and C the Classic ramp unclamped, both written
  here); without the key it paints what `smoothness` 1 paints, and that
  differs from 0 by more than 2 codes somewhere; a two-colour black -> white
  call at 0 is linear within 1 code; each paint is one step; `smoothness` 1.5
  answers in-band "smoothness must be between 0 and 1" and adds no step.
  start_recording, the 0.5 call, stop_recording name tool-smoothness: the
  step's `arguments.smoothness` is 0.5, and run_action on a fresh 8-bit
  document paints the same reds.

Usage: python3 scripts/verify_gradient_fill_psd.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = ROOT / 'core/tests/fixtures/psd_photoshop/gradient_fill_layer'
KEYS = ['GL8', 'GR8', 'GA8', 'GF8', 'GL16', 'GC8', 'GM8']
STYLES = {'Lnr ': 'linear', 'Rdl ': 'radial', 'Angl': 'angle', 'Rflc': 'reflected'}
GROW = (10, 6)
BOX = (8, 6, 40, 30)
GRID = [(x, y) for x in range(BOX[0] + 1, BOX[0] + BOX[2], 6)
        for y in range(BOX[1] + 1, BOX[1] + BOX[3], 5)]
sys.path.insert(0, str(HERE))
from verify_solid_fill_psd import near, srgb_in_p3  # noqa: E402


def gdfl_block(data):
    """The first `GdFl` block body and its offset in `data`."""
    at = data.index(b'8BIMGdFl')
    length = struct.unpack('>I', data[at + 8:at + 12])[0]
    return at + 12, data[at + 12:at + 12 + length]


def doubles(body, key):
    """Every `key doub` value of `body`, in order, with its offset."""
    out, at = [], 0
    while True:
        at = body.find(key + b'doub', at)
        if at < 0:
            return out
        out.append((at + 8, struct.unpack('>d', body[at + 8:at + 16])[0]))
        at += 16


def file_gradient(path):
    """The file's `GdFl` numbers: its type, and per colour stop the `RGBC`
    (0..255) or `CMYC` percent, read straight off the bytes."""
    _, body = gdfl_block(Path(path).read_bytes())
    at = body.index(b'GrdT')
    kind = body[at + 8:at + 12].decode('latin-1')
    if b'CMYC' in body:
        keys = [b'Cyn ', b'Mgnt', b'Ylw ', b'Blck']
    else:
        keys = [b'Rd  ', b'Grn ', b'Bl  ']
    columns = [[v for _, v in doubles(body, k)] for k in keys]
    return STYLES[kind], [list(stop) for stop in zip(*columns)], b'CMYC' in body


def file_align(path):
    """The file's `Algn` (absent = Photoshop's default, true)."""
    _, body = gdfl_block(Path(path).read_bytes())
    at = body.find(b'Algnbool')
    return True if at < 0 else body[at + 8] != 0


class Drive:
    def __init__(self, app, folder):
        self.app, self.folder = app, Path(folder)

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def open(self, path):
        report = self.call('inspect_psd', path=str(path))
        doc = self.call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
        return report, doc

    def fill(self, doc):
        return self.call('get_document', document_id=doc)['layers'][1].get('fill') or {}

    def cursor(self, doc):
        return self.call('list_history', document_id=doc)['cursor']

    def pixel16(self, doc, x, y):
        reply = self.call('sample_pixel', document_id=doc, source='layer', layer=1, x=x, y=y)
        return [v / 257.0 for v in reply['center_rgba_codes'][:3]]

    def grid(self, doc):
        """Layer 1's native codes on GRID (inside GM8's mask box)."""
        return [self.call('sample_pixel', document_id=doc, source='layer', layer=1, x=x,
                          y=y)['center_rgba_codes'][:3] for x, y in GRID]


def classic(stops, t):
    """The Classic ramp (Smoothness 100 %) of `stops` [(position,
    [r, g, b])] at `t`, clamped to 0..255: between stops k and k + 1 at the
    segment fraction f the cubic Hermite with tangents (v[k + 1] - v[k - 1]) / 2
    in segment-fraction units, the end stops repeated."""
    k = max(i for i in range(len(stops) - 1) if stops[i][0] <= t)
    (p0, a), (p1, b) = stops[k], stops[k + 1]
    before = stops[k - 1][1] if k > 0 else a
    after = stops[k + 2][1] if k + 2 < len(stops) else b
    f = (t - p0) / (p1 - p0)
    h00, h10 = 2 * f ** 3 - 3 * f ** 2 + 1, f ** 3 - 2 * f ** 2 + f
    h01, h11 = -2 * f ** 3 + 3 * f ** 2, f ** 3 - f ** 2
    return [min(255.0, max(0.0, h00 * a[c] + h10 * (b[c] - before[c]) / 2 + h01 * b[c]
                           + h11 * (after[c] - a[c]) / 2)) for c in range(3)]


def classic_unclamped(stops, t):
    """The Classic ramp of scalar `stops` [(position, value)] at `t`,
    NOT clamped (`classic` clamps per stop pair; Smoothness blends the
    spline's own value before anything clamps): the same cubic Hermite with
    tangents (v[k + 1] - v[k - 1]) / 2 in segment-fraction units, the end
    stops repeated."""
    k = max(i for i in range(len(stops) - 1) if stops[i][0] <= t)
    (p0, a), (p1, b) = stops[k], stops[k + 1]
    before = stops[k - 1][1] if k > 0 else a
    after = stops[k + 2][1] if k + 2 < len(stops) else b
    f = (t - p0) / (p1 - p0)
    h00, h10 = 2 * f ** 3 - 3 * f ** 2 + 1, f ** 3 - 2 * f ** 2 + f
    h01, h11 = -2 * f ** 3 + 3 * f ** 2, f ** 3 - f ** 2
    return h00 * a + h10 * (b - before) / 2 + h01 * b + h11 * (after - a) / 2


def linear_ramp(stops, t):
    """The linear ramp of scalar `stops` [(position, value)] at `t`."""
    k = max(i for i in range(len(stops) - 1) if stops[i][0] <= t)
    (p0, a), (p1, b) = stops[k], stops[k + 1]
    return a + (b - a) * (t - p0) / (p1 - p0)


def midpoint_g(f, m):
    """The take-2 description's midpoint remap: f / (2m) up to m,
    1/2 + (f - m) / (2 (1 - m)) past it."""
    return f / (2 * m) if f <= m else 0.5 + (f - m) / (2 * (1 - m))


def two_stop_s(f):
    """The two-stop Classic ramp: (f + 3f^2 - 2f^3) / 2."""
    return (f + 3 * f * f - 2 * f ** 3) / 2


def mdpn_values(body):
    """Every `Mdpn long` of a descriptor body, in order."""
    out, at = [], 0
    while True:
        at = body.find(b'Mdpnlong', at)
        if at < 0:
            return out
        out.append(struct.unpack('>i', body[at + 8:at + 12])[0])
        at += 12


def check_fill(fill, style, stops, cmyk, what, align=None):
    """OPEN's gradient (module doc)."""
    assert fill.get('kind') == 'gradient', (what, fill)
    g = fill['gradient']
    if align is not None:
        assert g.get('align_with_layer') is align, (what, 'align', g)
    assert g['style'] == style and abs(g['angle'] - 30) < 1e-3 and abs(g['scale'] - 0.8) < 1e-3, \
        (what, g)
    assert [round(s['position'], 4) for s in g['stops']] == [0, 0.5, 1], (what, g['stops'])
    assert [round(s['opacity'], 4) for s in g['stops']] == [1, 1, 0.4], (what, g['stops'])
    for got, want in zip(g['stops'], stops):
        if cmyk:
            assert near(got['ink'], want, 0.01), (what, got, want)
        else:
            hexed = '#' + ''.join(f'{round(v):02x}' for v in want)
            assert got['color'].lower() == hexed, (what, got, want)


def open_case(drive, key):
    path = FIXTURES / f'{key}.psd'
    style, stops, cmyk = file_gradient(path)
    align = file_align(path)
    report, doc = drive.open(path)
    observed = [o for o in report['observations'] if 'Gradient Fill layer (GdFl)' in o]
    assert observed, ('observation', key, report['observations'])
    assert not [loss for loss in report.get('verified_losses', []) if '(“GF”)' in loss], \
        ('no loss', key, report.get('verified_losses'))
    check_fill(drive.fill(doc), style, stops, cmyk, f'OPEN {key}', align)
    again = drive.folder / f'{key}-again.psd'
    drive.call('export_psd', document_id=doc, path=str(again), format='psd', mode='layers')
    _, back = drive.open(again)
    check_fill(drive.fill(back), style, stops, cmyk, f'PSD {key}', align)
    if key == 'GL8':
        reply = drive.call('canvas_size', document_id=doc, width=64 + GROW[0],
                           height=48 + GROW[1], anchor='center')
        assert reply.get('ok'), reply
        fill = drive.fill(doc)
        assert fill.get('kind') == 'gradient', ('REFIT', fill)
        assert fill['canvas'] == {'width': 74, 'height': 54}, ('REFIT', fill['canvas'])
        start = drive.cursor(doc)
        drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'angle': 45})
        assert drive.cursor(doc) == start + 1, 'EDIT: one step'
        assert abs(drive.fill(doc)['gradient']['angle'] - 45) < 1e-3, 'EDIT: angle 45'
        drive.call('undo', document_id=doc)
        assert abs(drive.fill(doc)['gradient']['angle'] - 30) < 1e-3, 'UNDO: angle 30'
    for d in (doc, back):
        drive.call('close_document', document_id=d, discard_changes=True)
    return dict(style=style, stops=stops, align=align)


def smooth_align_case(drive):
    _, doc = drive.open(FIXTURES / 'GL8.psd')
    start = drive.cursor(doc)
    reply = drive.call('edit_fill_layer', document_id=doc, layer=1,
                       gradient={'smoothness': 0.5, 'align_with_layer': True})
    assert drive.cursor(doc) == start + 1, ('one step', reply)
    g = reply['fill']['gradient']
    assert abs(g['smoothness'] - 0.5) < 1e-9 and g['align_with_layer'] is True, g
    saved = drive.folder / 'smooth-align.rz'
    drive.call('save_copy', document_id=doc, path=str(saved), format='rz')
    data = saved.read_bytes()
    words = [struct.unpack('<I', data[at:at + 4])[0] for at in (4, 8, 12)]
    assert words == [45, 105, 103], ('the .rz words', words)
    back = drive.call('open_document', path=str(saved))['document']['id']
    g = drive.fill(back)['gradient']
    assert abs(g['smoothness'] - 0.5) < 1e-9 and g['align_with_layer'] is True, ('.rz', g)
    drive.call('close_document', document_id=back, discard_changes=True)
    drive.call('undo', document_id=doc)
    g = drive.fill(doc)['gradient']
    assert g['smoothness'] == 1 and g['align_with_layer'] is True, ('undo', g)
    stops = [dict(position=0, color='#203040', opacity=1),
             dict(position=1, color='#e0c0a0', opacity=1)]
    drive.call('set_layer_style', document_id=doc, layer=1, style={'effects': [
        {'type': 'gradient_overlay', 'gradient': {'stops': stops, 'smoothness': 0.5}}]})
    style = drive.call('get_document', document_id=doc)['layers'][1].get('style') or {}
    overlay = style['effects'][0]['gradient']
    assert abs(overlay['smoothness'] - 0.5) < 1e-9, ('overlay', style)
    drive.call('close_document', document_id=doc, discard_changes=True)
    return dict(words=words)


def gm8_host_case(drive):
    _, doc = drive.open(FIXTURES / 'GM8.psd')
    imported = drive.grid(doc)
    drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'smoothness': 0.5})
    drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'smoothness': 1})
    hosted = drive.grid(doc)
    worst = max(abs(a - b) for p, q in zip(imported, hosted) for a, b in zip(p, q))
    assert worst <= 1, ('GM8-HOST', worst)
    drive.call('close_document', document_id=doc, discard_changes=True)
    drive.gm8 = imported
    return dict(points=len(GRID), worst=worst)


def in_app_align_case(drive):
    imported = getattr(drive, 'gm8', None)
    if imported is None:
        _, gm8 = drive.open(FIXTURES / 'GM8.psd')
        imported = drive.grid(gm8)
        drive.call('close_document', document_id=gm8, discard_changes=True)
    _, doc = drive.open(FIXTURES / 'GL8.psd')
    drive.call('select_rect', document_id=doc, x=BOX[0], y=BOX[1], width=BOX[2], height=BOX[3])
    drive.call('add_layer_mask', document_id=doc, layer=1, kind='from_selection')
    start = drive.cursor(doc)
    reply = drive.call('edit_fill_layer', document_id=doc, layer=1,
                       gradient={'align_with_layer': True})
    assert reply.get('changed') is True and drive.cursor(doc) == start + 1, ('one step', reply)
    aligned = drive.grid(doc)
    worst = max(abs(a - b) for p, q in zip(imported, aligned) for a, b in zip(p, q))
    assert worst <= 1, ('IN-APP-ALIGN', worst)
    drive.call('close_document', document_id=doc, discard_changes=True)
    return dict(points=len(GRID), worst=worst)


def wide_case(drive):
    p3_green = [0.0, 255.0, 0.0]
    doc = drive.call('new_document', width=64, height=48, depth=16, profile='display_p3',
                     background='white')['document']['id']
    stops = [dict(position=0, color='#FF0000'), dict(position=0.25, color='#00FF00'),
             dict(position=0.75, color='#00FF00'), dict(position=1, color='#0000FF')]
    drive.call('add_fill_layer', document_id=doc, kind='gradient',
               gradient=dict(stops=stops, style='linear', angle=0))
    exported = drive.folder / 'wide.psd'
    drive.call('export_psd', document_id=doc, path=str(exported), format='psd', mode='layers')
    drive.call('close_document', document_id=doc, discard_changes=True)
    data = bytearray(exported.read_bytes())
    start, body = gdfl_block(bytes(data))
    for key, value in ((b'Rd  ', 0.0), (b'Grn ', 255.0), (b'Bl  ', 0.0)):
        found = doubles(body, key)
        assert len(found) == 4, (key, found)
        for at, _ in found[1:3]:
            data[start + at:start + at + 8] = struct.pack('>d', value)
    patched = drive.folder / 'wide-p3.psd'
    patched.write_bytes(bytes(data))
    report, doc = drive.open(patched)
    assert any('Its stop colours are kept as wide colours.' in o
               for o in report['observations']), report['observations']
    centre = drive.pixel16(doc, 32, 24)
    assert near(centre, p3_green), ('WIDE', centre)
    step = drive.cursor(doc)
    drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'angle': 180})
    assert drive.cursor(doc) == step + 1, 'WIDE angle: one step'
    assert near(drive.pixel16(doc, 32, 24), p3_green), 'WIDE angle keeps the wide stops'
    same = [dict(position=s['position'], color=s['color'], opacity=s['opacity'])
            for s in drive.fill(doc)['gradient']['stops']]
    assert same[1]['color'].lower() == '#00ff00', same
    reply = drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'stops': same})
    assert reply.get('changed') is False and drive.cursor(doc) == step + 1, ('SAME', reply)
    assert near(drive.pixel16(doc, 32, 24), p3_green), 'SAME keeps the wide stops'
    changed = [dict(position=0, color='#FF0000'), dict(position=0.25, color='#00FF00'),
               dict(position=0.7, color='#00FF00'), dict(position=1, color='#0000FF')]
    drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'stops': changed})
    assert drive.cursor(doc) == step + 2, 'CHANGED: one step'
    srgb_green = classic([(0, srgb_in_p3([255, 0, 0])), (0.25, srgb_in_p3([0, 255, 0])),
                          (0.7, srgb_in_p3([0, 255, 0])), (1, srgb_in_p3([0, 0, 255]))], 0.5)
    edited = drive.pixel16(doc, 32, 24)
    assert near(edited, srgb_green, 2.0), ('CHANGED', edited, srgb_green)
    assert abs(edited[0] - p3_green[0]) > 50, ('the wide stops are dropped', edited)
    drive.call('undo', document_id=doc)
    restored = drive.pixel16(doc, 32, 24)
    assert near(restored, p3_green), ('UNDO', restored)
    drive.call('close_document', document_id=doc, discard_changes=True)
    return dict(centre=centre, edited=edited, restored=restored, srgb_green=srgb_green)


def midpoint_case(drive):
    stops = [dict(position=0, color='#000000', midpoint=0.25),
             dict(position=1, color='#ffffff')]
    doc = drive.call('new_document', width=256, height=8, depth=8,
                     background='white')['document']['id']
    start = drive.cursor(doc)
    drive.call('add_fill_layer', document_id=doc, kind='gradient',
               gradient=dict(stops=stops, style='linear', angle=0))
    assert drive.cursor(doc) == start + 1, 'MIDPOINT: one step'
    shown = drive.fill(doc)['gradient']['stops']
    assert shown[0].get('midpoint') == 0.25 and 'midpoint' not in shown[1], ('report', shown)
    reds = {}
    for x in range(16, 256, 32):
        red = drive.call('sample_pixel', document_id=doc, source='layer', layer=1, x=x,
                         y=4)['center_rgba_codes'][0]
        want = 255 * two_stop_s(midpoint_g(x / 256, 0.25))
        assert abs(red - want) <= 1, ('fill', x, red, want)
        reds[x] = red
    exported = drive.folder / 'midpoint.psd'
    drive.call('export_psd', document_id=doc, path=str(exported), format='psd', mode='layers')
    _, body = gdfl_block(exported.read_bytes())
    mdpn = mdpn_values(body)
    assert mdpn == [50, 25, 50, 50], ('Mdpn', mdpn)
    _, back = drive.open(exported)
    assert drive.fill(back)['gradient']['stops'][0].get('midpoint') == 0.25, 'PSD reopen'
    drive.call('close_document', document_id=back, discard_changes=True)
    saved = drive.folder / 'midpoint.rz'
    drive.call('save_copy', document_id=doc, path=str(saved), format='rz')
    data = saved.read_bytes()
    words = [struct.unpack('<I', data[at:at + 4])[0] for at in (4, 8)]
    assert words == [45, 104], ('the .rz words', words)
    back = drive.call('open_document', path=str(saved))['document']['id']
    assert drive.fill(back)['gradient']['stops'][0].get('midpoint') == 0.25, '.rz reopen'
    drive.call('close_document', document_id=back, discard_changes=True)
    overlay = [dict(position=0, color='#203040', opacity=1, midpoint=0.3, opacity_midpoint=0.7),
               dict(position=1, color='#e0c0a0', opacity=0.5)]
    drive.call('set_layer_style', document_id=doc, layer=1, style={'effects': [
        {'type': 'gradient_overlay', 'gradient': {'stops': overlay}}]})
    style = drive.call('get_document', document_id=doc)['layers'][1].get('style') or {}
    got = style['effects'][0]['gradient']['stops'][0]
    assert abs(got.get('midpoint', 0.5) - 0.3) < 1e-6 \
        and abs(got.get('opacity_midpoint', 0.5) - 0.7) < 1e-6, ('style', style)
    drive.call('undo', document_id=doc)
    drive.call('undo', document_id=doc)
    layers = drive.call('get_document', document_id=doc)['layers']
    assert len(layers) == 1, ('undo drops the layer', len(layers))
    drive.call('close_document', document_id=doc, discard_changes=True)
    tool = {}
    for depth in (8, 16):
        doc = drive.call('new_document', width=256, height=8, depth=depth,
                         background='white')['document']['id']
        drive.call('gradient', document_id=doc, layer=0, x0=0, y0=4, x1=256, y1=4,
                   shape='linear', stops=stops)
        scale = 257.0 if depth == 16 else 1.0
        for x in range(16, 256, 32):
            red = drive.call('sample_pixel', document_id=doc, source='layer', layer=0, x=x,
                             y=4)['center_rgba_codes'][0] / scale
            want = 255 * two_stop_s(midpoint_g((x + 0.5) / 256, 0.25))
            assert abs(red - want) <= 1, ('tool', depth, x, red, want)
            tool[f'{depth}:{x}'] = red
        drive.call('close_document', document_id=doc, discard_changes=True)
    return dict(fill=reds, mdpn=mdpn, words=words, tool=tool)


def midpoint_segments_case(drive):
    stops = [dict(position=0, color='#000000', opacity=1, midpoint=0.25, opacity_midpoint=0.3),
             dict(position=0.5, color='#ffffff', opacity=1, midpoint=0.75,
                  opacity_midpoint=0.8),
             dict(position=1, color='#000000', opacity=1)]
    doc = drive.call('new_document', width=256, height=8, depth=8,
                     background='white')['document']['id']
    start = drive.cursor(doc)
    drive.call('add_fill_layer', document_id=doc, kind='gradient',
               gradient=dict(stops=stops, style='linear', angle=0, smoothness=0))
    assert drive.cursor(doc) == start + 1, 'MIDPOINT-SEGMENTS: one step'

    def check_stops(shown, what):
        colour = [s.get('midpoint') for s in shown]
        opacity = [s.get('opacity_midpoint') for s in shown]
        assert colour == [0.25, 0.75, None] and opacity == [0.3, 0.8, None], (what, shown)

    check_stops(drive.call('get_document', document_id=doc)['layers'][1]['fill']['gradient']
                ['stops'], 'get_document')
    # Stop i's midpoint governs segment i..i + 1: black -> white through 0.25,
    # white -> black through 0.75, linear (Smoothness 0).
    ramp = [(0.0, 0.0, 0.25), (0.5, 1.0, 0.75), (1.0, 0.0, 0.5)]
    reds = {}
    for x in sorted(set(range(16, 256, 32)) | {32, 224}):
        t = x / 256
        k = 0 if t < 0.5 else 1
        (p0, v0, m), (p1, v1, _) = ramp[k], ramp[k + 1]
        want = 255 * (v0 + (v1 - v0) * midpoint_g((t - p0) / (p1 - p0), m))
        red = drive.call('sample_pixel', document_id=doc, source='layer', layer=1, x=x,
                         y=4)['center_rgba_codes'][0]
        assert abs(red - want) <= 1, ('segment', x, red, want)
        reds[x] = red
    assert abs(reds[32] - 127.5) <= 1 and abs(reds[224] - 127.5) <= 1, ('half way', reds)
    exported = drive.folder / 'midpoint-segments.psd'
    drive.call('export_psd', document_id=doc, path=str(exported), format='psd', mode='layers')
    _, body = gdfl_block(exported.read_bytes())
    mdpn = mdpn_values(body)
    assert mdpn == [50, 25, 75, 50, 30, 80], ('Mdpn', mdpn)
    _, back = drive.open(exported)
    check_stops(drive.fill(back)['gradient']['stops'], 'PSD reopen')
    drive.call('close_document', document_id=back, discard_changes=True)
    edited = [dict(stop) for stop in stops]
    edited[1]['midpoint'] = 0.4
    start = drive.cursor(doc)
    drive.call('edit_fill_layer', document_id=doc, layer=1, gradient={'stops': edited})
    assert drive.cursor(doc) == start + 1, 'edit: one step'
    got = drive.fill(doc)['gradient']['stops'][1].get('midpoint')
    assert got == 0.4, ('edit', got)
    drive.call('undo', document_id=doc)
    got = drive.fill(doc)['gradient']['stops'][1].get('midpoint')
    assert got == 0.75, ('undo', got)
    drive.call('close_document', document_id=doc, discard_changes=True)
    return dict(fill=reds, mdpn=mdpn)


def tool_smoothness_case(drive):
    stops = [dict(position=0, color='#000000'), dict(position=0.3, color='#ffffff'),
             dict(position=1, color='#404040')]
    ramp = [(0.0, 0.0), (0.3, 1.0), (1.0, 64 / 255)]
    xs = list(range(16, 256, 32))
    k = 0.5

    def reds(doc, depth):
        scale = 257.0 if depth == 16 else 1.0
        return {x: drive.call('sample_pixel', document_id=doc, source='layer', layer=0, x=x,
                              y=4)['center_rgba_codes'][0] / scale for x in xs}

    def paint(doc, **extra):
        start = drive.cursor(doc)
        drive.call('gradient', document_id=doc, layer=0, x0=0, y0=4, x1=256, y1=4,
                   shape='linear', **extra)
        assert drive.cursor(doc) == start + 1, ('one step', extra)

    out = {}
    for depth in (8, 16):
        doc = drive.call('new_document', width=256, height=8, depth=depth,
                         background='white')['document']['id']
        got = {}
        for key, extra in (('0.5', dict(smoothness=0.5)), ('none', {}), ('0', dict(smoothness=0)),
                           ('1', dict(smoothness=1))):
            paint(doc, stops=stops, **extra)
            got[key] = reds(doc, depth)
        for x in xs:
            t = (x + 0.5) / 256
            mixed = (1 - k) * linear_ramp(ramp, t) + k * classic_unclamped(ramp, t)
            want = 255 * min(1.0, max(0.0, mixed))
            assert abs(got['0.5'][x] - want) <= 1, ('k 0.5', depth, x, got['0.5'][x], want)
        assert got['none'] == got['1'], ('no key is 1', depth, got['none'], got['1'])
        assert any(abs(got['none'][x] - got['0'][x]) > 2 for x in xs), ('0 differs', depth, got)
        paint(doc, start_color='#000000', end_color='#ffffff', smoothness=0)
        two = reds(doc, depth)
        for x in xs:
            want = 255 * (x + 0.5) / 256
            assert abs(two[x] - want) <= 1, ('two-colour linear', depth, x, two[x], want)
        start = drive.cursor(doc)
        text = drive.call('gradient', fail=True, document_id=doc, layer=0, x0=0, y0=4, x1=256,
                          y1=4, shape='linear', stops=stops, smoothness=1.5)
        assert 'smoothness must be between 0 and 1' in text, ('refusal', depth, text)
        assert drive.cursor(doc) == start, ('a refusal adds no step', depth)
        drive.call('close_document', document_id=doc, discard_changes=True)
        out[depth] = dict(got, two=two)
    doc = drive.call('new_document', width=256, height=8, depth=8,
                     background='white')['document']['id']
    assert drive.call('start_recording', discard=True)['ok']
    paint(doc, stops=stops, smoothness=0.5)
    direct = reds(doc, 8)
    steps = drive.call('stop_recording', name='tool-smoothness')['steps']
    drive.call('close_document', document_id=doc, discard_changes=True)
    gradients = [step for step in steps if step['tool'] == 'gradient']
    assert len(gradients) == 1 and gradients[0]['arguments'].get('smoothness') == 0.5, steps
    fresh = drive.call('new_document', width=256, height=8, depth=8,
                       background='white')['document']['id']
    assert drive.call('run_action', document_id=fresh, name='tool-smoothness')['ok']
    replayed = reds(fresh, 8)
    assert replayed == direct, ('replay', direct, replayed)
    drive.call('close_document', document_id=fresh, discard_changes=True)
    return dict(paint=out, recorded=gradients[0]['arguments'], replayed=replayed)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', required=True, help='a fresh folder')
    arguments = parser.parse_args()
    output = Path(arguments.output).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=False)
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    results, failed = {}, []
    with App(output / 'app', output / 'recovery', interval=600) as app:
        drive = Drive(app, output)
        cases = [(key, lambda key=key: open_case(drive, key)) for key in KEYS]
        cases.append(('WIDE', lambda: wide_case(drive)))
        cases.append(('SMOOTH-ALIGN', lambda: smooth_align_case(drive)))
        cases.append(('GM8-HOST', lambda: gm8_host_case(drive)))
        cases.append(('IN-APP-ALIGN', lambda: in_app_align_case(drive)))
        cases.append(('MIDPOINT', lambda: midpoint_case(drive)))
        cases.append(('MIDPOINT-SEGMENTS', lambda: midpoint_segments_case(drive)))
        cases.append(('TOOL-SMOOTHNESS', lambda: tool_smoothness_case(drive)))
        for name, run in cases:
            try:
                results[name] = dict(passed=True, detail=run())
                print(f'PASS {name}', flush=True)
            except BaseException as error:  # noqa: BLE001 - every case reports
                results[name] = dict(passed=False, error=repr(error)[:2000])
                failed.append(name)
                print(f'FAIL {name}: {error!r}'[:2400], flush=True)
    (output / 'receipt.json').write_text(json.dumps(results, indent=2) + '\n')
    print('ALL CASES PASS' if not failed else f'FAILED: {failed}')
    raise SystemExit(1 if failed else 0)


if __name__ == '__main__':
    main()
