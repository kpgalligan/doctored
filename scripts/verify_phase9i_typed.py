#!/usr/bin/env python3
"""Owned production MCP qualification of native u16 Refine Selection.

The typed half of P7-12: on a 16-bit document the basic Smooth/Feather/Shift/
Contrast refinement runs at 65,536 levels, and a mask output is a canvas-sized
native u16 mask over an RGBA16 layer whose samples never change. Coverage,
masks and channels are read back through the independent native reader, never
through the product's own display. The depth receipt is the contrast-0.25 pair
(two inputs that share displayed byte 128 produce two distinct u16 codes that
also share byte 128) — contrast 1 thresholds exactly at the byte-128 boundary
and proves nothing about precision.
"""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('phase9i_typed_existing_support',
                                              ROOT / 'scripts/verify_phase9g.py')
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)
shared, probe, fixtures = g.shared, g.probe, g.fixtures
W, H, N = g.W, g.H, g.N


def dependencies():
    inputs = g.dependencies()
    for path in [Path(__file__).resolve(), ROOT / 'scripts/verify_phase9g.py']:
        inputs[str(path.relative_to(ROOT))] = g.f.digest(path)
    return inputs


def byte(value):
    """The maskBytes()/display projection rule, written independently."""
    return (value + 128) // 257


def threshold(values):
    return tuple(65535 if v >= 32768 else 0 for v in values)


def contrast(values, amount):
    """The pinned f64 order. floor(x+0.5) is Rust's half-away-from-zero for the
    non-negative results here, so Python's banker's rounding never applies."""
    out = []
    for value in values:
        t = min(1.0, max(0.0, (value / 65535 - 0.5) / (1 - amount) + 0.5))
        out.append(math.floor(t * 65535 + 0.5))
    return tuple(out)


def region():
    """A solid core with a soft odd-code rim. A speckle would smooth below the
    contour threshold and make every geometric stage return empty coverage."""
    out = []
    for y in range(H):
        for x in range(W):
            if 6 <= x < 18 and 4 <= y < 12:
                out.append(65535)
            elif 5 <= x < 19 and 3 <= y < 13:
                out.append([32769, 65534, 12345, 257, 1][(x + y) % 5])
            else:
                out.append(0)
    return tuple(out)


def state(call, folder, label):
    """Raw layer-0 PNG16 samples, the layer's mask, and every saved channel."""
    path = folder / f'{label}.rz'
    call('save_copy', path=str(path))
    parsed = probe.read_native(path)['document']
    raw = tuple(probe.png_samples(parsed['fallbacks'][0])['samples'])
    layer = parsed['layers'][0]
    mask = None
    if layer.get('mask'):
        mask = dict(layer['mask'])
        data = mask.pop('data')
        mask['samples'] = struct.unpack('<' + 'H' * (mask['width'] * mask['height']), data)
    channels = [struct.unpack('<' + 'H' * N, entry['data']) for entry in parsed['channels']]
    return raw, mask, channels


def zero_and_contrast(call, values, checks):
    g.select16(call, values)
    result = call('refine_selection')
    assert result['changed'] is False and result['coverage_storage'] == 'u16', result
    assert g.selected(call) == values
    checks.append('zero controls report changed:false and leave every odd code intact')

    result = call('refine_selection', contrast=1)
    assert result['changed'] and result['coverage_storage'] == 'u16', result
    assert g.selected(call) == threshold(values)
    call('undo')
    assert g.selected(call) == values
    checks.append('contrast 1 equals the independent >=32768 threshold; undo restores exactly')

    pair = tuple([32770, 32771][(x + y) % 2] for y in range(H) for x in range(W))
    g.select16(call, pair)
    result = call('refine_selection', contrast=0.25)
    refined = g.selected(call)
    assert result['coverage_storage'] == 'u16', result
    assert refined == contrast(pair, 0.25)
    assert set(refined) == {32771, 32772}, sorted(set(refined))
    # Both inputs and both outputs project to the SAME displayed byte.
    assert {byte(v) for v in (32770, 32771, 32772)} == {128}
    call('undo')
    checks.append('contrast 0.25 separates 32770/32771 into 32771/32772 inside one display byte')

    g.select16(call, values)
    call('refine_selection', feather=2)
    feathered = g.selected(call)
    assert feathered != values
    call('undo')
    call('modify_selection', operation='feather', radius=2)
    assert g.selected(call) == feathered
    call('undo')
    assert g.selected(call) == values
    checks.append('feather 2 equals modify_selection feather 2 exactly')


def actions_replay(app, call, folder, checks):
    """The third entry point: a RECORDED refinement replays to the identical
    u16 coverage. The sheet and MCP are driven above; this is Actions."""
    name = 'Typed refine replay'
    before = call('list_history')['undo_count']
    app.call('start_recording')
    result = call('refine_selection', smooth=1, feather=2)
    assert result['changed'] and result['coverage_storage'] == 'u16', result
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['refine_selection'], action
    assert call('list_history')['undo_count'] == before + 1
    recorded, coverage = call('get_selection'), g.selected(call)
    call('undo')
    assert call('list_history')['undo_count'] == before
    assert call('run_action', name=name)['ok']
    assert call('get_selection') == recorded
    assert g.selected(call) == coverage
    assert call('list_history')['undo_count'] == before + 1
    call('undo')
    assert call('list_history')['undo_count'] == before
    checks.append('a recorded refine_selection replays through run_action to the identical u16 coverage')


def mask_output(app, call, folder, source, checks):
    parameters = dict(smooth=1, feather=1, shift=1, contrast=0.25)
    values = region()
    g.select16(call, values)
    result = call('refine_selection', **parameters)
    assert result['changed'] and result['output'] == 'selection', result
    refined = g.selected(call)
    assert refined != values
    # A degenerate all-zero refinement would satisfy every identity below.
    assert 0 < len([v for v in refined if v]) < len(refined)
    assert len({v for v in refined if 0 < v < 65535}) > 1, sorted(set(refined))
    call('undo')
    assert g.selected(call) == values

    result = call('refine_selection', output='mask', layer=0, **parameters)
    assert result['changed'] and result['output'] == 'mask', result
    assert result['coverage_storage'] == 'u16' and result['layer'] == 0, result
    call('load_selection', **{'from': 'layer_mask'}, layer=0)
    assert g.selected(call) == refined
    checks.append('selection and mask outputs of the same parameters are identical coverage')

    raw, mask, _ = state(call, folder, 'mask-output')
    assert raw == source, 'the RGBA16 layer must be the unchanged input and output'
    assert (mask['width'], mask['height'], mask['x'], mask['y']) == (W, H, 0, 0), mask
    assert mask['samples'] == refined
    assert mask.get('enabled', True) and mask.get('linked', True)
    assert mask.get('density', 1.0) == 1.0 and mask.get('feather', 0.0) == 0.0
    # Tiny alpha codes of the fixture survive the mask write untouched.
    assert 1 in raw[3::4] and 0 in raw[3::4]
    checks.append('saved layer-0 PNG16 samples are byte-identical and the mask data is the refined u16')

    call('transform_mask', layer=0, matrix=[1, 0, 0, 1, 1, 0], filter='nearest')
    moved_raw, moved_mask, _ = state(call, folder, 'mask-moved')
    assert moved_raw == source
    assert moved_mask['samples'] == refined and (moved_mask['x'], moved_mask['y']) == (1, 0), moved_mask
    call('undo')
    checks.append('transform_mask translate (1,0) moves the refined mask and leaves the source exact')

    call('load_selection', **{'from': 'layer_mask'}, layer=0)
    call('save_selection', name='Refined coverage')
    channel_raw, channel_mask, channels = state(call, folder, 'channel')
    assert channels[0] == refined and channel_mask['samples'] == refined
    assert channel_raw == source
    checks.append('save_selection reuses the refined coverage as an exact u16 channel')
    return refined


def persistence(app, call, folder, source, refined, checks):
    snapshot = call('create_snapshot', name='Typed refinement')['snapshot_id']
    archive = folder / 'typed-refinement.drsnapshot'
    call('save_snapshot', snapshot_id=snapshot, path=str(archive))
    call('remove_layer_mask', layer=0)
    call('deselect')
    loaded = call('load_snapshot', path=str(archive))['snapshot_id']
    call('restore_snapshot', snapshot_id=loaded)
    raw, mask, channels = state(call, folder, 'restored')
    assert raw == source and mask['samples'] == refined and channels[0] == refined
    checks.append('snapshot capture/save/restore returns the exact mask, channel and source')

    call('load_selection', **{'from': 'layer_mask'}, layer=0)
    for arguments in [dict(edge_radius=65), dict(edge_strength=0.5), dict(decontaminate=True),
                      dict(trimap='auto'), dict(foreground=True), dict(smooth=65),
                      dict(shift=-65), dict(contrast=1.5), dict(unknown_field=1)]:
        shared.refusal(call, 'refine_selection', **arguments)
    checks.append('experimental and out-of-range controls refuse without touching history')

    call('write_recovery', wait=True)
    status = shared.wait_for(lambda: call('get_recovery_status')['document'],
                             lambda r: r.get('state') == 'protected' and not r.get('pending'),
                             timeout=60, detail='typed refinement recovery')
    return status['recovery_id']


def typed_run(folder, executable, checks):
    with probe.TracedApp(folder / 'app', folder / 'recovery', executable) as app:
        path = g.f.fixture(folder, 16, alpha=True)
        call = probe.opened(app, path, 16, folder / 'typed-calls.jsonl')
        catalog = app.client.rpc('tools/list', {})
        (folder / 'catalog.json').write_text(json.dumps(catalog, indent=2, sort_keys=True) + '\n')
        entry = next(t for t in catalog['tools'] if t['name'] == 'refine_selection')
        assert '16-bit document' in entry['description'], entry['description']
        source = state(call, folder, 'source')[0]
        values = g.coverage(16)
        zero_and_contrast(call, values, checks)
        actions_replay(app, call, folder, checks)
        refined = mask_output(app, call, folder, source, checks)
        recovery_id = persistence(app, call, folder, source, refined, checks)
        app.crash()
    with probe.TracedApp(folder / 'recovered-app', folder / 'recovery', executable) as app:
        available = app.call('get_recovery_status')['available']
        assert recovery_id in {r['recovery_id'] for r in available}
        identifier = app.call('recover_document', recovery_id=recovery_id, show=True)['document']['id']
        call = shared.call_for(app, identifier, folder / 'recovery-calls.jsonl')
        raw, mask, channels = state(call, folder, 'recovered')
        assert raw == source and mask['samples'] == refined and channels[0] == refined
        checks.append('crash recovery restores the refined mask, channel and RGBA16 source exactly')
    return refined


def byte_run(folder, executable, checks):
    with probe.TracedApp(folder / 'byte-app', folder / 'byte-recovery', executable) as app:
        path = g.f.fixture(folder, 8, alpha=True)
        call = probe.opened(app, path, 8, folder / 'byte-calls.jsonl')
        call('select_rect', x=4, y=3, width=14, height=9, feather=1.25)
        result = call('refine_selection', feather=1)
        assert result['coverage_storage'] == 'u8' and result['changed'], result
        checks.append('an 8-bit document still reports u8 coverage storage')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_executable())
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    args = parser.parse_args()
    # Accept either the bundle or the executable inside it.
    if args.app.is_dir():
        args.app = args.app / 'Contents/MacOS/Doctored'
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9i-typed-'))
    folder.mkdir(parents=True, exist_ok=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    before = dependencies()
    checks = []
    report = dict(passed=False, scratch=str(folder), app_sha256=g.f.digest(args.app), checks=checks)
    print('Typed refinement qualification:', folder, flush=True)
    try:
        typed_run(folder, args.app.resolve(), checks)
        byte_run(folder, args.app.resolve(), checks)
        after = dependencies()
        changes = sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k))
        report['source_check'] = dict(files=len(before), changes=changes,
                                      file_set_exact=before.keys() == after.keys())
        if args.require_current:
            assert not changes, changes
        assert g.f.digest(args.app) == report['app_sha256']
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['all_owned_apps_exited'] = all(a.process is None or a.process.poll() is not None
                                              for a in shared.HostApp.owners)
        args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
        print('Typed refinement receipt:', args.report, flush=True)


if __name__ == '__main__':
    main()
