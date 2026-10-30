#!/usr/bin/env python3
"""MCP drive for Synchronize RAW Settings' Masking: `sync_raw_settings`
with `locals: true` REPLACES each target's RAW locals with the source's, in
order, and its recorded `apply_raw_settings` step replays them.

The script owns its app instances: the Debug app (or `DR_VERIFY_APP`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, an
8 GiB working limit and private `DR_*_DIR`s (`verify_v1_lens.App`), the first
with `DR_MODELS_DIR` at the read-only fixture store
`~/devel/doctored-fixtures/models` (the sky model), the second with its own
empty store. It terminates only those processes. An absent fixture FAILS the
run. Every document is opened from its own copy of its file, at 16 bits.

Fixtures (built here, `verify_v1_raw_subject.mosaicked_dng`'s mosaic):
composite pictures of `~/devel/doctored-fixtures/phase6/photos/sky.jpg`
above `phase6/photos/portrait.png`, each part scaled by `sips -z`:
- SOURCE 640 x 1066 (sky 640 x 426 above the portrait at 640 x 640);
- TARGET 1024 x 1194 (sky 1024 x 682 above two portraits at 512 x 512, side
  by side).
Their width, height and aspect ratio all differ.

Source locals, each with its own tone key: r1 radial, a camera local with
custom white balance (7000 K, tint 10), feather 48, contrast; l1 linear,
whites; b1 an empty mask brushed with two soft dabs, blacks; g1 a full mask
with a luminance range, saturation; c1 a full mask with a colour range by
sample point, texture; s1 subject, shadows; p1 people, highlights; k1 sky,
exposure. The target starts with one local of its own (t0, a radial).

Bounds, fixed before the first run and never widened:
- FEATHER: each synced local's feather equals
  min(256, feather_source * sqrt((W_t / W_s) * (H_t / H_s))) within 1e-6;
- COVERAGE: on a 32 x 20 normalised lattice each drawn local's target
  coverage at a target pixel matches the source coverage bilinearly
  interpolated at the same normalised position within 2 % of full scale
  (1310.7 codes);
- every other control (name, enabled, density, white balance, temperature,
  tint, range with its Lab, tone, generated) equals the source's exactly.

Cases:
- SYNC: `locals: true`: order, ids and controls as above, t0 gone, FEATHER,
  COVERAGE; each generated local's `mask.sha256` equals a fresh `create`
  `{op: kind}` on the synced target (then undone); one `undo` restores the
  target's locals; the source's locals and RAW source are unchanged
  throughout; `redo` brings the sync back.
- REPEAT: the same sync again answers `changed_documents: 0` and the target's
  `list_history` `undo_count` is unchanged; a sync whose only target is the
  source layer leaves the source's locals, RAW source and `undo_count`
  unchanged.
- FIELDS+LOCALS: after `apply_raw_settings {exposure: 0.4}` on the target,
  `fields: [exposure]` with `locals: true` sets the target's exposure to the
  source's and its locals to the source's; two `undo`s restore the synced
  state.
- ACTIONS: from the target's own state (one `undo`), record the sync: the
  step is `apply_raw_settings` with eight `locals`, five `mask_paths` that
  exist in the instance's `DR_RAW_LOCAL_MASKS_DIR`, `source_size` [640, 1066]
  and no `raw`; save it, `undo`, `run_action`: every drawn local's mask
  `sha256` equals the live sync's.
- NO SKY MODEL: a second instance with no model store opens the source saved
  as `.rz` and a fresh target: the sync skips only k1; `result.skipped` is one
  line naming the local and `the sky model is not installed; download it in
  Settings ▸ Models`, and `result.note` names it too.
- ALERTS: neither instance's log holds an `[AutomationAlerts]` line.

Usage: python3 scripts/verify_v1_raw_locals_sync.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`; exits 1 on any failure.
"""
import argparse
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from verify_v1_raw_subject import (  # noqa: E402
    BUSY, LIMIT, MODEL_STORE, NO_SKY_MODEL, PORTRAIT, SKY, App, Mask, SlowClient, bmp_rgb,
    models_at, mosaicked_dng)

LATTICE = (32, 20)
COVERAGE = 0.02 * 65535
FEATHER = 1e-6
SOURCE_PARTS = (640, 426, 640, 1)   # width, sky height, portrait side, portraits
TARGET_PARTS = (1024, 682, 512, 2)
DRAWN = ['r1', 'l1', 'b1', 'g1', 'c1']
GENERATED = {'s1': 'subject', 'p1': 'people', 'k1': 'sky'}


# ------------------------------------------------------------- fixtures --

def write_bmp(path, rows):
    """A 24-bit bottom-up BMP of top-first rows of (r, g, b)."""
    height, width = len(rows), len(rows[0])
    stride = (24 * width + 31) // 32 * 4
    body = bytearray()
    for row in reversed(rows):
        line = bytearray()
        for r, g, b in row:
            line += bytes((b, g, r))
        body += line + b'\0' * (stride - len(line))
    header = struct.pack('<2sIHHI', b'BM', 54 + len(body), 0, 0, 54)
    info = struct.pack('<IiiHHIIiiII', 40, width, height, 1, 24, 0, len(body), 2835, 2835, 0, 0)
    path.write_bytes(header + info + bytes(body))


def scaled(folder, picture, tag, width, height):
    bmp = folder / f'{tag}.bmp'
    subprocess.run(['sips', '-z', str(height), str(width), '-s', 'format', 'bmp', str(picture),
                    '--out', str(bmp)], check=True, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL)
    w, h, rows = bmp_rgb(bmp)
    assert (w, h) == (width, height), (tag, w, h)
    return rows


def composite_dng(folder, name, parts):
    width, sky_height, side, count = parts
    assert side * count == width, parts
    rows = scaled(folder, SKY, f'{name}-sky', width, sky_height)
    portrait = scaled(folder, PORTRAIT, f'{name}-portrait', side, side)
    rows += [row * count for row in portrait]
    composite = folder / f'{name}-composite.bmp'
    write_bmp(composite, rows)
    return mosaicked_dng(folder, composite, name, (width, sky_height + side))


def source_operations(width, height):
    def at(fx, fy):
        return [fx * width, fy * height]
    return [
        dict(op='create', id='r1', name='Radial camera', mask=dict(
            op='radial', center=at(0.3, 0.4), radius=[0.2 * width, 0.15 * height], hardness=0.4),
            feather=48, white_balance='custom', temperature=7000, tint=10, tone=dict(contrast=20)),
        dict(op='create', id='l1', name='Linear', mask={
            'op': 'linear', 'from': at(0.1, 0.3), 'to': at(0.9, 0.7)},
            feather=6, density=0.8, tone=dict(whites=15)),
        dict(op='create', id='b1', name='Brush', mask=dict(op='empty'), feather=12,
             tone=dict(blacks=-10)),
        dict(op='mask', id='b1', command=dict(op='brush', mode='add', dabs=[
            dict(center=at(0.7, 0.3), radius=90, hardness=0, opacity=1),
            dict(center=at(0.6, 0.5), radius=70, hardness=0.2, opacity=0.7)])),
        dict(op='create', id='g1', name='Luminance', mask=dict(op='full'), range=dict(
            luminance=dict(low=0.3, high=0.7, low_falloff=0.1, high_falloff=0.1)),
            tone=dict(saturation=10)),
        dict(op='create', id='c1', name='Colour', mask=dict(op='full'), range=dict(
            color=dict(sample=at(0.5, 0.08), amount=30, falloff=20)), tone=dict(texture=15)),
        dict(op='create', id='s1', name='Subject', mask=dict(op='subject'), tone=dict(shadows=20)),
        dict(op='create', id='p1', name='People', mask=dict(op='people'), tone=dict(highlights=20)),
        dict(op='create', id='k1', name='Sky', mask=dict(op='sky'), tone=dict(exposure=-0.3)),
    ]


# --------------------------------------------------------------- plumbing --

class Run:
    def __init__(self, app, folder):
        self.app = app
        self.folder = Path(folder)
        self.serial = 0

    def waiting(self, tool, **arguments):
        # A reply can return while the document's own background step still
        # runs; the next call then answers BUSY. Wait it out, as
        # verify_v1_raw_subject does.
        deadline = time.monotonic() + 60
        while True:
            try:
                return self.app.call(tool, **arguments)
            except AssertionError as error:
                if BUSY not in str(error) or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)

    def open(self, path, tag):
        self.serial += 1
        copy = self.folder / f'{self.serial:02d}-{tag}{path.suffix}'
        shutil.copyfile(path, copy)
        arguments = dict(path=str(copy))
        if path.suffix != '.rz':
            arguments['precision'] = 'preserve'
        document = self.waiting('open_document', **arguments)['document']
        return document['id'], (document['width'], document['height'])

    def locals(self, document):
        return self.waiting('get_raw_locals', document_id=document, layer=0)

    def source(self, document):
        return self.waiting('get_raw_source', document_id=document, layer=0)['source']

    def undo_count(self, document):
        return self.waiting('list_history', document_id=document)['undo_count']

    def mask(self, document, identifier):
        self.serial += 1
        path = self.folder / f'{self.serial:02d}-{identifier}.drmask'
        self.waiting('get_raw_local_mask', document_id=document, layer=0, id=identifier,
                     path=str(path))
        return Mask(path.read_bytes())

    def edit(self, document, *operations):
        reply = self.waiting('edit_raw_development', document_id=document, layer=0,
                             operations=list(operations))
        assert reply['ok'] is True, reply
        return reply

    def sync(self, source, targets, **arguments):
        reply = self.waiting('sync_raw_settings', document_id=source, layer=0,
                             targets=[dict(document_id=t, layer=0) for t in targets], **arguments)
        assert reply['ok'] is True, reply
        return reply['result']


def bilinear(mask, fx, fy):
    """Coverage at normalised (fx, fy) between pixel centres, edge-clamped."""
    x = min(max(fx * mask.width - 0.5, 0), mask.width - 1)
    y = min(max(fy * mask.height - 0.5, 0), mask.height - 1)
    x0, y0 = int(x), int(y)
    x1, y1 = min(x0 + 1, mask.width - 1), min(y0 + 1, mask.height - 1)
    tx, ty = x - x0, y - y0
    top = mask.at(x0, y0) * (1 - tx) + mask.at(x1, y0) * tx
    bottom = mask.at(x0, y1) * (1 - tx) + mask.at(x1, y1) * tx
    return top * (1 - ty) + bottom * ty


def without(node, *keys):
    return {k: v for k, v in node.items() if k not in keys}


class Shared:
    def __init__(self, run):
        self.run = run
        self.source = self.target = None
        self.source_size = self.target_size = None
        self.source_locals = self.source_raw = None
        self.target_before = None
        self.synced = None


def setup(shared):
    run = shared.run
    source_dng = composite_dng(run.folder, 'source', SOURCE_PARTS)
    target_dng = composite_dng(run.folder, 'target', TARGET_PARTS)
    shared.source_dng, shared.target_dng = source_dng, target_dng
    shared.source, shared.source_size = run.open(source_dng, 'source')
    shared.target, shared.target_size = run.open(target_dng, 'target')
    width, height = shared.source_size
    run.edit(shared.source, *source_operations(width, height))
    tw, th = shared.target_size
    run.edit(shared.target, dict(op='create', id='t0', name='Own', exposure_offset=0.5, mask=dict(
        op='radial', center=[tw / 2, th / 2], radius=[50, 50], hardness=0.5)))
    shared.source_locals = run.locals(shared.source)
    shared.source_raw = run.source(shared.source)
    shared.target_before = run.locals(shared.target)
    ids = [node['id'] for node in shared.source_locals['nodes']]
    assert ids == DRAWN + list(GENERATED), ids
    for identifier, kind in GENERATED.items():
        node = next(n for n in shared.source_locals['nodes'] if n['id'] == identifier)
        assert node.get('generated') == kind, node
    return dict(source=list(shared.source_size), target=list(shared.target_size), ids=ids)


def check_synced(shared, listing, *, check_coverage):
    """Order, ids, controls and feather of a synced target listing; COVERAGE when asked."""
    run = shared.run
    source_nodes = shared.source_locals['nodes']
    nodes = listing['nodes']
    assert [n['id'] for n in nodes] == [n['id'] for n in source_nodes], [n['id'] for n in nodes]
    (sw, sh), (tw, th) = shared.source_size, shared.target_size
    assert (listing['source_width'], listing['source_height']) == (tw, th), listing
    scale = math.sqrt((tw / sw) * (th / sh))
    feathers = {}
    for node, original in zip(nodes, source_nodes):
        assert without(node, 'mask', 'feather') == without(original, 'mask', 'feather'), (
            node['id'], node, original)
        expected = min(256, original['feather'] * scale)
        assert abs(node['feather'] - expected) <= FEATHER, (node['id'], node['feather'], expected)
        assert (node['mask']['width'], node['mask']['height']) == (tw, th), node['mask']
        feathers[node['id']] = node['feather']
    worst = {}
    if check_coverage:
        columns, rows = LATTICE
        for identifier in DRAWN:
            original = run.mask(shared.source, identifier)
            synced = run.mask(shared.target, identifier)
            off = 0.0
            for j in range(rows):
                for i in range(columns):
                    x = int((i + 0.5) / columns * tw)
                    y = int((j + 0.5) / rows * th)
                    expected = bilinear(original, (x + 0.5) / tw, (y + 0.5) / th)
                    off = max(off, abs(synced.at(x, y) - expected))
            assert off <= COVERAGE, (identifier, off)
            worst[identifier] = round(off, 2)
    return dict(feathers=feathers, coverage_worst=worst)


def source_unchanged(shared):
    run = shared.run
    assert run.locals(shared.source) == shared.source_locals, 'the source locals changed'
    assert run.source(shared.source) == shared.source_raw, 'the source RAW changed'


# ----------------------------------------------------------------- cases --

def sync_case(shared):
    run = shared.run
    result = run.sync(shared.source, [shared.target], locals=True)
    assert result['changed_documents'] == 1 and result['skipped'] == [], result
    assert 'note' not in result, result
    listing = run.locals(shared.target)
    assert 't0' not in [n['id'] for n in listing['nodes']]
    detail = check_synced(shared, listing, check_coverage=True)
    source_unchanged(shared)
    shared.synced = listing
    hashes = {n['id']: n['mask']['sha256'] for n in listing['nodes']}
    run.edit(shared.target, *[dict(op='create', id=f'fresh-{kind}', mask=dict(op=kind))
                              for kind in GENERATED.values()])
    fresh = {n['id']: n['mask']['sha256'] for n in run.locals(shared.target)['nodes']}
    for identifier, kind in GENERATED.items():
        assert fresh[f'fresh-{kind}'] == hashes[identifier], (identifier, kind)
    run.waiting('undo', document_id=shared.target)
    assert run.locals(shared.target) == listing, 'undo of the fresh creates'
    run.waiting('undo', document_id=shared.target)
    assert run.locals(shared.target) == shared.target_before, 'one undo did not restore the target'
    source_unchanged(shared)
    run.waiting('redo', document_id=shared.target)
    assert run.locals(shared.target) == listing, 'redo did not bring the sync back'
    detail['hashes'] = hashes
    return detail


def repeat_case(shared):
    run = shared.run
    depth = run.undo_count(shared.target)
    result = run.sync(shared.source, [shared.target], locals=True)
    assert result['changed_documents'] == 0, result
    assert run.undo_count(shared.target) == depth, 'a repeated sync added an undo step'
    assert run.locals(shared.target) == shared.synced, 'a repeated sync changed the target'
    source_depth = run.undo_count(shared.source)
    itself = run.sync(shared.source, [shared.source], locals=True)
    assert itself['changed_documents'] == 0, itself
    assert run.undo_count(shared.source) == source_depth, 'syncing the source added an undo step'
    source_unchanged(shared)
    return dict(target_undo_count=depth, source_undo_count=source_depth)


def fields_case(shared):
    run = shared.run
    reply = run.waiting('apply_raw_settings', document_id=shared.target, layer=0,
                        raw=dict(exposure=0.4))
    assert reply['ok'] is True, reply
    exposure = shared.source_raw['raw']['recipe']['values']['exposure']
    assert abs(exposure - 0.4) > 1e-3, exposure
    result = run.sync(shared.source, [shared.target], locals=True, fields=['exposure'])
    assert result['changed_documents'] == 1, result
    target_exposure = run.source(shared.target)['raw']['recipe']['values']['exposure']
    assert abs(target_exposure - exposure) < 1e-6, (target_exposure, exposure)
    detail = check_synced(shared, run.locals(shared.target), check_coverage=False)
    run.waiting('undo', document_id=shared.target)
    run.waiting('undo', document_id=shared.target)
    assert run.locals(shared.target) == shared.synced, 'two undos did not restore the synced state'
    source_unchanged(shared)
    return dict(exposure=exposure, **detail)


def actions_case(shared, store):
    run = shared.run
    run.waiting('undo', document_id=shared.target)
    assert run.locals(shared.target) == shared.target_before
    run.waiting('start_recording', discard=True)
    result = run.sync(shared.source, [shared.target], locals=True)
    assert result['changed_documents'] == 1, result
    live = {n['id']: n['mask']['sha256'] for n in run.locals(shared.target)['nodes']}
    steps = run.waiting('stop_recording')['steps']
    applied = [s for s in steps if s['tool'] == 'apply_raw_settings']
    assert len(applied) == 1, steps
    arguments = applied[0]['arguments']
    assert 'raw' not in arguments, arguments.keys()
    assert len(arguments['locals']) == len(DRAWN) + len(GENERATED), arguments['locals']
    assert arguments['source_size'] == list(shared.source_size), arguments['source_size']
    paths = [Path(p) for p in arguments['mask_paths']]
    assert len(paths) == len(DRAWN), paths
    for path in paths:
        assert path.exists() and path.parent.resolve() == store.resolve(), path
    run.waiting('save_action', name='v1 locals sync', steps=steps)
    run.waiting('undo', document_id=shared.target)
    assert run.locals(shared.target) == shared.target_before
    played = run.waiting('run_action', name='v1 locals sync', document_id=shared.target)
    assert played.get('ok', True) is not False, played
    replayed = {n['id']: n['mask']['sha256'] for n in run.locals(shared.target)['nodes']}
    for identifier in DRAWN:
        assert replayed[identifier] == live[identifier], identifier
    source_unchanged(shared)
    path = run.folder / 'source.rz'
    run.waiting('save_copy', document_id=shared.source, path=str(path))
    shared.source_rz = path
    return dict(mask_paths=[str(p) for p in paths],
                generated_equal={i: replayed[i] == live[i] for i in GENERATED})


def no_sky_case(run, shared):
    source, _ = run.open(shared.source_rz, 'source')
    target, _ = run.open(shared.target_dng, 'target')
    result = run.sync(source, [target], locals=True)
    assert result['changed_documents'] == 1, result
    assert len(result['skipped']) == 1, result
    line = result['skipped'][0]
    assert 'Sky' in line and NO_SKY_MODEL in line, line
    assert NO_SKY_MODEL in result.get('note', ''), result
    ids = [n['id'] for n in run.locals(target)['nodes']]
    assert ids == DRAWN + ['s1', 'p1'], ids
    return dict(skipped=result['skipped'], note=result['note'])


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instances, the copies and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-raw-locals-sync-'))
    output.mkdir(parents=True, exist_ok=True)

    results, failed = {}, []

    def run_case(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
            return True
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)
            return False

    missing = [path for path in (PORTRAIT, SKY, MODEL_STORE) if not path.exists()]
    if missing:
        results['fixture'] = dict(passed=False, error=f'missing {missing}')
        failed.append('fixture')
        print(f'FAIL fixture: missing {missing}', flush=True)
    else:
        shared = None
        with models_at(MODEL_STORE), App(output / 'instance', LIMIT, SlowClient) as app:
            shared = Shared(Run(app, output))
            if run_case('setup', lambda: setup(shared)):
                run_case('sync', lambda: sync_case(shared))
                run_case('repeat', lambda: repeat_case(shared))
                run_case('fields+locals', lambda: fields_case(shared))
                run_case('actions', lambda: actions_case(
                    shared, output / 'instance' / 'raw-local-masks'))
        if getattr(shared, 'source_rz', None) is not None:
            with App(output / 'instance-no-models', LIMIT, SlowClient) as app:
                run_case('no sky model', lambda: no_sky_case(Run(app, output), shared))
        else:
            results['no sky model'] = dict(passed=False, error='no saved source from actions')
            failed.append('no sky model')
            print('FAIL no sky model: no saved source', flush=True)

        def alerts():
            lines = [line for log in ('instance/app.log', 'instance-no-models/app.log')
                     if (output / log).exists()
                     for line in (output / log).read_text(errors='replace').splitlines()
                     if '[AutomationAlerts]' in line]
            assert not lines, lines[:5]
            return dict(lines=0)

        run_case('alerts', alerts)

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
