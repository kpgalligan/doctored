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
- COMPS: layer_comps' five comps (All, PosOnly, AppOnly, HA, HB) are
  listed, and applying HA then HB gives the take's own visibilities
  (`photoshop.json`, layer_comps[0].steps).
- AUTHORED: the `authored_exchange` family. comps_rgb8's comps list with
  their comments, and applying each in order gives the
  step table's visibility and opacity (`photoshop.json`); paths_rgb8's Va is
  a vector mask with no raster part, Vb one with a raster part, and Vc (its
  subpaths subtract) is a raster mask, no vector mask. comps_rgb8,
  comps_rgb16, paths_rgb8, clip_a and clip_b are each exported with
  export_psd into `<output>/readback/` with an `expected.json` naming them,
  the files a Photoshop readback opens.
- PATHS: a saved path and a vector mask set on knockout_bottom's Mid layer
  survive export_psd and a layered reopen (list_paths, get_vector_mask).
- CLIPPING: set_clipping_path marks one saved path (refusing the work path and
  a call with no path), undo and redo move the mark, and it survives
  export_psd, a .doctored save_copy and a flat TIFF save_copy (whose
  flat_save_losses has no "paths", a PNG's does) with their reopens; path
  null clears it; a Grayscale document answers the tool.
- FLATNESS: list_paths gives the clipping path's clipping_flatness (0 for
  none); set_clipping_path's flatness sets it (0.5, then 3, the same value
  again a no-op, omitted keeps it), refuses 150, a boolean, a word and a
  flatness with no path, and undo and redo move it. A rename and a geometry
  edit keep it; the exported PSD's resource 2999 is the name then
  `00 03 00 00 00 01` (Adobe's layout, parsed here), and the PSD, .doctored
  and TIFF reopens list 3. Moving the mark gives the new path none. The
  authored clip_b.psd lists 0.5, and renamed then exported writes its own
  bytes after the new name.
- MODES: a new Grayscale, Lab and CMYK document with a raster-less vector
  mask on its second layer and a comp exports with no vector-mask loss; its
  layered reopen marks the bottom "ordinary", keeps the vector mask's corners
  and lists the comp.

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
    assert names == ['All', 'PosOnly', 'AppOnly', 'HA', 'HB'], names
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


def psd_resource(path, wanted):
    """Image resource `wanted`'s body in a classic PSD, by Adobe's layout: the
    26-byte header, the colour-mode section, then 8BIM blocks (id, a Pascal
    name padded even, a u32 size, the body padded even)."""
    data = Path(path).read_bytes()
    at = 26
    at += 4 + int.from_bytes(data[at:at + 4], 'big')
    end = at + 4 + int.from_bytes(data[at:at + 4], 'big')
    at += 4
    while at < end:
        assert data[at:at + 4] == b'8BIM', (path, at)
        rid = int.from_bytes(data[at + 4:at + 6], 'big')
        name = data[at + 6]
        at += 6 + (name + 2) // 2 * 2
        size = int.from_bytes(data[at:at + 4], 'big')
        body = data[at + 4:at + 4 + size]
        if rid == wanted:
            return body
        at += 4 + size + size % 2
    return None


def flatness_rows(drive, doc):
    """(name, clipping_flatness) for each path list_paths marks as the clipping path."""
    return [(p['name'], p.get('clipping_flatness'))
            for p in drive.call('list_paths', document_id=doc)['paths'] if p.get('clipping')]


def flatness(drive):
    doc = drive.open_psd(FIXTURES / 'knockout_bottom' / 'source.psd')
    square = {'closed': True, 'anchors': [[8, 8], [40, 8], [40, 30], [8, 30]]}
    frame = {'closed': True, 'anchors': [[2, 2], [60, 2], [60, 40], [2, 40]]}
    drive.call('set_path', document_id=doc, name='Logo', subpaths=[square])
    drive.call('set_path', document_id=doc, name='Frame', subpaths=[frame])
    drive.call('set_clipping_path', document_id=doc, path='Logo')
    assert flatness_rows(drive, doc) == [('Logo', 0)], flatness_rows(drive, doc)
    seen = {}
    half = drive.call('set_clipping_path', document_id=doc, path='Logo', flatness=0.5)
    assert half['changed'] is True and half['flatness'] == 0.5, half
    assert flatness_rows(drive, doc) == [('Logo', 0.5)], flatness_rows(drive, doc)
    same = drive.call('set_clipping_path', document_id=doc, path='Logo', flatness=0.5)
    assert same['changed'] is False and 'with that flatness' in same['note'], same
    three = drive.call('set_clipping_path', document_id=doc, path='Logo', flatness=3)
    assert three['changed'] is True and three['flatness'] == 3, three
    kept = drive.call('set_clipping_path', document_id=doc, path='Logo')
    assert kept['changed'] is False and kept['flatness'] == 3, kept
    for arguments, words in [
        (dict(path='Logo', flatness=150), 'flatness must be 0 (none) or from 0.2 to 100'),
        (dict(path='Logo', flatness=True), 'flatness must be a number, not true'),
        (dict(path='Logo', flatness='steep'), 'flatness must be a number'),
        (dict(path=None, flatness=1), 'flatness goes with a path'),
    ]:
        refused = drive.call('set_clipping_path', fail=True, document_id=doc, **arguments)
        assert words in refused, (arguments, refused)
    assert flatness_rows(drive, doc) == [('Logo', 3)], 'a refusal changes nothing'
    drive.call('undo', document_id=doc)
    assert flatness_rows(drive, doc) == [('Logo', 0.5)], 'undo'
    drive.call('redo', document_id=doc)
    assert flatness_rows(drive, doc) == [('Logo', 3)], 'redo'
    # A rename and a geometry edit hand the list back with the reading in it.
    drive.call('rename_path', document_id=doc, path='Logo', name='Mark')
    drive.call('set_path', document_id=doc, path='Mark', subpaths=[frame])
    assert flatness_rows(drive, doc) == [('Mark', 3)], flatness_rows(drive, doc)
    psd = drive.folder / 'flatness.psd'
    drive.call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    body = psd_resource(psd, 2999)
    assert body == bytes([4]) + b'Mark' + bytes([0, 3, 0, 0, 0, 1]), body
    native = drive.folder / 'flatness.doctored'
    drive.call('save_copy', document_id=doc, path=str(native), format='rz')
    tiff = drive.folder / 'flatness.tif'
    drive.call('save_copy', document_id=doc, path=str(tiff), format='tiff')
    for name, opened in [('psd', drive.open_psd(psd)), ('native', drive.open_native(native)),
                         ('tiff', drive.open_native(tiff))]:
        assert flatness_rows(drive, opened) == [('Mark', 3)], (name, flatness_rows(drive, opened))
        seen[name] = flatness_rows(drive, opened)
        drive.close(opened)
    moved = drive.call('set_clipping_path', document_id=doc, path='Frame')
    assert moved['changed'] is True and moved['flatness'] == 0, moved
    assert flatness_rows(drive, doc) == [('Frame', 0)], flatness_rows(drive, doc)
    drive.close(doc)
    # Photoshop's own 0.5: listed, and a rename writes the file's bytes after the new name.
    authored = drive.open_psd(AUTHORED / 'clip_b.psd')
    assert flatness_rows(drive, authored) == [('Clip5', 0.5)], flatness_rows(drive, authored)
    source = psd_resource(AUTHORED / 'clip_b.psd', 2999)
    drive.call('rename_path', document_id=authored, path='Clip5', name='Renamed')
    out = drive.folder / 'clip_b-renamed.psd'
    drive.call('export_psd', document_id=authored, path=str(out), format='psd', mode='layers')
    written = psd_resource(out, 2999)
    assert written == bytes([7]) + b'Renamed' + source[1 + source[0]:], (source, written)
    drive.close(authored)
    seen['authored_tail'] = written[1 + written[0]:].hex()
    return seen


def modes(drive):
    square = {'closed': True, 'anchors': [[8, 8], [40, 8], [40, 30], [8, 30]]}
    seen = {}
    for mode in ('gray', 'lab', 'cmyk'):
        doc = drive.call('new_document', width=64, height=48, unit='px', color_mode=mode,
                         depth=8, background='white')['document']['id']
        drive.call('new_layer', document_id=doc, name='Top')
        drive.call('set_vector_mask', document_id=doc, layer=1, subpaths=[square])
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
        names = [c['name'] for c in drive.call('list_layer_comps', document_id=back)['comps']]
        assert names == ['Both'], (mode, names)
        drive.close(doc, back)
        seen[mode] = dict(background=rows[0].get('background'), corners=corners, comps=names)
    return seen


AUTHORED = FIXTURES / 'authored_exchange'


def authored(drive):
    take = json.loads((AUTHORED / 'photoshop.json').read_text())
    table = take['comps']['comps_rgb8.psd']
    doc = drive.open_psd(AUTHORED / 'comps_rgb8.psd')
    listed = drive.call('list_layer_comps', document_id=doc)['comps']
    want = [(c['name'], c['comment'] or '') for c in table['comps']]
    assert [(c['name'], c.get('comment') or '') for c in listed] == want, listed
    seen = {}
    for step in table['steps'][1:]:
        drive.call('apply_layer_comp', document_id=doc, name=step['state'])
        rows = {r['name']: r for r in drive.layers(doc)}
        for name, row in step['layers'].items():
            got = rows[name]
            assert got['visible'] == row['visible'], (step['state'], name, got)
            assert round(got['opacity'] * 100) == row['opacity'], (step['state'], name, got)
        seen[step['state']] = {n: [r['visible'], r['opacity']] for n, r in rows.items()}
    drive.close(doc)
    paths = drive.open_psd(AUTHORED / 'paths_rgb8.psd')
    names = [r['name'] for r in drive.layers(paths)]
    for name, vector, raster in [('Va', True, False), ('Vb', True, True), ('Vc', False, None)]:
        mask = drive.call('get_vector_mask', document_id=paths, layer=names.index(name))
        assert mask['vector_mask'] is vector, (name, mask)
        if raster is not None:
            assert mask['has_raster_mask'] is raster, (name, mask)
    drive.close(paths)
    folder = drive.folder / 'readback'
    folder.mkdir()
    files = []
    for source in ('comps_rgb8.psd', 'comps_rgb16.psd', 'paths_rgb8.psd', 'clip_a.psd',
                   'clip_b.psd'):
        opened = drive.open_psd(AUTHORED / source)
        target = folder / f'dr_{source}'
        drive.call('export_psd', document_id=opened, path=str(target), format='psd',
                   mode='layers')
        drive.close(opened)
        files.append(dict(name=Path(source).stem, file=target.name, source=source))
    (folder / 'expected.json').write_text(json.dumps(dict(files=files), indent=2) + '\n')
    return dict(steps=seen, readback=[f['file'] for f in files])


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
        for case in (background, comps, paths, clipping, flatness, modes, authored):
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
