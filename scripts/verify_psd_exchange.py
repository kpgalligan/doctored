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
        for case in (background, comps, paths):
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
