#!/usr/bin/env python3
"""The MCP drive for Edit ▸ Sky Replacement… (`sky_replacement`) and its sky list
(`list_skies`) on the Debug app with the real sky model and the sky library from the shared
fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by
`scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release`
(NETWORK for the other models) and only read here; without a ready sky model and sky library
there the drive prints `SKIP sky replacement: …` and exits 0. The photo is the phase-6 fixture
sky.jpg (CC0, 2048 × 1366); every other image is generated here. Each instance is owned by this
file (`phase5_support.App` on its own scratch MCP socket, a dummy API key and a private folder
for every preference library); DR_SKIES_DIR is a scratch folder into which one user sky is
written BEFORE launch, and DR_MODELS_DIR is the fixture store, or for case N a scratch store
holding a symlink to the sky model's folder and a copy of `acceptances.json` (no library).

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

- **L1** `list_skies`: the library is `ready` at version sky-library-1 with 24 skies, six in
  each of blue, sunset, dramatic and night, every one CC0-1.0 with a source URL; the pre-written
  user sky is listed with its size.
- **A8 / A16** (8 and 16 bits): `sky_replacement` with the first library sky and output
  duplicate_layer on sky.jpg answers ok, changed, output_layer 1, sky {kind library, id}, the sky model's id, sky_share in (0, 1)
  and planned_bytes > 0; the document then has 2 layers, the new one named "Sky Replacement"
  and active, and its top row differs from the photo's (the sky was replaced).
- **U1** one undo after A8 leaves one layer.
- **G8 / G16** (M2b, the default output): `sky_replacement` with no `output` answers output
  new_layers and output_layer 4; the document has 5 layers: the group "Sky Replacement Group"
  (kind group, Pass Through, depth 0, active) holding "Foreground Color" (adjustment, curves,
  Normal, opacity 0.5), "Foreground Lighting" (adjustment, curves, Multiply, opacity 0.5) and
  "Sky" (raster, Normal, opacity 1), each at depth 1 with a mask.
- **GS** lighting_mode screen: the lighting child is Screen.
- **GR8 / GR16** `save_copy` as .rz, then `open_document` of it: the same five rows.
- **GP8 / GP16** `export_psd` (layers), then `open_document` of it: the same five rows.
- **GU** one undo after G8 leaves one layer.
- **U2** the user sky by name: sky {kind user, name}.
- **P1** `sky_path` (a generated gradient PNG): sky {kind file, path}.
- **D8 / D16** `sky_replacement` with `decontaminate: true` and no `output` answers
  decontaminate true, output new_layers and output_layer 5; the document has 6 rows: the group
  at 5 (active) holding first "Foreground Edge" (kind layer, not an adjustment, Normal, opacity 1,
  depth 1, with a mask), then G's three children.
- **DR8 / DR16** `save_copy` as .rz, then `open_document` of it: the same six rows.
- **DP8 / DP16** `export_psd` (layers), then `open_document` of it: the same six rows.
- **DX8** output duplicate_layer at 8 bits with the first `night` library sky, decontaminate true
  against false on fresh copies of the photo, each new layer rendered at max_side 1024: some
  pixel's Rec.709 luma differs by more than 2 codes, and among those pixels more are darker
  with it on than brighter (the light halo is gone).
- **AR** `save_action` of one `sky_replacement` step (the first sky, output new_layers, layer
  0, no decontaminate, as every step recorded before `decontaminate` existed), then
  `run_action` on a fresh 8-bit photo: G's five rows (no Foreground Edge).
- **R1** CMYK: refused with "sky_replacement is unavailable in a CMYK document".
- **R2** a group layer: "sky replacement needs a raster layer; layer N is not one".
- **R3** an unknown sky: "No sky named “nope”.".
- **R4** both sky and sky_path: "Give exactly one of sky".
- **R5** 32-bit: refused with "sky_replacement is unavailable in a 32-bit float document".
- **N1** a store with the sky model but no library: a library id refuses with
  "the sky library is not installed; download it in Settings ▸ Models", and `list_skies` says
  not_downloaded with no skies.

Usage: python3 scripts/verify_sky_replacement.py [--receipts DIR]. Exit 0 only when every gate
passes.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import model_store_paths
from phase5_support import App
from verify_phase1 import png
from verify_phase2 import read_png

# ---- the expectations, fixed here BEFORE anything is measured -------------
STORE = Path.home() / 'devel/doctored-fixtures/models'
FIXTURES = Path(os.environ.get('DR_JOBS_PROBE_FIXTURES', str(Path.home() / 'devel/doctored-fixtures')))
PHOTO = FIXTURES / 'phase6/photos/sky.jpg'
SKY_MODEL = 'twistlabs/fotometis-sky'
LIBRARY = 'doctored/sky-library'
VERSION = 'sky-library-1'
CATEGORIES = ['blue', 'sunset', 'dramatic', 'night']
USER_SKY = 'Drive Gradient'
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
NOT_RASTER = 'sky replacement needs a raster layer; layer {} is not one'
NOT_INSTALLED = 'the sky library is not installed; download it in Settings ▸ Models'


def ready(store, model):
    return model_store_paths.is_ready(store, model)


def gradient(path, width, height):
    pixels = bytes(v for y in range(height) for x in range(width)
                   for v in (40 + 150 * x // width, 80 + 120 * y // height, 220, 255))
    png(path, width, height, pixels)


def write_user_sky(skies):
    """One user sky, as the dialog's Add Sky from File… writes it."""
    images = skies / 'Images'
    images.mkdir(parents=True)
    source = skies.parent / 'user-sky.png'
    gradient(source, 320, 120)
    data = source.read_bytes()
    image = hashlib.sha256(data).hexdigest() + '.png'
    (images / image).write_bytes(data)
    entry = dict(schema_version=1, name=USER_SKY, image=image, width=320, height=120)
    name = hashlib.sha256(USER_SKY.lower().encode()).hexdigest() + '.json'
    (skies / name).write_text(json.dumps(entry, sort_keys=True))


def top_row(app, doc, layer):
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': doc, 'max_side': 256, 'layer': layer}})
    assert not response.get('isError'), response
    width, _, pixels = read_png(base64.b64decode(
        next(i['data'] for i in response['content'] if i['type'] == 'image')))
    return pixels[:width]


def layers(app, doc):
    return app.call('get_document', document_id=doc).get('layers') or []


def open_at(app, folder, name, depth):
    path = folder / f'{name}.jpg'
    path.write_bytes(PHOTO.read_bytes())
    doc = app.call('open_document', path=str(path))['document']['id']
    if depth in (16, 32):
        app.call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
    return doc


GROUP = 'Sky Replacement Group'
CHILDREN = ['Foreground Color', 'Foreground Lighting', 'Sky']


EDGE = 'Foreground Edge'


def group_rows(rows, mode='Multiply', edge=False):
    """The facts G (or, with `edge`, D) checks, and whether they hold, for a document's
    `get_document` rows."""
    facts = [dict(name=r.get('name'), kind=r.get('kind'), depth=r.get('depth'),
                  blend=r.get('blend_mode'), opacity=r.get('opacity'), mask=r.get('has_mask'),
                  adjustment=r.get('is_adjustment'), op=(r.get('adjustment') or {}).get('op'))
             for r in rows]
    first = 2 if edge else 1
    if len(facts) != first + 4:
        return facts, False
    group, children = facts[first + 3], facts[first:first + 3]
    if edge:
        e = facts[1]
        if not (e['name'] == EDGE and e['kind'] == 'layer' and e['adjustment'] is False
                and e['blend'] == 'Normal' and e['opacity'] == 1 and e['depth'] == 1
                and e['mask'] is True):
            return facts, False
    ok = (group['name'] == GROUP and group['kind'] == 'group' and group['blend'] == 'Pass Through'
          and group['depth'] == 0 and group['opacity'] == 1
          and [c['name'] for c in children] == CHILDREN
          and all(c['depth'] == 1 and c['mask'] is True for c in children)
          and all(c['adjustment'] is True and c['op'] == 'curves' for c in children[:2])
          and children[2]['adjustment'] is False
          and [c['blend'] for c in children] == ['Normal', mode, 'Normal']
          and [c['opacity'] for c in children] == [0.5, 0.5, 1])
    return facts, ok


def group_cases(app, folder, receipts, gates, first, depth):
    """G, GS, GR, GP and GU (module doc) at `depth`."""
    doc = open_at(app, folder, f'sky-group-{depth}', depth)
    reply = app.call('sky_replacement', document_id=doc, layer=0, sky=first)
    facts, ok = group_rows(layers(app, doc))
    active = app.call('get_document', document_id=doc).get('active_layer')
    receipts[f'G{depth}'] = dict(reply=reply, rows=facts, active=active)
    gates[f'G{depth}'] = (ok and reply.get('ok') is True and reply.get('changed') is True
                          and reply.get('output') == 'new_layers' and reply.get('output_layer') == 4
                          and active == 4)
    native = folder / f'group-{depth}.rz'
    app.call('save_copy', document_id=doc, path=str(native), format='rz')
    reopened = app.call('open_document', path=str(native))['document']['id']
    facts, ok = group_rows(layers(app, reopened))
    receipts[f'GR{depth}'] = facts
    gates[f'GR{depth}'] = ok
    psd = folder / f'group-{depth}.psd'
    app.call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = app.call('inspect_psd', path=str(psd))
    reopened = app.call('open_document', path=str(psd), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
    facts, ok = group_rows(layers(app, reopened))
    receipts[f'GP{depth}'] = facts
    gates[f'GP{depth}'] = ok
    if depth == 8:
        app.call('undo', document_id=doc)
        receipts['GU'] = len(layers(app, doc))
        gates['GU'] = receipts['GU'] == 1
        screen = open_at(app, folder, 'sky-group-screen', depth)
        reply = app.call('sky_replacement', document_id=screen, layer=0, sky=first,
                         lighting_mode='screen')
        facts, ok = group_rows(layers(app, screen), mode='Screen')
        receipts['GS'] = dict(reply=reply, rows=facts)
        gates['GS'] = ok and reply.get('changed') is True


def luma(p):
    return 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]


def layer_pixels(app, doc, layer):
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': doc, 'max_side': 1024, 'layer': layer}})
    assert not response.get('isError'), response
    _, _, pixels = read_png(base64.b64decode(
        next(i['data'] for i in response['content'] if i['type'] == 'image')))
    return pixels


def decontaminate_cases(app, folder, receipts, gates, first, night, depth):
    """D, DR and DP (module doc) at `depth`; DX and AR at 8 bits."""
    doc = open_at(app, folder, f'sky-edge-{depth}', depth)
    reply = app.call('sky_replacement', document_id=doc, layer=0, sky=first, decontaminate=True)
    facts, ok = group_rows(layers(app, doc), edge=True)
    active = app.call('get_document', document_id=doc).get('active_layer')
    receipts[f'D{depth}'] = dict(reply=reply, rows=facts, active=active)
    gates[f'D{depth}'] = (ok and reply.get('ok') is True and reply.get('changed') is True
                          and reply.get('decontaminate') is True
                          and reply.get('output') == 'new_layers' and reply.get('output_layer') == 5
                          and active == 5)
    native = folder / f'edge-{depth}.rz'
    app.call('save_copy', document_id=doc, path=str(native), format='rz')
    reopened = app.call('open_document', path=str(native))['document']['id']
    facts, ok = group_rows(layers(app, reopened), edge=True)
    receipts[f'DR{depth}'] = facts
    gates[f'DR{depth}'] = ok
    psd = folder / f'edge-{depth}.psd'
    app.call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = app.call('inspect_psd', path=str(psd))
    reopened = app.call('open_document', path=str(psd), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
    facts, ok = group_rows(layers(app, reopened), edge=True)
    receipts[f'DP{depth}'] = facts
    gates[f'DP{depth}'] = ok
    if depth != 8:
        return
    shots = {}
    for flag in (True, False):
        flat = open_at(app, folder, f'sky-dx-{flag}', 8)
        reply = app.call('sky_replacement', document_id=flat, layer=0, sky=night,
                         output='duplicate_layer', decontaminate=flag)
        shots[flag] = (reply, layer_pixels(app, flat, 1))
    on, off = shots[True][1], shots[False][1]
    deltas = [luma(a) - luma(b) for a, b in zip(on, off)]
    darker = sum(1 for d in deltas if d < -2)
    brighter = sum(1 for d in deltas if d > 2)
    receipts['DX8'] = dict(night=night, replies=[shots[True][0], shots[False][0]], pixels=len(on),
                           same_size=len(on) == len(off), darker=darker, brighter=brighter,
                           largest=max((abs(d) for d in deltas), default=0))
    gates['DX8'] = (len(on) == len(off) and shots[True][0].get('decontaminate') is True
                    and shots[False][0].get('decontaminate') is False
                    and darker + brighter > 0 and darker > brighter)
    app.call('save_action', name='sky-replay', overwrite=True,
             steps=[{'tool': 'sky_replacement',
                     'arguments': {'layer': 0, 'sky': first, 'output': 'new_layers'}}])
    fresh = open_at(app, folder, 'sky-action', 8)
    replayed = app.call('run_action', document_id=fresh, name='sky-replay')
    facts, ok = group_rows(layers(app, fresh))
    receipts['AR'] = dict(reply=replayed, rows=facts)
    gates['AR'] = ok


def replace_cases(app, folder, receipts, gates):
    listed = app.call('list_skies')
    skies = listed.get('skies') or []
    counts = {c: sum(1 for s in skies if s.get('category') == c) for c in CATEGORIES}
    users = listed.get('user_skies') or []
    receipts['L1'] = dict(library=listed.get('library'), counts=counts, user_skies=users)
    gates['L1'] = (listed.get('library') == {'state': 'ready', 'version': VERSION} and len(skies) == 24
                   and all(n == 6 for n in counts.values())
                   and all(s.get('license') == 'CC0-1.0' and s.get('source_url') for s in skies)
                   and {'name': USER_SKY, 'width': 320, 'height': 120} in users)
    first = skies[0]['id'] if skies else 'missing'
    night = next((s['id'] for s in skies if s.get('category') == 'night'), 'missing')
    for depth in (8, 16):
        doc = open_at(app, folder, f'sky-{depth}', depth)
        before = top_row(app, doc, 0)
        reply = app.call('sky_replacement', document_id=doc, layer=0, sky=first, edge_lighting=30,
                         output='duplicate_layer')
        after = layers(app, doc)
        replaced = top_row(app, doc, 1)
        changed_row = sum(1 for a, b in zip(before, replaced) if a[:3] != b[:3]) / max(1, len(before))
        receipts[f'A{depth}'] = dict(reply=reply, layers=[l.get('name') for l in after], changed_row=changed_row)
        gates[f'A{depth}'] = (reply.get('ok') is True and reply.get('changed') is True
                              and reply.get('output_layer') == 1
                              and reply.get('sky') == {'kind': 'library', 'id': first}
                              and reply.get('model', {}).get('id') == SKY_MODEL
                              and 0 < reply.get('sky_share', 0) < 1 and reply.get('planned_bytes', 0) > 0
                              and len(after) == 2 and after[1].get('name') == 'Sky Replacement'
                              and changed_row > 0.5)
        if depth == 8:
            app.call('undo', document_id=doc)
            receipts['U1'] = len(layers(app, doc))
            gates['U1'] = receipts['U1'] == 1
            reply = app.call('sky_replacement', document_id=doc, sky=USER_SKY)
            receipts['U2'] = reply
            gates['U2'] = reply.get('sky') == {'kind': 'user', 'name': USER_SKY} and reply.get('changed') is True
            path = folder / 'path-sky.png'
            gradient(path, 400, 160)
            reply = app.call('sky_replacement', document_id=doc, layer=0, sky_path=str(path))
            receipts['P1'] = reply
            gates['P1'] = reply.get('sky') == {'kind': 'file', 'path': str(path)} and reply.get('changed') is True
            message = app.call('sky_replacement', fail=True, document_id=doc, layer=0, sky='nope')
            receipts['R3'] = message
            gates['R3'] = 'No sky named “nope”.' in message
            message = app.call('sky_replacement', fail=True, document_id=doc, layer=0, sky=first,
                               sky_path=str(path))
            receipts['R4'] = message
            gates['R4'] = 'Give exactly one of sky' in message
            grouped = app.call('group_layers', document_id=doc, layers=[0])
            index = next((l['index'] for l in layers(app, doc) if l.get('is_group') or l.get('kind') == 'group'),
                         None)
            if index is None:
                index = grouped.get('group_index', grouped.get('index', 0))
            message = app.call('sky_replacement', fail=True, document_id=doc, layer=index, sky=first)
            receipts['R2'] = dict(message=message, group=index, grouped=grouped)
            gates['R2'] = NOT_RASTER.format(index) in message
        group_cases(app, folder, receipts, gates, first, depth)
        decontaminate_cases(app, folder, receipts, gates, first, night, depth)
    cmyk = open_at(app, folder, 'sky-cmyk', 8)
    app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(GENERIC))
    message = app.call('sky_replacement', fail=True, document_id=cmyk, sky=first)
    receipts['R1'] = message
    gates['R1'] = 'sky_replacement is unavailable in a CMYK document' in message
    float_doc = open_at(app, folder, 'sky-32', 32)
    message = app.call('sky_replacement', fail=True, document_id=float_doc, sky=first)
    receipts['R5'] = message
    gates['R5'] = 'sky_replacement is unavailable in a 32-bit float document' in message
    return first


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    missing = [m for m in (SKY_MODEL, LIBRARY) if not ready(STORE, m)]
    if missing:
        print(f'SKIP sky replacement: no ready {", ".join(missing)} in the fixture store {STORE}')
        return 0
    assert PHOTO.is_file(), PHOTO
    output = Path(args.receipts or tempfile.mkdtemp(prefix='sky-replacement-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_sky_replacement.py before any measurement'}
    gates = {}
    skies = folder / 'skies'
    write_user_sky(skies)
    saved = {key: os.environ.get(key) for key in ('DR_MODELS_DIR', 'DR_SKIES_DIR')}
    try:
        os.environ['DR_MODELS_DIR'] = str(STORE)
        os.environ['DR_SKIES_DIR'] = str(skies)
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            first = replace_cases(app, folder, receipts, gates)
        # N: a store with the sky model and no library.
        bare = folder / 'bare-store'
        bare.mkdir()
        (bare / SKY_MODEL.replace('/', '--')).symlink_to(STORE / SKY_MODEL.replace('/', '--'))
        shutil.copy(STORE / 'acceptances.json', bare / 'acceptances.json')
        os.environ['DR_MODELS_DIR'] = str(bare)
        with App(folder / 'bare', folder / 'recovery-bare', interval=600) as app:
            doc = open_at(app, folder, 'sky-bare', 8)
            message = app.call('sky_replacement', fail=True, document_id=doc, sky=first)
            listed = app.call('list_skies')
            receipts['N1'] = dict(message=message, library=listed.get('library'), skies=len(listed.get('skies') or []))
            gates['N1'] = (NOT_INSTALLED in message and listed.get('library', {}).get('state') == 'not_downloaded'
                           and not listed.get('skies'))
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 0 if gates and all(gates.values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
