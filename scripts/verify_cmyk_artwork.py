#!/usr/bin/env python3
"""The #547 artwork-batch MCP drive: type, shape and Solid Color / Gradient
fill layers on a CMYK document — made, edited, rasterized, saved and reopened
at 8 and 16 bits, carried through Image ▸ Mode both ways, Convert to Profile,
copy and paste and PSD export — and the RGB document's ink refusals.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket, a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.
PASTEBOARD ISOLATION: `DR_PASTEBOARD_NAME` is set to a private name before the
app starts, so the copy and paste never touch the general pasteboard.

Ink is read as `sample_color`'s `ink16` (the composite over a white CMYK
document, whose paper is zero ink); an opaque artwork pixel's composite IS its
ink. `ink(p)` is `round(p / 100 · 65535)`, the percents chosen away from a
half code.

- **A1 make.** On a 16-bit and an 8-bit CMYK document: `add_fill_layer`
  solid `ink` FILL, `add_shape_layer` rect `fill_ink` SHAPE_FILL with
  `stroke_ink`, `add_text_layer` `ink` TEXT_INK. Each reply reports the ink
  percents exactly as passed (and the CMYK `working_memory_plan_bytes`), and
  the fill's and the shape interior's composite ink16 is EXACTLY ink().
- **A2 edit keeps ink.** `edit_text_layer` (new string), `edit_shape_layer`
  (new w) and `edit_fill_layer` with no colour argument keep each stored ink
  EXACTLY; `edit_fill_layer` with a new `ink` recolours it exactly.
- **A3 gradient.** A gradient stored out of position order (stops[0] at 1,
  K 100; stops[1] at 0, C 100), angle 0 over a 64 px canvas: the first column
  is ≥ 97 % C and ≤ 3 % K, the last ≤ 3 % C and ≥ 97 % K. An angle-only
  `edit_fill_layer` keeps `stop_inks` (each stop's reported `ink`) EXACTLY,
  the K-only stop still 0, 0, 0, 100.
- **A3b geometry.** On a copy of the A1 16-bit document: `rotate` 90 (an
  exact quarter turn, composed into the descriptions), `rotate` 7 (any other
  angle, the captured re-render) and `align_layers` of the shape and text
  layers to the canvas's left edge keep the text, shape and fill objects
  with their inks EXACTLY.
- **A4 rasterize, save, reopen.** `rasterize_artwork` on the shape drops its
  description (undo restores it); `save_copy rz` reopens with every text,
  shape and fill object and every probe's ink16 equal.
- **A5 Mode.** `convert_mode rgb` keeps the text, shape and fill layers
  editable (their objects reported, ink "srgb"); `convert_mode cmyk` back
  keeps them editable in ink. An RGB document with a text layer at index 2
  and a Levels layer converts with `flatten` to ONE layer; without the Levels
  layer the text layer converts editable.
- **A6 Convert to Profile.** `convert_profile` into a second installed CMYK
  profile keeps the text layer editable (its ink reported); with `flatten`
  the document is one layer.
- **A7 copy and paste.** A whole-layer `copy` of the text layer pasted into
  another CMYK document of the same profile lands as an editable text layer
  with the SAME ink; into a document of the second profile, `profile`
  "preserve" (Don't Convert) keeps that ink and the default converts it.
- **A8 PSD.** `export_psd` mode layers names the text layer on the pixels-only
  loss line.
- **A9 refusals.** On an RGB document `add_text_layer` `ink`,
  `add_shape_layer` `fill_ink` and `add_fill_layer` `gradient.stops[].ink`
  each refuse with "ink is a CMYK document's colour. Pass color."

Usage: python3 -B scripts/verify_cmyk_artwork.py [--receipts <folder>]
"""
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SECOND_PRESS = 'Coated FOGRA39 (ISO 12647-2:2004)'
W, H = 64, 40
FILL = [20, 40, 0, 60]
FILL_2 = [60, 0, 20, 40]
SHAPE_FILL = [0, 80, 20, 0]
SHAPE_STROKE = [100, 0, 0, 0]
TEXT_INK = [0, 0, 0, 100]
SHAPE_BOX = dict(x=8, y=8, w=24, h=16)
SHAPE_INSIDE = (20, 16)
FILL_PROBE = (60, 36)
RGB_INK_REFUSAL = "ink is a CMYK document's colour. Pass color."
LOSS = 'has a re-editable description (text or shape); it is exported as pixels only.'
GRADIENT_FAR = 0.97
GRADIENT_NEAR = 0.03


def code(percent):
    return int(percent / 100 * 65535 + 0.5)


def codes(percents):
    return [code(p) for p in percents]


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), f'the Generic CMYK profile is missing: {GENERIC}'
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-artwork-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    pasteboard = f'com.kgalligan.doctored.verify-cmyk-artwork.{os.getpid()}'
    os.environ['DR_PASTEBOARD_NAME'] = pasteboard
    from phase5_support import App  # after the pasteboard name is set
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_artwork.py before any measurement',
                'pasteboard': pasteboard}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def ink(doc, x, y):
            sample = call('sample_color', document_id=doc, x=x, y=y)
            assert sample.get('color_mode') == 'cmyk', sample
            return sample['ink16'] + [sample['alpha16']]

        def layers(doc):
            return call('get_document', document_id=doc)['layers']

        def layer(doc, index):
            return next(l for l in layers(doc) if l['index'] == index)

        def cmyk_doc(depth):
            return call('new_document', width=W, height=H, unit='px', color_mode='cmyk',
                        depth=depth, background='white')['document']['id']

        def rgb_doc():
            return call('new_document', width=W, height=H, depth=16, profile='srgb',
                        background='white')['document']['id']

        # ---- A1, A2: make and edit at 16 and 8 bits ---------------------------------------
        made = {}
        for depth in (16, 8):
            doc = cmyk_doc(depth)
            fill = call('add_fill_layer', document_id=doc, kind='solid', ink=FILL)
            assert fill['fill']['ink'] == FILL, fill
            assert fill['fill']['color_space'] == 'cmyk', fill
            assert fill['working_memory_plan_bytes'] > 0, fill
            assert ink(doc, *FILL_PROBE) == codes(FILL) + [65535], (depth, ink(doc, *FILL_PROBE))
            shape = call('add_shape_layer', document_id=doc, kind='rect', fill_ink=SHAPE_FILL,
                         stroke_ink=SHAPE_STROKE, stroke_width=2, **SHAPE_BOX)
            assert shape['shape']['fill_ink'] == SHAPE_FILL, shape
            assert shape['shape']['stroke_ink'] == SHAPE_STROKE, shape
            assert shape['shape']['version'] == 6, shape
            assert ink(doc, *SHAPE_INSIDE) == codes(SHAPE_FILL) + [65535], ink(doc, *SHAPE_INSIDE)
            text = call('add_text_layer', document_id=doc, text='Ink', x=36, y=2, size=24,
                        ink=TEXT_INK)
            assert text['text']['ink'] == TEXT_INK, text
            text_index = text['layer']
            shape_index = shape['layer']
            fill_index = fill['layer']
            edited = call('edit_text_layer', document_id=doc, layer=text_index, text='Inks')
            assert edited['text']['ink'] == TEXT_INK, edited
            reshaped = call('edit_shape_layer', document_id=doc, layer=shape_index, w=26)
            assert reshaped['shape']['fill_ink'] == SHAPE_FILL, reshaped
            assert reshaped['shape']['stroke_ink'] == SHAPE_STROKE, reshaped
            assert ink(doc, *SHAPE_INSIDE) == codes(SHAPE_FILL) + [65535]
            recoloured = call('edit_fill_layer', document_id=doc, layer=fill_index, ink=FILL_2)
            assert recoloured['fill']['ink'] == FILL_2, recoloured
            assert ink(doc, *FILL_PROBE) == codes(FILL_2) + [65535]
            made[depth] = {'doc': doc, 'text': text_index, 'shape': shape_index,
                           'fill': fill_index, 'replies': [fill, shape, text, edited, reshaped,
                                                           recoloured]}
        receipts['a1_a2'] = {str(d): v['replies'] for d, v in made.items()}
        print('PASS a1/a2: fill, shape and text layers in ink at 16 and 8 bits, edits keep ink',
              flush=True)

        # ---- A3: gradient --------------------------------------------------------------
        gdoc = cmyk_doc(16)
        gradient = call('add_fill_layer', document_id=gdoc, kind='gradient', gradient={
            'stops': [{'position': 1, 'ink': [0, 0, 0, 100]},
                      {'position': 0, 'ink': [100, 0, 0, 0]}],
            'angle': 0, 'style': 'linear'})
        stops = gradient['fill']['gradient']['stops']
        assert [s['ink'] for s in stops] == [[0, 0, 0, 100], [100, 0, 0, 0]], stops
        first, last = ink(gdoc, 0, H // 2), ink(gdoc, W - 1, H // 2)
        assert first[0] >= GRADIENT_FAR * 65535 and first[3] <= GRADIENT_NEAR * 65535, first
        assert last[0] <= GRADIENT_NEAR * 65535 and last[3] >= GRADIENT_FAR * 65535, last
        turned = call('edit_fill_layer', document_id=gdoc, layer=gradient['layer'],
                      gradient={'angle': 30})
        turned_stops = turned['fill']['gradient']['stops']
        assert [s['ink'] for s in turned_stops] == [[0, 0, 0, 100], [100, 0, 0, 0]], turned_stops
        assert turned['fill']['gradient']['angle'] == 30, turned
        receipts['a3'] = {'add': gradient, 'first': first, 'last': last, 'turned': turned}
        print('PASS a3: an out-of-order gradient renders C at its start and K at its end, and an '
              'angle-only edit keeps stop_inks', flush=True)

        # ---- A3b: geometry --------------------------------------------------------------
        geo_path = folder / 'geometry.rz'
        call('save_copy', document_id=made[16]['doc'], path=str(geo_path), format='rz')
        geo = call('open_document', path=str(geo_path))['document']['id']

        def inks_of(doc):
            text = layer(doc, made[16]['text'])['text']
            shape = layer(doc, made[16]['shape'])['shape']
            fill = layer(doc, made[16]['fill'])['fill']
            return text['ink'], shape['fill_ink'], shape['stroke_ink'], fill['ink']

        expected = (TEXT_INK, SHAPE_FILL, SHAPE_STROKE, FILL_2)
        assert inks_of(geo) == expected, inks_of(geo)
        g3 = {}
        for name, arguments in (('rotate', dict(degrees=90)), ('rotate', dict(degrees=7)),
                                ('align_layers', dict(layers=[made[16]['shape'], made[16]['text']],
                                                      edge='left', to='canvas'))):
            reply = call(name, document_id=geo, **arguments)
            assert inks_of(geo) == expected, (name, arguments, inks_of(geo))
            g3[f'{name} {arguments}'] = reply
        receipts['a3b'] = g3
        print('PASS a3b: quarter turn, any-angle rotate and align keep every description and ink',
              flush=True)

        # ---- A4: rasterize, save, reopen -------------------------------------------------
        doc = made[16]['doc']
        shape_index = made[16]['shape']
        call('rasterize_artwork', document_id=doc, layer=shape_index)
        assert 'shape' not in layer(doc, shape_index), layer(doc, shape_index)
        call('undo', document_id=doc)
        assert layer(doc, shape_index)['shape']['fill_ink'] == SHAPE_FILL
        probes = [FILL_PROBE, SHAPE_INSIDE, (40, 10)]
        before = {p: ink(doc, *p) for p in probes}
        described = {i: {k: layer(doc, i).get(k) for k in ('text', 'shape', 'fill')}
                     for i in (made[16]['text'], shape_index, made[16]['fill'])}
        rz = folder / 'artwork.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        for i, objects in described.items():
            for key, value in objects.items():
                got = layer(again, i).get(key)
                assert json.dumps(got, sort_keys=True) == json.dumps(value, sort_keys=True), \
                    (i, key, got, value)
        assert layer(again, made[16]['text'])['text']['ink'] == TEXT_INK
        for p in probes:
            assert ink(again, *p) == before[p], (p, ink(again, *p), before[p])
        receipts['a4'] = {'described': described}
        print('PASS a4: rasterize drops the description (undo restores it); .rz reopens every '
              'description and ink', flush=True)

        # ---- A5: Mode ----------------------------------------------------------------------
        call('convert_mode', document_id=again, mode='rgb', rgb_space='srgb')
        rgb_layers = layers(again)
        rgb_text = next(l for l in rgb_layers if 'text' in l)
        assert rgb_text['text']['ink'] == 'srgb', rgb_text
        assert any('shape' in l for l in rgb_layers) and any('fill' in l for l in rgb_layers)
        call('convert_mode', document_id=again, mode='cmyk', profile='file', path=str(GENERIC))
        back = next(l for l in layers(again) if 'text' in l)
        assert isinstance(back['text']['ink'], list) and len(back['text']['ink']) == 4, back
        flat_source = rgb_doc()
        call('new_layer', document_id=flat_source, name='Mid')
        call('add_text_layer', document_id=flat_source, text='Flat', x=4, y=4, size=20,
             color='#204080')
        assert layers(flat_source)[-1]['index'] == 2 and 'text' in layers(flat_source)[-1]
        call('add_adjustment_layer', document_id=flat_source, op='levels',
             params={'output_black': 0.1})
        call('convert_mode', document_id=flat_source, mode='cmyk', profile='file',
             path=str(GENERIC), flatten=True)
        assert len(layers(flat_source)) == 1, layers(flat_source)
        editable_source = rgb_doc()
        call('new_layer', document_id=editable_source, name='Mid')
        call('add_text_layer', document_id=editable_source, text='Kept', x=4, y=4, size=20,
             color='#204080')
        call('convert_mode', document_id=editable_source, mode='cmyk', profile='file',
             path=str(GENERIC))
        kept_text = layer(editable_source, 2)
        assert 'text' in kept_text and len(kept_text['text']['ink']) == 4, kept_text
        receipts['a5'] = {'rgb_text': rgb_text['text'], 'back': back['text'],
                          'kept': kept_text['text']}
        print('PASS a5: Mode both ways keeps text, shape and fill editable; Flatten with a '
              'Levels layer gives one layer', flush=True)

        # ---- A6: Convert to Profile ---------------------------------------------------------
        presses = call('list_installed_profiles', kinds=['cmyk'])['profiles']
        second = next((p for p in presses if p['name'] == SECOND_PRESS and p['proofable']), None) \
            or next((p for p in presses if p['proofable'] and p['name'] != 'Generic CMYK Profile'),
                    None)
        assert second, f'no second proofable CMYK profile is installed: {presses}'
        convert_doc = made[8]['doc']
        call('convert_profile', document_id=convert_doc, profile='installed', name=second['name'])
        converted_text = layer(convert_doc, made[8]['text'])
        assert 'text' in converted_text and len(converted_text['text']['ink']) == 4, converted_text
        flat_convert = cmyk_doc(16)
        call('add_text_layer', document_id=flat_convert, text='Flat', x=4, y=4, size=20,
             ink=TEXT_INK)
        call('convert_profile', document_id=flat_convert, profile='installed',
             name=second['name'], flatten=True)
        assert len(layers(flat_convert)) == 1, layers(flat_convert)
        receipts['a6'] = {'second': second['name'], 'converted_text': converted_text['text']}
        print('PASS a6: Convert to Profile keeps the text layer editable; with Flatten one layer',
              flush=True)

        # ---- A7: copy and paste ------------------------------------------------------------
        source = made[16]['doc']
        target = cmyk_doc(16)
        call('deselect', document_id=source)
        call('copy', document_id=source, layer=made[16]['text'])
        pasted = call('paste', document_id=target, placement='in_place')
        landed = next(l for l in layers(target) if 'text' in l)
        assert landed['text']['ink'] == TEXT_INK, landed
        # Into the second profile (A6's document): Don't Convert keeps the record's ink
        # verbatim, Convert re-separates it.
        kept = call('paste', document_id=convert_doc, placement='in_place', profile='preserve')
        kept_text = layer(convert_doc, kept['layer'])['text']
        assert kept_text['ink'] == TEXT_INK, kept_text
        moved = call('paste', document_id=convert_doc, placement='in_place')
        moved_text = layer(convert_doc, moved['layer'])['text']
        assert moved_text['ink'] != TEXT_INK, moved_text
        receipts['a7'] = {'paste': pasted, 'landed': landed['text'], 'preserved': kept_text,
                          'converted': moved_text}
        print('PASS a7: a whole-layer copy of a CMYK text layer pastes editable with its ink',
              flush=True)

        # ---- A8: PSD ----------------------------------------------------------------------
        psd = folder / 'artwork.psd'
        written = call('export_psd', document_id=source, path=str(psd), format='psd',
                       mode='layers')
        losses = '\n'.join(written.get('losses', []))
        assert f'Layer “Inks” {LOSS}' in losses, losses
        receipts['a8'] = written.get('losses')
        print('PASS a8: the layered PSD names the text layer on the pixels-only loss line',
              flush=True)

        # ---- A9: refusals -------------------------------------------------------------------
        rgb = rgb_doc()
        refusals = {
            'text': call('add_text_layer', fail=True, document_id=rgb, text='x', x=1, y=1,
                         ink=TEXT_INK),
            'shape': call('add_shape_layer', fail=True, document_id=rgb, kind='rect', x=1, y=1,
                          w=4, h=4, fill_ink=SHAPE_FILL),
            'fill': call('add_fill_layer', fail=True, document_id=rgb, kind='gradient',
                         gradient={'stops': [{'position': 0, 'ink': [0, 0, 0, 100]},
                                             {'position': 1, 'color': '#ffffff'}]}),
        }
        for key, text in refusals.items():
            assert RGB_INK_REFUSAL in text, (key, text)
        receipts['a9'] = refusals
        print('PASS a9: an RGB document refuses every ink argument by name', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True,
                                                    default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
