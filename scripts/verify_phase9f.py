#!/usr/bin/env python3
"""Owned Release qualification of selected native tone/filter workflows.

Read raw layer PNGs from native files with the independent framed reader, so
transparent stored RGB and off-canvas pixels are checked without a compositor.
Kernel mathematics has separate independent Rust references; this probe checks
selection mapping, publication, Actions, Undo and persistence end to end.
"""
import argparse
import base64
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
spec = importlib.util.spec_from_file_location('selected_workflow_support', ROOT / 'scripts/verify_phase9e.py')
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
probe, fixtures = shared.probe, shared.fixtures
digest, wait_for, call_for = shared.digest, shared.wait_for, shared.call_for
WIDTH, HEIGHT = 24, 16
stats = shared.load('selected_statistics_oracles', ROOT/'scripts/selected-tone-probe/stats.py')


def dependencies():
    values = shared.dependencies()
    for path in [Path(__file__).resolve(), ROOT/'scripts/verify_phase9e.py',
                 *sorted((ROOT/'scripts/selected-tone-probe').glob('*.py'))]:
        values[str(path.relative_to(ROOT))] = digest(path)
    return values


def fixture(folder, depth, alpha=False):
    maximum = (1 << depth) - 1
    samples = []
    endpoints = [0, 1, 2, 255, 256, 257, 32895, 32896, 65534, 65535]
    for y in range(HEIGHT):
        for x in range(WIDTH):
            value = endpoints[x % len(endpoints)] if y == 0 else 4099 + (x * 1877 + y * 419) % 48000
            rgb = (value, (value + 7103) % 65536, (value + 13001) % 65536)
            a = [0, 1, 257, 20003, 65535][(x + 2*y) % 5] if alpha else 65535
            if depth == 8:
                rgb, a = tuple((v + 128) // 257 for v in rgb), (a + 128) // 257
            samples.append((*rgb, min(a, maximum)))
    path = folder / f'fixture-{depth}-{"alpha" if alpha else "opaque"}.png'
    path.write_bytes(fixtures.png(WIDTH, HEIGHT, depth, samples, fixtures.srgb_profile()))
    return path


def raw(call, folder, name, layer=0):
    path = folder / (name + '.rz')
    call('save_copy', path=str(path))
    native = shared.probe.read_native(path)
    image = shared.probe.png_samples(native['document']['fallbacks'][layer])
    return image['width'], image['height'], image['depth'], tuple(image['samples'])


def raw_file(path, layer=0):
    native = shared.probe.read_native(path)
    image = shared.probe.png_samples(native['document']['fallbacks'][layer])
    return image['width'], image['height'], image['depth'], tuple(image['samples'])


def selection(call, soft=True):
    call('select_rect', x=4, y=3, width=14, height=9, feather=1.25 if soft else 0)
    points = [dict(x=x, y=y) for y in range(HEIGHT) for x in range(WIDTH)]
    values = call('get_selection', points=points)['samples']
    coverage = [int(p['coverage']) for p in values]
    assert 0 in coverage and 255 in coverage
    if soft:
        assert any(0 < v < 255 for v in coverage), 'fixture needs fractional write coverage'
    return coverage


def gated(before, full, coverage, offset=(0, 0), tolerance=1):
    """Independent positive-sum premultiplied interpolation of complete results.

The native implementation gates its unquantized result; using the separately
published full result as this oracle introduces at most one native output code.
Coverage-zero and coverage-one endpoints are checked exactly.
"""
    assert before[:3] == full[:3]
    w, h, _, original = before
    expected = []
    for y in range(h):
        for x in range(w):
            p = (y*w+x)*4
            a, b = original[p:p+4], full[3][p:p+4]
            cx, cy = x+offset[0], y+offset[1]
            c = coverage[cy*WIDTH+cx] if 0 <= cx < WIDTH and 0 <= cy < HEIGHT else 0
            if c == 0 or a == b:
                value = a
            elif c == 255:
                value = b
            else:
                alpha_sum = a[3]*(255-c)+b[3]*c
                if alpha_sum == 0:
                    value = (0, 0, 0, 0)
                else:
                    colors = tuple(math.floor((a[k]*a[3]*(255-c)+b[k]*b[3]*c)/alpha_sum+0.5) for k in range(3))
                    value = (*colors, math.floor(alpha_sum/255+0.5))
            expected.extend(value)
    return tuple(expected)


def check_selected(before, full, after, coverage, offset=(0, 0)):
    assert before[:3] == after[:3] == full[:3]
    expected = gated(before, full, coverage, offset)
    maximum = 0
    for y in range(before[1]):
        for x in range(before[0]):
            p = (y*before[0]+x)*4
            cx, cy = x+offset[0], y+offset[1]
            c = coverage[cy*WIDTH+cx] if 0 <= cx < WIDTH and 0 <= cy < HEIGHT else 0
            difference = max(abs(a-b) for a, b in zip(expected[p:p+4], after[3][p:p+4]))
            assert difference <= (0 if c in (0, 255) else 1), (x, y, c, expected[p:p+4], after[3][p:p+4])
            maximum = max(maximum, difference)
    return maximum


def filters():
    flat = [
        ('grayscale', {}), ('invert', {}), ('sepia', {}), ('edge_detect', {}), ('emboss', {}),
        ('blur', dict(sigma=1.3)), ('sharpen', dict(amount=1.3)),
        ('unsharp_mask', dict(radius=1.3, amount=0.8, threshold=0.001)),
        ('high_pass', dict(radius=1.3)),
        ('reduce_noise', dict(radius=2, luminance=0.05, detail=0.4, color=0.6)),
        ('dust_and_scratches', dict(radius=2, threshold=0.001)),
        ('adjust', dict(brightness=0.07, contrast=0.04, saturation=0.06)),
        ('levels', dict(black=1001/65535, white=61003/65535, gamma=1.13)),
        ('hue_rotate', dict(degrees=17.5)), ('threshold', dict(level=32895.5/65535)),
        ('posterize', dict(levels=7)), ('pixelate', dict(block=3)),
        ('add_noise', dict(amount=0.04, seed=193)),
    ]
    params = [
        ('curves', dict(rgb=[[0,0],[255,61003/257]])),
        ('exposure', dict(exposure=0.25)), ('vibrance', dict(vibrance=0.25)),
        ('hue_saturation', dict(lightness=0.08)),
        ('color_balance', dict(midtones=dict(cyan_red=0.15))),
        ('black_and_white', dict(reds=0.9)),
        ('photo_filter', dict(color='#808080', density=0.4, preserve_luminosity=False)),
        ('channel_mixer', dict(monochrome=True, gray=dict(r=0.2, g=0.3, b=0.4, constant=0.05))),
        ('selective_color', dict(neutrals=dict(k=0.15))),
        ('shadows_highlights', dict(shadows=dict(amount=0.4, tone=0.5), radius=2)),
        ('white_balance', dict(temperature=4800)),
        ('gradient_map', dict(dither=False, gradient=dict(stops=[dict(position=0,color='#000000'),dict(position=1,color='#ffffff')]))),
        ('color_lookup', dict(kind='1d', size=2, table=base64.b64encode(struct.pack('<6f',0,0.125,0.25,1,0.875,0.75)).decode())),
    ]
    return [(name, dict(filter=name, **arguments)) for name, arguments in flat] + [
        (name, dict(filter=name, params=arguments)) for name, arguments in params]


def inventory(app, call, folder, depth, baseline=None):
    cases = []
    original = raw(call, folder, 'original')
    for name, arguments in filters():
        call('deselect')
        response = call('apply_filter', layer=0, **arguments)
        full = raw(call, folder, name+'-full')
        if baseline is not None and depth == 8 and name != 'curves':
            assert full == raw_file(baseline/(name+'-full.rz')), (name, 'legacy byte output changed')
        assert full != original, (name, 'fixture must change', response)
        call('undo')
        assert raw(call, folder, name+'-full-undo') == original
        coverage = selection(call)
        history = probe.history(call)
        app.call('start_recording')
        call('apply_filter', layer=0, **arguments)
        after = raw(call, folder, name+'-selected')
        maximum = check_selected(original, full, after, coverage)
        assert probe.history(call)[1] == history[1] + 1, name
        action = app.call('stop_recording', name=f'Selected {name} {depth}')
        assert len(action['steps']) == 1 and action['steps'][0]['tool'] == 'apply_filter', action
        call('undo')
        assert raw(call, folder, name+'-undo') == original
        call('redo')
        assert raw(call, folder, name+'-redo') == after
        call('undo')
        call('run_action', name=f'Selected {name} {depth}')
        assert raw(call, folder, name+'-replay') == after, name
        call('undo')
        assert raw(call, folder, name+'-replay-undo') == original
        cases.append(dict(filter=name, maximum_gate_error=maximum, samples=len(after[3]), action=action['steps'][0]))
    return cases


def alpha_offsets(app, folder, depth):
    image = fixture(folder, depth, alpha=True)
    call = probe.opened(app, image, depth, folder/'alpha-calls.jsonl')
    call('set_layer_properties', index=0, offset_x=-3, offset_y=2)
    original = raw(call, folder, 'alpha-original')
    cases = []
    wanted = {'invert', 'levels', 'blur', 'sharpen', 'edge_detect', 'emboss', 'pixelate', 'add_noise'}
    for name, arguments in filters():
        if name not in wanted:
            continue
        call('deselect')
        call('apply_filter', layer=0, **arguments)
        full = raw(call, folder, 'alpha-'+name+'-full')
        call('undo')
        coverage = selection(call, soft=False)
        call('apply_filter', layer=0, **arguments)
        after = raw(call, folder, 'alpha-'+name+'-selected')
        check_selected(original, full, after, coverage, offset=(-3, 2))
        call('undo')
        assert raw(call, folder, 'alpha-'+name+'-undo') == original
        cases.append(name)
    return cases


def automatic(app, call, folder, depth):
    coverage = selection(call, soft=False)
    original = raw(call, folder, 'auto-original')
    cases = []
    for tool in ['auto_tone', 'auto_contrast', 'auto_color']:
        default = call(tool, layer=0, clip=0.001)
        default_pixels = raw(call, folder, tool+'-default')
        call('undo')
        whole = call(tool, layer=0, clip=0.001, analysis_scope='whole_target')
        assert raw(call, folder, tool+'-whole') == default_pixels
        call('undo')
        app.call('start_recording')
        selected = call(tool, layer=0, clip=0.001, analysis_scope='selection')
        selected_pixels = raw(call, folder, tool+'-selection')
        assert selected_pixels != default_pixels, (tool, 'analysis scopes must differ')
        for i, c in enumerate(coverage):
            if c == 0:
                assert selected_pixels[3][i*4:i*4+4] == original[3][i*4:i*4+4]
        action = app.call('stop_recording', name=f'Selected {tool} {depth}')
        assert action['steps'][0]['arguments']['analysis_scope'] == 'selection', action
        call('undo')
        assert raw(call, folder, tool+'-undo') == original
        call('run_action', name=f'Selected {tool} {depth}')
        assert raw(call, folder, tool+'-replay') == selected_pixels
        call('undo')
        cases.append(dict(tool=tool, default=default, whole=whole, selection=selected))
    return cases


def persistence(app, call, folder, depth):
    selection(call)
    call('apply_filter', layer=0, filter='levels', black=1001/65535, white=61003/65535, gamma=1.13)
    expected = raw(call, folder, 'persisted-edit')
    native = folder/'persisted.rz'
    call('save_copy', path=str(native))
    reopened = probe.opened(app, native, depth, folder/'reopened.jsonl')
    assert raw(reopened, folder, 'reopened-raw') == expected
    snapshot = call('create_snapshot', name='Selected native tone')['snapshot_id']
    path = folder/'selected.drsnapshot'
    call('save_snapshot', snapshot_id=snapshot, path=str(path))
    call('apply_filter', filter='invert', layer=0)
    loaded = call('load_snapshot', path=str(path))['snapshot_id']
    call('restore_snapshot', snapshot_id=loaded)
    assert raw(call, folder, 'restored') == expected
    call('write_recovery', wait=True)
    status = wait_for(lambda: call('get_recovery_status')['document'],
        lambda r: r.get('state') == 'protected' and not r.get('pending'), timeout=60,
        detail='selected-tone durable recovery')
    return expected, status['recovery_id']


def run_depth(folder, executable, depth, baseline=None):
    folder.mkdir(parents=True)
    image = fixture(folder, depth)
    with probe.TracedApp(folder/'app', folder/'recovery', executable) as app:
        call = probe.opened(app, image, depth, folder/'calls.jsonl')
        catalog = app.client.rpc('tools/list', {})
        (folder/'catalog.json').write_text(json.dumps(catalog, indent=2)+'\n')
        result = dict(depth=depth, fixture_sha256=digest(image), filters=inventory(app,call,folder,depth,baseline),
                      alpha_and_offset_filters=alpha_offsets(app,folder,depth),
                      auto=automatic(app,call,folder,depth))
        result['statistics'] = stats.run(app,folder/'statistics',depth,sys.modules[__name__])
        if depth == 16:
            # 9G admits the paintable planes at 16 bits (PaintTarget.paintablePlanes,
            # routed to precisionScalarFilter); the unmasked fixture's mask and a
            # missing channel still refuse (OperationScope, AgentServer+ChannelTargets).
            for target in ['red', 'alpha']:
                before = raw(call, folder, f'channel-target-{target}-before')
                reply = call('apply_filter', layer=0, filter='invert', target=target)
                assert reply['target'] == target and reply['filter'] == 'invert', reply
                assert reply['storage'] == 'u16' and reply['changed'], reply
                call('undo')
                assert raw(call, folder, f'channel-target-{target}-undo') == before, target
            for target, sentence in [('mask', 'The target layer has no mask.'),
                                     ('channel:missing', 'No channel named "missing"')]:
                error = shared.refusal(call, 'apply_filter', layer=0, filter='invert', target=target)
                assert sentence in error, (target, error)
        shared.refusal(call, 'apply_filter', layer=0, filter='levels', params=dict(black=0.1))
        shared.refusal(call, 'apply_filter', layer=0, filter='exposure', exposure=0.25)
        shared.refusal(call, 'auto_tone', layer=0, analysis_scope='unknown')
        expected, recovery_id = persistence(app,call,folder,depth)
        app.crash()
    with probe.TracedApp(folder/'recovery-app', folder/'recovery', executable) as app:
        available = app.call('get_recovery_status')['available']
        assert recovery_id in {r['recovery_id'] for r in available}
        identifier = app.call('recover_document', recovery_id=recovery_id, show=True)['document']['id']
        call = call_for(app,identifier,folder/'recovered.jsonl')
        assert raw(call,folder,'recovered-raw') == expected
        call('apply_filter',layer=0,filter='invert')
        call('undo')
        assert raw(call,folder,'recovered-undo') == expected
    result['native_snapshot_recovery'] = True
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_executable())
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--require-current', action='store_true')
    parser.add_argument('--byte-baseline', type=Path, help='Optional prior-app receipt directory with independent native outputs')
    args = parser.parse_args()
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase9f-'))
    print('Owned selected-tone workflow:',folder,flush=True)
    before = dependencies()
    report = dict(passed=False,scratch=str(folder),app_sha256=digest(args.app))
    try:
        if args.byte_baseline:
            baseline = json.loads((args.byte_baseline/'report.json').read_text())
            assert baseline['passed'] and len(baseline['cases']) == 30
            report['byte_baseline'] = dict(path=str(args.byte_baseline),app_sha256=baseline['app_sha256'],
                                          fixture_sha256=baseline['fixture_sha256'])
        report['cases'] = [run_depth(folder/f'rgba{d}',args.app.resolve(),d,args.byte_baseline) for d in (8,16)]
        after = dependencies()
        changes = sorted(k for k in before.keys()|after.keys() if before.get(k)!=after.get(k))
        report['source_check'] = dict(files=len(before),changes=changes,file_set_exact=before.keys()==after.keys())
        if args.require_current:
            assert not changes, changes
        assert digest(args.app) == report['app_sha256']
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['all_owned_apps_exited'] = all(a.process is None or a.process.poll() is not None for a in shared.HostApp.owners)
        args.report.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
        print('Selected tone/filter receipt:',args.report,flush=True)


if __name__ == '__main__':
    main()
