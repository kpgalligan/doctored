#!/usr/bin/env python3
"""Owned Release-app qualification for source-constrained content-aware fill.

The independent PNG reader checks real native samples, history and persistence.
Photographic exclusion/numerical kernels and actual sheet events have separate
receipts; this script never calls a synthetic ramp a photograph.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/retouch-probe'))
import app_under_test
from support import HostApp, call_for, fixtures, wait_for


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


probe = load('caf_retouch_probe', ROOT / 'scripts/retouch-probe/mcp.py')
_inventory = load('caf_input_inventory', ROOT / 'scripts/verify_phase9d.py')


def dependencies():
    result = _inventory.dependencies()
    result[str(Path(__file__).relative_to(ROOT))] = digest(__file__)
    return result


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fixture(folder, depth, large=False):
    w, h = (512, 512) if large else (64, 48)
    bounds = (120, 120, 270, 270) if large else (27, 19, 7, 7)
    x0, y0, width, height = bounds
    values = []
    for y in range(h):
        for x in range(w):
            v = 12001 + (97*x + 71*y) % 16000
            rgb = (v, v+3001, v+6002)
            if x0 <= x < x0+width and y0 <= y < y0+height:
                rgb = (3001, 3002, 3003)
            if depth == 8:
                rgb = tuple(round(c/257) for c in rgb)
            values.append((*rgb, (1 << depth)-1))
    path = folder / f'{"large" if large else "ramp"}-{depth}.png'
    path.write_bytes(fixtures.png(w, h, depth, values, fixtures.srgb_profile()))
    return path, bounds


def select(call, bounds):
    x, y, width, height = bounds
    call('select_rect', x=x, y=y, width=width, height=height)


def capture(call, folder, name):
    return probe.samples(call, folder, name)


def ready(call, preview):
    return wait_for(lambda: call('preview_content_aware_fill', preview_id=preview),
                    lambda r: r.get('state') != 'running', timeout=60,
                    detail='exact content-aware candidate')


def exact_outside(before, after, bounds):
    assert before[:3] == after[:3]
    x0, y0, width, height = bounds
    for y in range(before[1]):
        for x in range(before[0]):
            if not (x0 <= x < x0+width and y0 <= y < y0+height):
                assert probe.at(before, x, y) == probe.at(after, x, y), (x, y)
    assert before[3][3::4] == after[3][3::4], 'fill changed layer alpha'


def refusal(call, tool, **arguments):
    history = probe.history(call)
    error = call(tool, fail=True, **arguments)
    assert probe.history(call) == history, (tool, error)
    return error


def run_depth(folder, executable, depth):
    folder.mkdir(parents=True)
    image, bounds = fixture(folder, depth)
    groups = []
    mask = dict(version=1, initial='all', operations=[
        dict(op='exclude_rect', x=43, y=4, width=12, height=38),
        dict(op='include_brush', radius=3, points=[[45, 8], [45, 12]])])
    parameters = dict(layer=0, ring=0, seed=19, sample='current', source_mask=mask)
    with probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        catalog = app.client.rpc('tools/list', {})
        tools = {tool['name']: tool for tool in catalog['tools']}
        assert {'content_aware_fill', 'preview_content_aware_fill',
                'discard_content_aware_fill_preview', 'patch_region'} <= tools.keys()
        fields = tools['content_aware_fill']['inputSchema']['properties']
        assert {'sample', 'sample_all_layers', 'source_mask', 'output', 'preview_id'} <= fields.keys()
        (folder/'catalog.json').write_text(json.dumps(catalog, indent=2)+'\n')
        call = probe.opened(app, image, depth, folder/'calls.jsonl')
        select(call, bounds)
        initial = capture(call, folder, 'initial')

        # Omitting new controls preserves every inherited parameter spelling.
        call('content_aware_fill', layer=0, ring=0, seed=19, sample_all_layers=False)
        legacy = capture(call, folder, 'legacy')
        assert legacy != initial
        exact_outside(initial, legacy, bounds)
        call('undo')
        call('content_aware_fill', layer=0, ring=0, seed=19, sample='current',
             sample_all_layers=False, source_mask=dict(version=1, initial='all', operations=[]))
        assert capture(call, folder, 'explicit-defaults') == legacy
        call('undo')
        groups.append('legacy arguments/default source mask remain exact at native depth')

        # The exact candidate is not a document edit; accepting it records once.
        before_history = probe.history(call)
        app.call('start_recording')
        preview = call('preview_content_aware_fill', **parameters)
        token = preview['preview_id']
        review = ready(call, token) if preview.get('state') == 'running' else preview
        assert review['state'] == 'ready', review
        assert probe.history(call) == before_history
        assert capture(call, folder, 'unpublished') == initial
        call('content_aware_fill', preview_id=token)
        repaired = capture(call, folder, 'reviewed-result')
        assert repaired != initial
        exact_outside(initial, repaired, bounds)
        assert probe.history(call)[1] == before_history[1]+1
        recorded = app.call('stop_recording', name=f'Constrained fill {depth}')
        assert [step['tool'] for step in recorded['steps']] == ['content_aware_fill'], recorded
        arguments = recorded['steps'][0]['arguments']
        assert arguments['source_mask'] == mask and 'preview_id' not in arguments, arguments
        refusal(call, 'content_aware_fill', preview_id=token)
        call('undo')
        assert capture(call, folder, 'review-undo') == initial
        call('redo')
        assert capture(call, folder, 'review-redo') == repaired
        call('undo')
        call('run_action', name=f'Constrained fill {depth}')
        assert capture(call, folder, 'action-replayed') == repaired
        call('undo')
        call('content_aware_fill', **parameters)
        assert capture(call, folder, 'direct-equivalent') == repaired
        call('undo')
        groups.append('exact staged candidate, single-use acceptance, one Undo/Redo and canonical Action replay')

        preview = call('preview_content_aware_fill', **parameters)
        token = preview['preview_id']
        select(call, (1, 1, 2, 2))
        refusal(call, 'content_aware_fill', preview_id=token)
        assert capture(call, folder, 'stale-selection') == initial
        select(call, bounds)
        preview = call('preview_content_aware_fill', **parameters)
        token = preview['preview_id']
        call('discard_content_aware_fill_preview', preview_id=token)
        refusal(call, 'content_aware_fill', preview_id=token)
        assert capture(call, folder, 'discarded') == initial
        groups.append('selection changes and explicit discard invalidate candidate without pixel edits')

        for invalid in [
            dict(version=2, initial='all', operations=[]),
            dict(version=1, initial='all', operations=[dict(op='exclude_brush', radius=-1, points=[[3, 4]])]),
            dict(version=1, initial='all', operations=[dict(op='unknown', x=0, y=0)]),
            dict(version=1, initial='none', operations=[]),
        ]:
            refusal(call, 'content_aware_fill', source_mask=invalid, layer=0)
        for target in ('mask', 'red', 'channel:missing'):
            refusal(call, 'content_aware_fill', target=target, layer=0)
        refusal(call, 'content_aware_fill', sample='all', sample_all_layers=False, layer=0)
        assert capture(call, folder, 'refused') == initial
        groups.append('source recipe/version/empty-source, scalar-target and alias refusals are atomic')

        original_layer = call('get_document')['layers'][0]
        call('content_aware_fill', output='new_layer', **parameters)
        layered = capture(call, folder, 'new-layer')
        assert len(call('get_document')['layers']) == 2
        assert call('get_document')['layers'][0] == original_layer
        assert max(abs(a-b) for a, b in zip(layered[3], repaired[3])) <= 2
        call('undo')
        assert capture(call, folder, 'new-layer-undo') == initial
        groups.append('new retouch layer preserves original and matches opaque in-place output')

        call('patch_region', layer=0, offset_x=-12, offset_y=0,
             sample='current', points=[[24,16], [40,16], [40,32], [24,32]])
        patched = capture(call, folder, 'patch')
        assert patched != initial
        exact_outside(initial, patched, (24,16,16,16))
        assert probe.at(patched, 30, 22)[0] > (10000 if depth == 16 else 38)
        call('undo')
        assert capture(call, folder, 'patch-undo') == initial
        call('redo')
        assert capture(call, folder, 'patch-redo') == patched
        call('undo')
        groups.append('native Patch tool, selection confinement and exact Undo/Redo')

        # Native files, snapshots and recovery preserve original artwork as well
        # as the new fill output, rather than merely preserving a flat picture.
        call('add_shape_layer', kind='rect', x=27, y=19, w=7, h=7, fill='#080808')
        artwork = call('get_document')['layers'][1]
        call('content_aware_fill', layer=1, sample='current_and_below',
             output='new_layer', source_mask=mask, seed=19)
        assert call('get_document')['layers'][1] == artwork
        expected = capture(call, folder, 'artwork-fill')
        native = folder/'filled.rz'
        call('save_copy', path=str(native))
        reopened = probe.opened(app, native, depth, folder/'reopened.jsonl')
        assert capture(reopened, folder, 'reopened') == expected
        reopened('edit_shape_layer', layer=1, fill='#090909')
        reopened('undo')
        assert capture(reopened, folder, 'reopened-undo') == expected
        groups.append('native reopen preserves fill samples and original artwork editability')

        snapshot = call('create_snapshot', name='Constrained fill')['snapshot_id']
        path = folder/'filled.drsnapshot'
        call('save_snapshot', snapshot_id=snapshot, path=str(path))
        call('edit_shape_layer', layer=1, fill='#101010')
        loaded = call('load_snapshot', path=str(path))['snapshot_id']
        call('restore_snapshot', snapshot_id=loaded)
        assert capture(call, folder, 'snapshot-restored') == expected
        call('write_recovery', wait=True)
        status = wait_for(lambda: call('get_recovery_status')['document'],
                          lambda r: r.get('state') == 'protected' and not r.get('pending'),
                          timeout=60, detail='content-aware durable recovery')
        recovery_id = status['recovery_id']
        app.crash()
    with probe.TracedApp(folder/'recovery-app', folder/'recovery', executable) as app:
        assert recovery_id in {r['recovery_id'] for r in app.call('get_recovery_status')['available']}
        doc = app.call('recover_document', recovery_id=recovery_id, show=True)['document']['id']
        call = call_for(app, doc, folder/'recovered.jsonl')
        assert capture(call, folder, 'recovered') == expected
        call('edit_shape_layer', layer=1, fill='#111111')
        call('undo')
        assert capture(call, folder, 'recovered-undo') == expected
        groups.append('saved snapshot and owned-process crash recovery retain fill/artwork/Undo')
    return dict(depth=depth, groups=groups, group_count=len(groups), fixture_sha256=digest(image))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
        default=app_under_test.default_app_executable())
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    args = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9e-'))
    before = dependencies()
    report = dict(passed=False, scratch=str(folder), app_sha256=digest(args.app))
    try:
        report['cases'] = [run_depth(folder/f'rgba{depth}', args.app.resolve(), depth)
                           for depth in (8, 16)]
        after = dependencies()
        changes = sorted(key for key in before.keys() | after.keys()
                         if before.get(key) != after.get(key))
        report['source_check'] = dict(files=len(before), changes=changes,
                                     file_set_exact=before.keys() == after.keys())
        if args.require_current:
            assert not changes, changes
        assert digest(args.app) == report['app_sha256']
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['all_owned_apps_exited'] = all(a.process is None or a.process.poll() is not None
                                             for a in HostApp.owners)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True)+'\n')
        print('Content-aware workflow receipt:', args.report, flush=True)


if __name__ == '__main__':
    main()
