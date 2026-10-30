#!/usr/bin/env python3
"""Drive: STACKED layer-effect instances over MCP.

One owned Debug instance on a scratch socket builds, at RGB8, RGB16, RGB32
and CMYK16 (Generic CMYK Profile), a 96 x 64 document: opaque white (paper on
CMYK) with a "Stacked" layer holding RECT (x 28..68, y 20..44) of SOURCE.
Expectations are fixed here before any measurement.

THE STYLE (array order is render order: a later instance draws on top):
  drop_shadow   [#0000ff distance 8, #ff0000 distance 6]  angle 90, size 0, Normal 100 %
  inner_shadow  [#0000ff distance 8, #ff0000 distance 4]  the same
  stroke        [#ff0000 outside 4, #0000ff outside 2]    Normal 100 %
  color_overlay [#ff0000 50 %, #0000ff 50 %]
  gradient_overlay [#00ff00 -> #00ff00 100 %, #ff00ff -> #ff00ff 50 %]
On CMYK each colour is its ink: red (0, 1, 1, 0), blue (1, 1, 0, 0), green
(1, 0, 1, 0), magenta (0, 1, 0, 0).

THE CLOSED FORMS (W3C over, Normal; in the document's compositing space: 8-
and 16-bit RGB on the encoded codes, 32-bit in linear light, CMYK on ink;
every colour above is 0 or 1 in each), on the mid-lines at x 48 / y 32:
  - interior (48, 32): over(over(over(green, magenta, .5), red, .5), blue, .5);
  - inner rows y 20..23 red, 24..27 blue (x 48);
  - stroke rows y 19, 18 blue, 17, 16 red, 15 backdrop; columns x 27, 26
    blue, 25, 24 red, 23 backdrop (y 32); rows y 44, 45 blue, 46, 47 red;
  - shadow rows y 48, 49 red, 50, 51 blue, 52 backdrop (x 48).
Bounds: 8-bit 1 code, 16-bit and CMYK 64 codes (a quarter of an 8-bit code),
32-bit 1e-4.

ITEMS, at each depth:
  S1 `set_layer_style` with the style is ONE undo step, and `get_document`
     echoes each kind's instances in order (colours / inks).
  S2 every probe equals its closed form.
  S3 `save_copy rz` writes word 89 (RGB: 45 -> 89 at 8 bits, 46 -> 89 at 16
     and 32) or the CMYK rung 88 (46 -> 88), and the reopened copy has the
     same probes and the same instances.
  S4 (not at 32 bits: there is no float PSD writer) `export_psd` layers and
     `open_document` psd_mode layers reopen the same instances and probes;
     on CMYK also at 8 bits through `convert_precision cmyk8`.
  S5 an 11th Color Overlay refuses by name ("exceeds 10 instances of
     color_overlay") and changes nothing.
Every item prints PASS or raises.

`build_exports(call, folder)` is take2's Doctored side
(`photoshop-probe/stacked_effects_take.py --build`): the RGB16 document's
psd16 and psb16, its rgba8 conversion's psd8 and flattened TIFF; the CMYK16
document's psd16, its cmyk8 conversion's psd8 and flattened TIFF.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 96, 64
RECT = dict(x=28, y=20, width=40, height=24)
X0, Y0, X1, Y1 = 28, 20, 68, 44
MX, MY = 48, 32
SOURCE_HEX = '#30a050'
SOURCE_INK = [70, 0, 90, 0]
RGB = dict(red='#ff0000', blue='#0000ff', green='#00ff00', magenta='#ff00ff')
INK = dict(red=[0, 1, 1, 0], blue=[1, 1, 0, 0], green=[1, 0, 1, 0], magenta=[0, 1, 0, 0])
UNIT = dict(red=[1, 0, 0], blue=[0, 0, 1], green=[0, 1, 0], magenta=[1, 0, 1])
BOUND = {8: 1.0 / 255, 16: 64 / 65535, 32: 1e-4, 'cmyk': 64 / 65535}


def colour(name, cmyk):
    return {'ink': INK[name]} if cmyk else {'color': RGB[name]}


def solid_gradient(name, cmyk):
    stop = dict(colour(name, cmyk), opacity=1)
    return {'stops': [dict(stop, position=0), dict(stop, position=1)]}


def style(cmyk):
    """THE STYLE (module doc)."""
    def shadow(kind, name, distance):
        return dict(type=kind, blend='normal', opacity=1, angle=90, use_global_light=False,
                    distance=distance, size=0, **({'spread': 0} if kind == 'drop_shadow'
                                                  else {'choke': 0}), **colour(name, cmyk))

    def stroke(name, size):
        return dict(type='stroke', position='outside', size=size, blend='normal', opacity=1,
                    **colour(name, cmyk))

    def overlay(name, opacity):
        return dict(type='color_overlay', blend='normal', opacity=opacity, **colour(name, cmyk))

    def gradient(name, opacity):
        return dict(type='gradient_overlay', blend='normal', opacity=opacity,
                    gradient=solid_gradient(name, cmyk))
    return {'effects': [
        shadow('drop_shadow', 'blue', 8), shadow('drop_shadow', 'red', 6),
        shadow('inner_shadow', 'blue', 8), shadow('inner_shadow', 'red', 4),
        stroke('red', 4), stroke('blue', 2),
        overlay('red', 0.5), overlay('blue', 0.5),
        gradient('green', 1), gradient('magenta', 0.5)]}


# ---------------------------------------------------------- the oracle --

def over(cb, cs, a):
    return [b + a * (s - b) for b, s in zip(cb, cs)]


def unit(name, cmyk):
    return list(INK[name]) if cmyk else list(UNIT[name])


def expectations(cmyk):
    """{(x, y): value} on the mid-lines (module doc, THE CLOSED FORMS)."""
    paper = [0.0] * 4 if cmyk else [1.0] * 3
    u = lambda n: unit(n, cmyk)  # noqa: E731
    interior = over(over(over(u('green'), u('magenta'), 0.5), u('red'), 0.5), u('blue'), 0.5)
    out = {(MX, MY): interior}
    for y in range(20, 24):
        out[(MX, y)] = u('red')
    for y in range(24, 28):
        out[(MX, y)] = u('blue')
    for d, name in ((1, 'blue'), (2, 'blue'), (3, 'red'), (4, 'red')):
        out[(MX, Y0 - d)] = u(name)
        out[(X0 - d, MY)] = u(name)
        out[(MX, Y1 - 1 + d)] = u(name)
    out[(MX, Y0 - 5)] = paper
    out[(X0 - 5, MY)] = paper
    for y, name in ((48, 'red'), (49, 'red'), (50, 'blue'), (51, 'blue')):
        out[(MX, y)] = u(name)
    out[(MX, 52)] = paper
    return out


# ------------------------------------------------------------ the reads --

def sample(call, doc, x, y, kind):
    reply = call('sample_color', document_id=doc, x=x, y=y)
    if kind == 'cmyk':
        return [v / 65535 for v in reply['ink16'][:4]]
    if kind == 32:
        return [float(v) for v in reply['center_rgba32'][:3]]
    if kind == 16:
        return [v / 65535 for v in reply['center_rgba16'][:3]]
    hexed = reply['hex'].lstrip('#')
    return [int(hexed[i:i + 2], 16) / 255 for i in (0, 2, 4)]


def instances(style_value):
    """{type: [colour or ink, ...]} of a style's enabled effects, in order;
    a gradient overlay by its first stop."""
    out = {}
    for e in (style_value or {}).get('effects', []):
        if not e.get('enabled', True):
            continue
        if e['type'] == 'gradient_overlay':
            stop = e['gradient']['stops'][0]
            value = stop.get('ink') or stop.get('color')
        else:
            value = e.get('ink') or e.get('color')
        if isinstance(value, list):
            value = [round(v, 3) for v in value]
        elif isinstance(value, str):
            value = value.lower()
        out.setdefault(e['type'], []).append(value)
    return out


# ------------------------------------------------------------ documents --

def cmyk_document(call, folder, name):
    from verify_cmyk_structure import cmyk_tiff
    path = Path(folder) / name
    path.write_bytes(cmyk_tiff(W, H, [0] * (W * H * 4), GENERIC.read_bytes()))
    doc = call('open_document', path=str(path))['document']['id']
    assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
    return doc


def build(call, folder, kind):
    """A fresh document of `kind` (8, 16, 32 or 'cmyk') with "Stacked"."""
    cmyk = kind == 'cmyk'
    if cmyk:
        doc = cmyk_document(call, folder, 'paper.tif')
    else:
        doc = call('new_document', width=W, height=H, unit='px', depth=kind,
                   background='white')['document']['id']
    call('new_layer', document_id=doc, name='Stacked')
    call('select_rect', document_id=doc, **RECT)
    fill = {'ink': SOURCE_INK} if cmyk else {'color': SOURCE_HEX}
    call('fill_selection', document_id=doc, layer=index_of(call, doc, 'Stacked'), **fill)
    call('deselect', document_id=doc)
    return doc


def index_of(call, doc, name):
    return next(l['index'] for l in call('get_document', document_id=doc)['layers']
                if l['name'] == name)


def style_of(call, doc, name='Stacked'):
    return next(l for l in call('get_document', document_id=doc)['layers']
                if l['name'] == name).get('style')


def build_exports(call, folder):
    """take2's export sets (module doc): {name: {files, flat, layer, style, cmyk}}."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    sets = {}
    for name, kind in (('rgb', 16), ('cmyk', 'cmyk')):
        cmyk = kind == 'cmyk'
        doc = build(call, folder, kind)
        sent = style(cmyk)
        call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Stacked'), style=sent)
        files = {}
        files['psd16'] = str(folder / f'{name}16.psd')
        call('export_psd', document_id=doc, path=files['psd16'], format='psd', mode='layers')
        if not cmyk:
            files['psb16'] = str(folder / f'{name}16.psb')
            call('export_psd', document_id=doc, path=files['psb16'], format='psb', mode='layers')
        call('convert_precision', document_id=doc, mode='cmyk8' if cmyk else 'rgba8',
             **({} if cmyk else {'structure': 'preserve'}))
        files['psd8'] = str(folder / f'{name}8.psd')
        call('export_psd', document_id=doc, path=files['psd8'], format='psd', mode='layers')
        flat = str(folder / f'{name}.rz8.tif')
        call('export_psd', document_id=doc, path=flat, format='tiff', mode='flattened')
        call('close_document', document_id=doc, discard_changes=True)
        sets[name] = dict(files=files, flat=flat, layer='Stacked', style=sent, cmyk=cmyk)
    return sets


# ---------------------------------------------------------------- items --

def word_after(path, slot):
    data = Path(path).read_bytes()
    return struct.unpack('<I', data[4 + 4 * slot:8 + 4 * slot])[0]


def check_probes(call, doc, kind, want, what):
    bound = BOUND[kind]
    got = {}
    for (x, y), value in want.items():
        g = sample(call, doc, x, y, kind)
        got[f'{x},{y}'] = g
        assert all(abs(a - b) <= bound for a, b in zip(g, value)), \
            (what, kind, (x, y), g, value)
    return got


def drive_depth(call, folder, kind, receipts):
    cmyk = kind == 'cmyk'
    tag = 'cmyk16' if cmyk else f'rgb{kind}'
    work = Path(folder) / tag
    work.mkdir()
    doc = build(call, work, kind)
    layer = index_of(call, doc, 'Stacked')
    sent = style(cmyk)
    before = int(call('list_history', document_id=doc)['undo_count'])
    call('set_layer_style', document_id=doc, layer=layer, style=sent)
    assert int(call('list_history', document_id=doc)['undo_count']) == before + 1, tag
    echoed = style_of(call, doc)
    assert instances(echoed) == instances(sent), (tag, instances(echoed), instances(sent))
    print(f'PASS {tag} S1: set_layer_style is one undo step and echoes every instance', flush=True)

    want = expectations(cmyk)
    probes = check_probes(call, doc, kind, want, 'render')
    print(f'PASS {tag} S2: {len(want)} probes equal the closed forms', flush=True)

    rz = work / 'stacked.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    word = word_after(rz, 1)
    assert word == (88 if cmyk else 89), (tag, word)
    again = call('open_document', path=str(rz))['document']['id']
    check_probes(call, again, kind, want, 'reopened rz')
    assert instances(style_of(call, again)) == instances(sent), tag
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} S3: save_copy rz writes word {word} and reopens the same', flush=True)

    psd_words = []
    if kind != 32:
        psd_words.append(psd_round_trip(call, doc, kind, cmyk, want, sent, work, tag, 'psd'))
        if cmyk:
            call('convert_precision', document_id=doc, mode='cmyk8')
            psd_words.append(psd_round_trip(call, doc, 'cmyk8', cmyk, None, sent, work, tag,
                                            'psd8'))
        print(f'PASS {tag} S4: export_psd layers reopens every instance and probe', flush=True)

    target = index_of(call, doc, 'Stacked')
    eleven = {'effects': [dict(type='color_overlay', **colour('red', cmyk))] * 11}
    count = int(call('list_history', document_id=doc)['undo_count'])
    message = call('set_layer_style', fail=True, document_id=doc, layer=target, style=eleven)
    assert 'exceeds 10 instances of color_overlay' in message, (tag, message)
    assert int(call('list_history', document_id=doc)['undo_count']) == count, tag
    print(f'PASS {tag} S5: an 11th instance refuses by name', flush=True)
    call('close_document', document_id=doc, discard_changes=True)
    receipts[tag] = dict(word=word, probes=probes, echoed=instances(echoed), psd=psd_words,
                         refusal=message)


def psd_round_trip(call, doc, kind, cmyk, want, sent, work, tag, label):
    psd = work / f'{tag}-{label}.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    got = instances(style_of(call, back))
    assert got == instances(sent), (tag, label, got, instances(sent))
    if want is not None:
        check_probes(call, back, kind, want, f'reopened {label}')
    call('close_document', document_id=back, discard_changes=True)
    return dict(file=str(psd), instances=got)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    from phase5_support import App
    output = Path(args.receipts or tempfile.mkdtemp(prefix='stacked-effects-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_stacked_effects.py before any measurement'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        for kind in (8, 16, 32, 'cmyk'):
            drive_depth(app.call, folder, kind, receipts)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print('PASS: every item', flush=True)


if __name__ == '__main__':
    main()
