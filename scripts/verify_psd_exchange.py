#!/usr/bin/env python3
"""MCP drive for PSD exchange through the app: the Background mark, layer comps,
paths and vector masks (#13).

The script owns its app instance: the Debug app (`phase5_support.App`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s. It terminates only that
process. The files are the committed Photoshop-authored families under
`core/tests/fixtures/psd_photoshop` (`knockout`, `knockout_bottom`,
`layer_comps`); every expectation is the file's own record or the take's
own step table, never read back from the app first.

Cases:
- BACKGROUND: knockout_bottom's record 0 is an ordinary layer and knockout's
  is Photoshop's Background record, so get_document's bottom row reports
  background "ordinary" and "background"; both survive a .doctored save and
  reopen, and export_psd and a layered reopen.
- COMPS: layer_comps' comps that record visibility (All, HA, HB) are
  listed, and applying HA then HB gives the take's own visibilities
  (`photoshop.json`, layer_comps[0].steps).
- PATHS: a saved path and a vector mask set on knockout_bottom's Mid layer
  survive export_psd and a layered reopen (list_paths, get_vector_mask).
- CLIPPING: set_clipping_path marks one saved path (refusing the work path and
  a call with no path), undo and redo move the mark, and it survives
  export_psd, a .doctored save_copy and a flat TIFF save_copy (whose
  flat_save_losses has no "paths", a PNG's does) with their reopens; path
  null clears it; a Grayscale document answers the tool.
- MODES: a new Grayscale, Lab and CMYK document with a raster-less vector
  mask on its second layer (and, but in CMYK, which has no comp tools, a
  comp) exports with no vector-mask loss; its layered reopen marks the bottom
  "ordinary", keeps the vector mask's corners and lists the comp.

Usage: python3 scripts/verify_psd_exchange.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = ROOT / 'core/tests/fixtures/psd_photoshop'


class Drive:
    def __init__(self, app, folder):
        self.app, self.folder = app, Path(folder)

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def open_psd(self, path):
        report = self.call('inspect_psd', path=str(path))
        return self.call('open_document', path=str(path), psd_mode='layers',
                         psd_fingerprint=report['source_fingerprint'])['document']['id']

    def open_native(self, path):
        return self.call('open_document', path=str(path))['document']['id']

    def layers(self, doc):
        return self.call('get_document', document_id=doc)['layers']

    def close(self, *docs):
        for doc in docs:
            self.call('close_document', document_id=doc, discard_changes=True)


def background(drive):
    seen = {}
    for family, want in [('knockout_bottom', 'ordinary'), ('knockout', 'background')]:
        doc = drive.open_psd(FIXTURES / family / 'source.psd')
        row = drive.layers(doc)[0]
        assert row.get('background') == want, (family, row)
        assert all('background' not in r for r in drive.layers(doc)[1:]), family
        native = drive.folder / f'{family}.doctored'
        drive.call('save_copy', document_id=doc, path=str(native), format='rz')
        back = drive.open_native(native)
        assert drive.layers(back)[0].get('background') == want, (family, 'native')
        again = drive.folder / f'{family}-again.psd'
        drive.call('export_psd', document_id=doc, path=str(again), format='psd', mode='layers')
        reopened = drive.open_psd(again)
        assert drive.layers(reopened)[0].get('background') == want, (family, 'psd')
        drive.close(doc, back, reopened)
        seen[family] = want
    return seen


def comps(drive):
    take = json.loads((FIXTURES / 'layer_comps' / 'photoshop.json').read_text())
    steps = {s['step']: s['after'] for s in take['layer_comps'][0]['steps']}
    doc = drive.open_psd(FIXTURES / 'layer_comps' / 'source.psd')
    names = [c['name'] for c in drive.call('list_layer_comps', document_id=doc)['comps']]
    assert names == ['All', 'HA', 'HB'], names
    seen = {}
    for comp, step in [('HA', 'apply_ha'), ('HB', 'apply_hb')]:
        drive.call('apply_layer_comp', document_id=doc, name=comp)
        rows = {r['name']: r['visible'] for r in drive.layers(doc)}
        for name, visible, _parent in steps[step]:
            assert rows[name] == visible, (step, name, rows)
        seen[comp] = rows
    drive.close(doc)
    return seen


def paths(drive):
    doc = drive.open_psd(FIXTURES / 'knockout_bottom' / 'source.psd')
    square = {'closed': True, 'anchors': [[8, 8], [40, 8], [40, 30], [8, 30]]}
    drive.call('set_path', document_id=doc, name='Logo', subpaths=[square])
    drive.call('set_vector_mask', document_id=doc, layer=1, subpaths=[square])
    again = drive.folder / 'paths-again.psd'
    drive.call('export_psd', document_id=doc, path=str(again), format='psd', mode='layers')
    back = drive.open_psd(again)
    listed = drive.call('list_paths', document_id=back)['paths']
    assert [p['name'] for p in listed] == ['Logo'], listed
    mask = drive.call('get_vector_mask', document_id=back, layer=1)
    assert mask['vector_mask'] is True and mask['has_raster_mask'] is False, mask
    corners = [[a['x'], a['y']] for a in mask['subpaths'][0]['anchors']]
    assert all(abs(a - b) <= 1e-3 for p, q in zip(corners, square['anchors'])
               for a, b in zip(p, q)), corners
    drive.close(doc, back)
    return dict(paths=[p['name'] for p in listed], corners=corners)


def clipping_marks(drive, doc):
    """(name, clipping) for each path list_paths reports."""
    return [(p['name'], p.get('clipping', False))
            for p in drive.call('list_paths', document_id=doc)['paths']]


def clipping(drive):
    doc = drive.open_psd(FIXTURES / 'knockout_bottom' / 'source.psd')
    square = {'closed': True, 'anchors': [[8, 8], [40, 8], [40, 30], [8, 30]]}
    frame = {'closed': True, 'anchors': [[2, 2], [60, 2], [60, 40], [2, 40]]}
    drive.call('set_path', document_id=doc, name='Logo', subpaths=[square])
    drive.call('set_path', document_id=doc, name='Frame', subpaths=[frame])
    drive.call('set_path', document_id=doc, subpaths=[square])
    want = [('Work Path', False), ('Logo', True), ('Frame', False)]
    set_ = drive.call('set_clipping_path', document_id=doc, path='Logo')
    assert set_['changed'] is True and set_['clipping_path'] == 'Logo', set_
    assert clipping_marks(drive, doc) == want, clipping_marks(drive, doc)
    again = drive.call('set_clipping_path', document_id=doc, path='Logo')
    assert again['changed'] is False, again
    refused = drive.call('set_clipping_path', fail=True, document_id=doc, path='Work Path')
    assert 'work path cannot be the clipping path' in refused, refused
    refused = drive.call('set_clipping_path', fail=True, document_id=doc)
    assert 'path' in refused, refused
    drive.call('undo', document_id=doc)
    assert not any(c for _, c in clipping_marks(drive, doc)), 'undo clears it'
    drive.call('redo', document_id=doc)
    assert clipping_marks(drive, doc) == want, 'redo sets it again'
    seen = {}
    psd = drive.folder / 'clipping-again.psd'
    drive.call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    native = drive.folder / 'clipping.doctored'
    drive.call('save_copy', document_id=doc, path=str(native), format='rz')
    tiff = drive.folder / 'clipping.tif'
    saved = drive.call('save_copy', document_id=doc, path=str(tiff), format='tiff')
    assert 'paths' not in saved['flat_save_losses'], saved['flat_save_losses']
    png = drive.call('save_copy', document_id=doc, path=str(drive.folder / 'clipping.png'),
                     format='png')
    assert 'paths' in png['flat_save_losses'], png['flat_save_losses']
    for name, opened in [('psd', drive.open_psd(psd)), ('native', drive.open_native(native)),
                         ('tiff', drive.open_native(tiff))]:
        assert clipping_marks(drive, opened) == want, (name, clipping_marks(drive, opened))
        seen[name] = clipping_marks(drive, opened)
        drive.close(opened)
    cleared = drive.call('set_clipping_path', document_id=doc, path=None)
    assert cleared['changed'] is True and cleared['clipping_path'] is None, cleared
    drive.close(doc)
    # A Grayscale document answers the tool in its closed set.
    gray = drive.call('new_document', width=64, height=48, unit='px', color_mode='gray',
                      depth=8, background='white')['document']['id']
    drive.call('set_path', document_id=gray, name='G', subpaths=[square])
    drive.call('set_clipping_path', document_id=gray, path='G')
    assert clipping_marks(drive, gray) == [('G', True)], clipping_marks(drive, gray)
    drive.close(gray)
    return seen


def modes(drive):
    square = {'closed': True, 'anchors': [[8, 8], [40, 8], [40, 30], [8, 30]]}
    seen = {}
    for mode in ('gray', 'lab', 'cmyk'):
        doc = drive.call('new_document', width=64, height=48, unit='px', color_mode=mode,
                         depth=8, background='white')['document']['id']
        drive.call('new_layer', document_id=doc, name='Top')
        drive.call('set_vector_mask', document_id=doc, layer=1, subpaths=[square])
        if mode != 'cmyk':  # a CMYK document offers no Layer Comps tools
            drive.call('save_layer_comp', document_id=doc, name='Both')
        report = drive.call('inspect_psd_export', document_id=doc, format='psd', mode='layers')
        assert not any(l.startswith('The vector mask on') for l in report['losses']), report
        path = drive.folder / f'modes-{mode}.psd'
        drive.call('export_psd', document_id=doc, path=str(path), format='psd', mode='layers')
        back = drive.open_psd(path)
        rows = drive.layers(back)
        assert rows[0].get('background') == 'ordinary', (mode, rows[0])
        mask = drive.call('get_vector_mask', document_id=back, layer=1)
        assert mask['vector_mask'] is True and mask['has_raster_mask'] is False, (mode, mask)
        corners = [[a['x'], a['y']] for a in mask['subpaths'][0]['anchors']]
        assert all(abs(a - b) <= 1e-3 for p, q in zip(corners, square['anchors'])
                   for a, b in zip(p, q)), (mode, corners)
        names = None
        if mode != 'cmyk':
            names = [c['name'] for c in drive.call('list_layer_comps', document_id=back)['comps']]
            assert names == ['Both'], (mode, names)
        drive.close(doc, back)
        seen[mode] = dict(background=rows[0].get('background'), corners=corners, comps=names)
    return seen


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', required=True, help='a fresh folder')
    arguments = parser.parse_args()
    output = Path(arguments.output).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=False)
    os.environ.setdefault('DR_BACKGROUND', '1')
    sys.path.insert(0, str(HERE))
    from phase5_support import App
    results, failed = {}, []
    with App(output / 'app', output / 'recovery', interval=600) as app:
        drive = Drive(app, output)
        for case in (background, comps, paths, clipping, modes):
            name = case.__name__.upper()
            try:
                results[name] = dict(passed=True, detail=case(drive))
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
