#!/usr/bin/env python3
"""File ▸ Save of a flat Grayscale, CMYK, Indexed Color or Bitmap file keeps the flat
type only while the file can hold the document.

The instance is owned by this file (`phase5_support.App`: a scratch MCP socket, a
dummy API key, private preference folders), and only it is terminated. No key press
and no focus: `get_document`'s `save_preservation.save_types` is File ▸ Save's own
list (`ImageDocument.writableTypes(for: .saveOperation)`), and a type that leaves it
sends ⌘S to the Save panel with `.doctored` first.

Each CELL is a flat file the app itself writes (`save_copy`) and opens again
(`open_document`), so the document's file type is that flat type. For each STATE the
mode's tools allow, on a fresh open of the cell's file:

- before: the type is in `save_types` (a plain file saves in place);
- after the state is added: the type has left `save_types`, `save_as_types` leads
  with `.doctored`, `losses_by_format` names the state's line, and a flat
  `save_copy` reports the same list in `flat_save_losses`;
- the oracle: that copy, opened again, has the plain file's structure (layers,
  channels, guides, ruler, samplers, paths, comps, global light, depth), so the line
  named something the file really dropped;
- a one-call state: `undo` puts the type back in `save_types`.

KEPT cells stay in `save_types`: a Grayscale PNG with transparency and an Indexed
PNG whose transparent index is used.

PACKET cells: a Grayscale, Indexed Color and Bitmap document holding an IPTC packet
(opened from a JPEG the drive gives one). The core's carry table has no IPTC slot in
a PNG, so `losses_by_format.png` names "IPTC metadata", PNG is not in `save_as_types`,
and the PNG copy reopens without the packet; a TIFF, which has the slot, names no
packet line, and its copy reopens with it.

Every failing check is printed and the run continues, so a red run lists every cell;
the exit status is 1 when any check failed. Run with `--output <folder>`; `--cells`
runs only the cells whose name contains one of its comma-separated words.
"""

import argparse
import json
import shutil
import sys
from pathlib import Path

from phase5_support import App
from verify_cmyk_export import iptc_resource

NATIVE = 'com.kgalligan.doctored-document'
TYPES = {'png': 'public.png', 'tiff': 'public.tiff', 'jpeg': 'public.jpeg'}
KEYS = {'png': 'png', 'tiff': 'tiff', 'jpeg': 'jpg'}  # `losses_by_format`'s keys

PROPS = 'layer properties'
GUIDES = 'guides and ruler origin'
COMBINED = 'saved channels, paths, colour samplers, guides and the ruler origin'
BOX = {'closed': True, 'anchors': [[1, 1], [4, 1], [4, 3], [1, 3]]}

# name: (calls, expected line). A call is (tool, arguments).
STATES = {
    'guide': ([('add_guide', {'orientation': 'vertical', 'position': 2})], GUIDES),
    'ruler': ([('set_ruler_origin', {'x': 2, 'y': 1})], GUIDES),
    'channel': ([('add_channel', {'from': 'empty', 'name': 'Alpha 1'})],
                'saved selection channels'),
    'sampler': ([('set_color_samplers', {'samplers': [{'x': 2, 'y': 2}]})], 'color samplers'),
    'path': ([('set_path', {'name': 'Box', 'subpaths': [BOX]})], 'paths'),
    'comp': ([('save_layer_comp', {'name': 'Comp'})], 'layer comps'),
    'mask': ([('add_layer_mask', {'layer': 0, 'kind': 'reveal_all'})], 'layer masks'),
    'style': ([('set_layer_style', {'layer': 0, 'style': {'fill_opacity': 0.5}})],
              'layer styles'),
    'opacity': ([('set_layer_properties', {'index': 0, 'opacity': 0.5})], PROPS),
    'hidden': ([('set_layer_properties', {'index': 0, 'visible': False})], PROPS),
    'blend': ([('set_layer_properties', {'index': 0, 'blend_mode': 'Multiply'})], PROPS),
    'lock': ([('set_layer_lock', {'layer': 0, 'locks': ['position']})], PROPS),
    'label': ([('set_layer_label', {'layer': 0, 'label': 'red'})], PROPS),
    'rename': ([('set_layer_properties', {'index': 0, 'name': 'Renamed'})], PROPS),
    'layer': ([('new_layer', {})], 'layers and groups'),
    'light': ([('set_global_light', {'angle': 45, 'altitude': 20})], 'global light settings'),
    'offset': ([('set_layer_properties', {'index': 0, 'offset_x': 1, 'offset_y': 0})],
               'layer bounds and off-canvas pixels'),
    'erase': ([('eraser_stroke', {'layer': 0, 'points': [[1, 1], [3, 1]], 'size': 2,
                                  'hardness': 1})], 'pixel transparency'),
    'depth16': ([('convert_precision', {'mode': 'cmyk16'})], '16-bit CMYK precision'),
}

ALL_GRAY = ['guide', 'ruler', 'channel', 'sampler', 'path', 'comp', 'mask', 'style',
            'opacity', 'hidden', 'blend', 'lock', 'label', 'rename', 'layer', 'light',
            'offset']
SOME = ['guide', 'channel', 'mask', 'opacity', 'label']
ALL_CMYK = ['guide', 'ruler', 'channel', 'sampler', 'path', 'comp', 'mask', 'style',
            'opacity', 'hidden', 'blend', 'lock', 'label', 'rename', 'layer', 'offset',
            'erase']
ONE_LAYER = {'guide': COMBINED, 'channel': COMBINED, 'sampler': COMBINED, 'path': COMBINED,
             'lock': PROPS, 'label': PROPS, 'rename': PROPS, 'hidden': PROPS}
IPTC = 'IPTC metadata'


def with_iptc(source, path):
    """`source`, a JPEG, written to `path` with a `Photoshop 3.0` APP13 after its APP0
    holding one IPTC resource (`verify_cmyk_export.iptc_resource`)."""
    data = Path(source).read_bytes()
    assert data[:2] == b'\xff\xd8', f'{source} is not a JPEG'
    at = 4 + int.from_bytes(data[4:6], 'big') if data[2:4] == b'\xff\xe0' else 2
    payload = b'Photoshop 3.0\x00' + iptc_resource()
    segment = b'\xff\xed' + (len(payload) + 2).to_bytes(2, 'big') + payload
    Path(path).write_bytes(data[:at] + segment + data[at:])
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    parser.add_argument('--cells', default='')
    args = parser.parse_args()
    only = [word for word in args.cells.split(',') if word]

    def wanted(cell):
        return not only or any(word in cell for word in only)

    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    failures = []
    results = []

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def attempt(tool, **arguments):
            """The tool's reply, or ('refused', text) for an in-band error."""
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = ''.join(c.get('text', '') for c in result.get('content', []) or [])
            if result.get('isError'):
                return ('refused', text)
            return json.loads(text) if text.strip().startswith(('{', '[')) else text

        def preservation(doc):
            return call('get_document', document_id=doc)['save_preservation']

        def snapshot(doc):
            """The structure a flat file can hold or drop (the oracle's view)."""
            info = call('get_document', document_id=doc)
            shot = {key: info.get(key) for key in ('channels', 'guides', 'global_light',
                                                   'width', 'height')}
            shot['layers'] = [{k: v for k, v in row.items() if k not in ('id', 'thumbnail')}
                              for row in info.get('layers') or []]
            shot['ruler'] = {k: v for k, v in (info.get('ruler') or {}).items() if k != 'unit'}
            precision = attempt('get_precision', document_id=doc)
            shot['bits'] = precision.get('bits_per_channel') if isinstance(precision, dict) else None
            # The alpha the erase state clears, in the sample's own unit.
            sample = attempt('sample_color', document_id=doc, x=2, y=1)
            if isinstance(sample, dict):
                shot['alpha'] = (sample['center_rgba16'][3] if 'center_rgba16' in sample
                                 else sample.get('a', sample.get('alpha16', sample.get('alpha'))))
            for tool, key in (('get_color_samplers', 'samplers'), ('list_paths', 'paths'),
                              ('list_layer_comps', 'comps')):
                reply = attempt(tool, document_id=doc)
                if isinstance(reply, dict):
                    reply = {k: v for k, v in reply.items() if k not in ('document', 'ok')}
                shot[key] = reply
            return shot

        def check(ok, what):
            if not ok:
                failures.append(what)
                print(f'FAIL {what}', flush=True)
            return ok

        def run_cell(cell, base, fmt, states, expected=None):
            if not wanted(cell):
                return
            ftype = TYPES[fmt]
            for name in states:
                calls, line = STATES[name]
                line = (expected or {}).get(name, line)
                tag = f'{cell} {name}'
                path = folder / f'{cell}-{name}.{fmt}'
                shutil.copyfile(base, path)
                doc = call('open_document', path=str(path))['document']['id']
                before = snapshot(doc)
                ok = check(ftype in preservation(doc)['save_types'], f'{tag}: plain file saves in place')
                refused = None
                for tool, arguments in calls:
                    reply = attempt(tool, document_id=doc, **arguments)
                    if isinstance(reply, tuple):
                        refused = reply[1]
                        break
                if refused is not None:
                    print(f'SKIP {tag}: {calls[0][0]} refused: {refused[:160]}', flush=True)
                    results.append((tag, 'refused'))
                    call('close_document', document_id=doc, discard_changes=True)
                    continue
                check(snapshot(doc) != before, f'{tag}: the state took')
                after = preservation(doc)
                lost = after['losses_by_format'].get(KEYS[fmt], [])
                ok &= check(ftype not in after['save_types'],
                            f'{tag}: {ftype} left save_types ({after["save_types"]})')
                ok &= check(after['save_as_types'][:1] == [NATIVE],
                            f'{tag}: Save As leads with .doctored ({after["save_as_types"][:2]})')
                ok &= check(any(line in item for item in lost),
                            f'{tag}: losses_by_format.{KEYS[fmt]} names "{line}" ({lost})')
                copy = folder / f'{cell}-{name}-copy.{fmt}'
                reply = attempt('save_copy', document_id=doc, path=str(copy), format=fmt)
                if isinstance(reply, tuple):
                    # The flat write itself refuses (a hidden Index layer): nothing is lost
                    # silently, and there is no copy to reopen.
                    print(f'NOTE {tag}: save_copy refused: {reply[1][:160]}', flush=True)
                else:
                    ok &= check(reply.get('flat_save_losses') == lost,
                                f'{tag}: save_copy flat_save_losses {reply.get("flat_save_losses")} '
                                f'== losses_by_format {lost}')
                    again = call('open_document', path=str(copy))['document']['id']
                    reopened = snapshot(again)
                    # The copy holds the composite: a hidden, translucent or moved layer's
                    # pixels are baked into it, so only the erase state compares pixels.
                    def records(shot):
                        out = dict(shot)
                        if name != 'erase':
                            out.pop('alpha', None)
                        out['layers'] = [{k: v for k, v in row.items() if not k.startswith('content_')}
                                         for row in shot['layers']]
                        return out
                    changed = {k: (before[k], reopened[k]) for k in before
                               if records(before).get(k) != records(reopened).get(k)}
                    ok &= check(records(reopened) == records(before),
                                f'{tag}: the copy reopens without the state ({changed})')
                    call('close_document', document_id=again, discard_changes=True)
                # Colour samplers are no undo step: clearing them is the way back.
                if name == 'sampler':
                    call('set_color_samplers', document_id=doc, samplers=[])
                elif len(calls) == 1:
                    call('undo', document_id=doc)
                if name == 'sampler' or len(calls) == 1:
                    ok &= check(ftype in preservation(doc)['save_types'],
                                f'{tag}: the way back puts {ftype} back in save_types')
                call('close_document', document_id=doc, discard_changes=True)
                results.append((tag, 'PASS' if ok else 'FAIL'))
                print(f'{"PASS" if ok else "FAIL"} {tag}', flush=True)

        def kept(cell, path, fmt):
            if not wanted(cell):
                return
            doc = call('open_document', path=str(path))['document']['id']
            pres = preservation(doc)
            ok = check(TYPES[fmt] in pres['save_types'],
                       f'{cell}: kept file saves in place ({pres["losses_by_format"].get(KEYS[fmt])})')
            results.append((cell, 'PASS' if ok else 'FAIL'))
            print(f'{"PASS" if ok else "FAIL"} {cell}', flush=True)
            call('close_document', document_id=doc, discard_changes=True)

        def base_file(doc, name, fmt, **extra):
            path = folder / f'{name}.{fmt}'
            call('save_copy', document_id=doc, path=str(path), format=fmt, **extra)
            return path

        def holds_iptc(doc):
            return call('get_metadata', document_id=doc)['iptc']['present']

        def packets(cell, path, convert=None, in_place=None):
            """A document holding an IPTC packet: a PNG has no slot for it, a TIFF has."""
            if not wanted(cell):
                return
            doc = call('open_document', path=str(path))['document']['id']
            if convert:
                call('convert_mode', document_id=doc, **convert)
            ok = check(holds_iptc(doc), f'{cell}: the document holds the IPTC packet')
            pres = preservation(doc)
            lost = pres['losses_by_format']
            if in_place:
                ok &= check(in_place in pres['save_types'],
                            f'{cell}: {in_place} saves in place ({pres["save_types"]})')
            ok &= check(lost.get('png') == [IPTC],
                        f'{cell}: losses_by_format.png is ["{IPTC}"] ({lost.get("png")})')
            ok &= check(IPTC not in lost.get('tiff', []),
                        f'{cell}: losses_by_format.tiff names no packet ({lost.get("tiff")})')
            for fmt in ('png', 'tiff'):
                listed = lost.get(KEYS[fmt])
                ok &= check((TYPES[fmt] in pres['save_as_types']) == (listed == []),
                            f'{cell}: {TYPES[fmt]} is in save_as_types exactly when its list is '
                            f'empty ({pres["save_as_types"]})')
                copy = folder / f'{cell.replace(" ", "-")}-copy.{fmt}'
                reply = attempt('save_copy', document_id=doc, path=str(copy), format=fmt)
                if isinstance(reply, tuple):
                    ok &= check(False, f'{cell}: save_copy {fmt} refused: {reply[1][:160]}')
                    continue
                ok &= check(reply.get('flat_save_losses') == listed,
                            f'{cell}: save_copy {fmt} flat_save_losses '
                            f'{reply.get("flat_save_losses")} == losses_by_format {listed}')
                again = call('open_document', path=str(copy))['document']['id']
                ok &= check(holds_iptc(again) == (fmt == 'tiff'),
                            f'{cell}: the {fmt} copy reopens '
                            f'{"with" if fmt == "tiff" else "without"} the packet')
                call('close_document', document_id=again, discard_changes=True)
            call('close_document', document_id=doc, discard_changes=True)
            results.append((cell, 'PASS' if ok else 'FAIL'))
            print(f'{"PASS" if ok else "FAIL"} {cell}', flush=True)

        # ---- Grayscale --------------------------------------------------------------------
        gray8 = call('new_document', width=6, height=4, color_mode='gray', depth=8,
                     background='#808080')['document']['id']
        gray16 = call('new_document', width=6, height=4, color_mode='gray', depth=16,
                      background='#808080')['document']['id']
        run_cell('gray8-png', base_file(gray8, 'gray8', 'png'), 'png', ALL_GRAY)
        run_cell('gray8-tiff', base_file(gray8, 'gray8', 'tiff'), 'tiff', SOME)
        run_cell('gray16-tiff', base_file(gray16, 'gray16', 'tiff'), 'tiff', SOME)
        gray8_jpeg = base_file(gray8, 'gray8', 'jpeg')
        run_cell('gray8-jpeg', gray8_jpeg, 'jpeg', SOME + ['erase'])
        # Kept: a Grayscale PNG whose one layer has transparency (PNG has alpha).
        clear = call('new_document', width=6, height=4, color_mode='gray', depth=8,
                     background='#808080')['document']['id']
        call('eraser_stroke', document_id=clear, layer=0, points=[[1, 1], [3, 1]], size=2,
             hardness=1)
        alpha_png = base_file(clear, 'gray8-alpha', 'png')
        kept('gray8-png kept transparency', alpha_png, 'png')
        run_cell('gray8-png-alpha', alpha_png, 'png', ['guide'])

        # ---- CMYK ------------------------------------------------------------------------
        cmyk8 = call('new_document', width=6, height=4, unit='px', color_mode='cmyk', depth=8,
                     background='#808080')['document']['id']
        run_cell('cmyk8-tiff', base_file(cmyk8, 'cmyk8', 'tiff'), 'tiff', ALL_CMYK)
        run_cell('cmyk-jpeg', base_file(cmyk8, 'cmyk8', 'jpeg'), 'jpeg',
                 SOME + ['erase', 'depth16'])

        # ---- Indexed Color and Bitmap ----------------------------------------------------
        indexed = call('new_document', width=6, height=4, background='#808080')['document']['id']
        call('convert_mode', document_id=indexed, mode='indexed', palette='exact',
             dither='none')
        run_cell('indexed-png', base_file(indexed, 'indexed', 'png'), 'png',
                 list(ONE_LAYER), ONE_LAYER)
        clear_rgb = call('new_document', width=6, height=4, background='#808080')['document']['id']
        call('eraser_stroke', document_id=clear_rgb, layer=0, points=[[1, 1], [3, 1]], size=2,
             hardness=1)
        call('convert_mode', document_id=clear_rgb, mode='indexed', palette='exact',
             dither='none', transparency=True)
        kept('indexed-png kept transparent index', base_file(clear_rgb, 'indexed-alpha', 'png'),
             'png')
        bitmap = call('new_document', width=6, height=4, color_mode='gray', depth=8,
                      background='#808080')['document']['id']
        call('convert_mode', document_id=bitmap, mode='bitmap', method='threshold')
        run_cell('bitmap-png', base_file(bitmap, 'bitmap', 'png'), 'png',
                 ['guide', 'sampler', 'path', 'lock', 'label', 'rename', 'hidden'], ONE_LAYER)

        # ---- Capture packets: the core's carry table has no IPTC slot in a PNG ------------
        gray_iptc = with_iptc(gray8_jpeg, folder / 'gray8-iptc.jpeg')
        packets('gray8-jpeg iptc', gray_iptc, in_place=TYPES['jpeg'])
        packets('bitmap iptc', gray_iptc, convert={'mode': 'bitmap', 'method': 'threshold'})
        rgb = call('new_document', width=6, height=4, background='#808080')['document']['id']
        rgb_iptc = with_iptc(base_file(rgb, 'rgb8', 'jpeg'), folder / 'rgb8-iptc.jpeg')
        packets('indexed iptc', rgb_iptc,
                convert={'mode': 'indexed', 'palette': 'exact', 'dither': 'none'})

    print()
    for tag, outcome in results:
        print(f'{outcome:8} {tag}')
    print(f'\n{len(failures)} failed checks')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
