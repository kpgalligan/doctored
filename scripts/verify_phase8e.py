#!/usr/bin/env python3
"""Isolated Release-app qualification for revisitable native-depth artwork.

Run only after the final Phase8E app build. --app selects its executable. Fixtures
use independently framed PNG/zlib; exports are independently decoded. Core tests
provide the separate area/curve/continuous-compositing oracles. This external
probe checks editing, geometry, profiles, Actions, native reopen and real crash
recovery, with private settings stores, dummy credentials and owned processes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import time
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
import phase5_support
from phase5_support import App, wait_for, write_report
from verify_phase1 import APP
from verify_phase8b import source_call

sys.path.insert(0, str(ROOT / 'scripts/precision-probe'))
from retained_linked_fixtures import decode_png

WIDTH, HEIGHT = 240, 180
RGBA = [10001 / 65535, 20002 / 65535, 30003 / 65535, 1.0]
TEXT_RGBA = [0.123456789, 0.287654321, 0.745678901, 0.875]
RUNS = [{'location': 0, 'length': 1, 'rgba': [0.817234567, 0.132345678, 0.245678901, 1.0]},
        {'location': 3, 'length': 1, 'rgba': [0.192345678, 0.653456789, 0.287654321, 0.75]}]


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def dependencies():
    files = [*(ROOT / 'app/Sources').glob('*.swift'), *(ROOT / 'core/src').rglob('*.rs'),
             *(ROOT / 'core/tests').rglob('*.rs'), *(ROOT / 'core/include').glob('*.h'),
             *(ROOT / 'app/Bridging').glob('*.h'), ROOT / 'core/Cargo.toml', ROOT / 'core/Cargo.lock',
             ROOT / 'Makefile', ROOT / 'project.yml', Path(__file__).resolve(),
             ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/verification_isolation.py',
             ROOT / 'scripts/app_under_test.py',
             ROOT / 'scripts/verify_phase1.py',
             ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py',
             ROOT / 'scripts/precision-probe/retained_fixtures.py',
             ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
             ROOT / 'scripts/precision-probe/fixtures.py']
    return {str(path.relative_to(ROOT)): digest(path) for path in sorted(set(files))}


def fixture(path, depth, width=WIDTH, height=HEIGHT, transparent=False):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    values = [11311, 22422, 33533, 65535] if depth == 16 else [44, 87, 130, 255]
    if transparent:
        values = [0, 0, 0, 0]
    pixel = struct.pack('>4H', *values) if depth == 16 else bytes(values)
    rows = (b'\0' + pixel * width) * height
    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
        + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


class ArtworkApp(App):
    owners = []

    def __init__(self, folder, recovery, executable):
        super().__init__(folder, recovery, interval=600)
        self.executable = executable

    def __enter__(self):
        self.owners.append(self)
        prior_app = phase5_support.APP
        prior_presets = os.environ.get('DR_RAW_PRESETS_DIR')
        phase5_support.APP = self.executable
        os.environ['DR_RAW_PRESETS_DIR'] = str(self.folder / 'raw-presets')
        try:
            return super().__enter__()
        finally:
            phase5_support.APP = prior_app
            if prior_presets is None:
                os.environ.pop('DR_RAW_PRESETS_DIR', None)
            else:
                os.environ['DR_RAW_PRESETS_DIR'] = prior_presets


def call_for(app, document, trace):
    def call(tool, **arguments):
        try:
            result = source_call(app, document, tool, **arguments)
        except BaseException as error:
            with trace.open('a') as stream:
                stream.write(json.dumps({'tool': tool, 'arguments': arguments, 'error': repr(error)}) + '\n')
            raise
        with trace.open('a') as stream:
            stream.write(json.dumps({'tool': tool, 'arguments': arguments, 'result': result}) + '\n')
        return result
    return call


def normalized_layer(value):
    # The established typed structural report uses dimension/offset arrays and
    # a nullable mask object. Preserve its values while naming them uniformly.
    value = dict(value)
    if 'dimensions' in value:
        value['width'], value['height'] = value['dimensions']
    if 'offset' in value:
        value['offset_x'], value['offset_y'] = value['offset']
    if 'has_mask' not in value:
        value['has_mask'] = value.get('mask') is not None
    return value


def layer(call, index):
    return normalized_layer(next(value for value in call('get_document')['layers'] if value['index'] == index))


def close_numbers(actual, expected, bound=1e-12):
    assert len(actual) == len(expected), (actual, expected)
    assert max(abs(a - b) for a, b in zip(actual, expected)) <= bound, (actual, expected)


def artwork_fields(call):
    result = []
    for value in map(normalized_layer, call('get_document')['layers']):
        entry = {key: value[key] for key in ['index', 'name', 'opacity', 'visible', 'has_mask'] if key in value}
        for key in ['text', 'shape', 'mask']:
            if key in value:
                entry[key] = value[key]
        result.append(entry)
    return result


def exported(call, path, depth):
    call('save_copy', path=str(path))
    decoded = decode_png(path.read_bytes())
    assert decoded['depth'] == depth, decoded['depth']
    return decoded


def pixel_digest(decoded):
    body = b''.join(struct.pack('>4H', *pixel) for pixel in decoded['pixels'])
    return hashlib.sha256(body).hexdigest()


def history_identity(call):
    report = call('list_history')
    return report['current_state_id'], report['undo_count'], report['redo_count']


def validate_description(call, shape_index, text_index):
    shape = layer(call, shape_index)['shape']
    text = layer(call, text_index)['text']
    assert shape['coverage'] == text['coverage'] == 'outline_area_v1'
    assert text['intrinsic_glyphs'] == 'platform_byte_assets_v1'
    close_numbers(text['rgba'], TEXT_RGBA)
    assert text['string'] == 'A😀B' and len(text['color_runs']) == 2
    for actual, expected in zip(text['color_runs'], RUNS):
        assert (actual['location'], actual['length']) == (expected['location'], expected['length'])
        close_numbers(actual['rgba'], expected['rgba'])
    close_numbers(shape['fill_rgba'], RGBA)
    assert shape['stroke_rgba'] is None


def native_alpha_coverage(app, folder):
    """Measure actual persisted transparent edge samples, separately per producer."""
    results = {}
    for kind in ['shape', 'text']:
        source = folder / f'coverage-{kind}.png'
        fixture(source, 16, width=768, height=384, transparent=True)
        document = app.open(source)
        call = call_for(app, document, folder / f'coverage-{kind}-calls.jsonl')
        if kind == 'shape':
            index = call('add_shape_layer', kind='ellipse', x=17.125, y=21.375,
                         w=697.625, h=327.625, fill_rgba=[0.23, 0.57, 0.81, 1], stroke_rgba=None)['layer']
        else:
            index = call('add_text_layer', text='Native curves AgWQRS', x=12.125, y=12.375,
                         size=81.375, font='Helvetica Neue', rgba=[0.23, 0.57, 0.81, 1], wrap_width=730)['layer']
        pixels = exported(call, folder / f'coverage-{kind}-render.png', 16)['pixels']
        alpha = {pixel[3] for pixel in pixels}
        nonbyte = {value for value in alpha if value % 257}
        assert len(alpha) > 256 and len(nonbyte) > 256, (kind, len(alpha), len(nonbyte))
        native = folder / f'coverage-{kind}.rz'
        call('save_copy', path=str(native))
        reopened = call_for(app, app.open(native), folder / f'coverage-{kind}-reopen-calls.jsonl')
        assert kind in layer(reopened, index)
        assert exported(reopened, folder / f'coverage-{kind}-reopened.png', 16)['pixels'] == pixels
        results[kind] = {'unique_alpha': len(alpha), 'unique_nonbyte_alpha': len(nonbyte),
                         'exact_reopened_components': len(pixels) * 4}
    return results


def scenario(folder, depth, executable):
    folder.mkdir(parents=True)
    source = folder / 'base.png'
    fixture(source, depth)
    source_hash = digest(source)
    groups, processes, owned = [], [], []
    recovery = folder / 'recovery'
    native = folder / 'artwork.rz'
    expected_fields = None
    expected_pixels = None
    recovery_id = None
    with ArtworkApp(folder / 'initial', recovery, executable) as app:
        processes.append(app.process.pid); owned.append(app.process)
        catalog = {entry['name'] for entry in app.client.rpc('tools/list', {})['tools']}
        assert {'add_text_layer', 'edit_text_layer', 'add_shape_layer', 'edit_shape_layer',
                'transform_layer', 'distort_layer', 'rasterize_artwork', 'write_recovery',
                'recover_document', 'run_action'} <= catalog
        document = app.open(source)
        call = call_for(app, document, folder / 'initial-calls.jsonl')
        assert not call('get_document')['unsaved_changes']
        app.call('start_recording')
        shape_index = call('add_shape_layer', kind='rect', x=13, y=17, w=83.375, h=42.625,
                           fill_rgba=RGBA, stroke_rgba=None)['layer']
        text_index = call('add_text_layer', text='A😀B', x=15, y=86, size=36,
                          font='Helvetica Neue', rgba=TEXT_RGBA, color_runs=RUNS, wrap_width=190)['layer']
        recorded = app.call('stop_recording', name=f'Native artwork {depth}')
        assert [step['tool'] for step in recorded['steps']] == ['add_shape_layer', 'add_text_layer']
        validate_description(call, shape_index, text_index)
        assert call('get_document')['unsaved_changes']
        initial_fields = artwork_fields(call)
        initial = exported(call, folder / 'initial.png', depth)
        center = initial['pixels'][30 * WIDTH + 30]
        expected_rgb = [round(value * ((1 << depth) - 1)) for value in RGBA[:3]]
        assert max(abs(a - b) for a, b in zip(center[:3], expected_rgb)) <= 1, (center, expected_rgb)
        if depth == 16:
            assert any(value % 257 for value in center[:3]), center
        groups += ['native_continuous_shape_and_mixed_intrinsic_attributed_text', 'one_public_edit_per_action_and_dirty_state']

        repeated = history_identity(call)
        call('edit_shape_layer', layer=shape_index, fill_rgba=RGBA)
        call('edit_text_layer', layer=text_index, rgba=TEXT_RGBA, color_runs=RUNS)
        assert history_identity(call) == repeated
        assert pixel_digest(exported(call, folder / 'no-op.png', depth)) == pixel_digest(initial)
        for tool, arguments in [
            ('edit_text_layer', {'layer': text_index, 'color_runs': [{'location': 1, 'length': 1, 'rgba': RGBA}]}),
            ('edit_text_layer', {'layer': text_index, 'color_runs': [{'location': 0.5, 'length': 1, 'rgba': RGBA}]}),
            ('edit_text_layer', {'layer': text_index, 'rgba': [True, 0, 0, 1]}),
            ('edit_text_layer', {'layer': text_index, 'rgba': RGBA, 'color': '#000000'}),
            ('edit_shape_layer', {'layer': shape_index, 'fill_rgba': [1.1, 0, 0, 1]}),
            ('edit_shape_layer', {'layer': shape_index, 'fill_rgba': RGBA, 'fill': '#000000'}),
        ]:
            call(tool, fail=True, **arguments)
            assert history_identity(call) == repeated
            assert artwork_fields(call) == initial_fields
        groups.append('semantic_noops_and_strict_unicode_color_refusals')

        replay_source = folder / 'replay-input' / 'base.png'
        replay_source.parent.mkdir()
        shutil.copy2(source, replay_source)
        replay_id = app.open(replay_source)
        replay = call_for(app, replay_id, folder / 'action-calls.jsonl')
        replay('run_action', name=f'Native artwork {depth}')
        assert artwork_fields(replay) == initial_fields
        assert exported(replay, folder / 'action-replay.png', depth)['pixels'] == initial['pixels']
        groups.append('recorded_action_reproduces_exact_authored_fields_and_output')

        if depth == 16:
            for tool, arguments, description_key in [
                ('add_shape_layer', {'kind': 'ellipse', 'x': 5, 'y': 8, 'w': 17, 'h': 21,
                                     'fill_rgba': RGBA, 'stroke_rgba': None}, 'shape'),
                ('add_text_layer', {'text': 'Below top', 'x': 7, 'y': 9, 'size': 18,
                                    'font': 'Helvetica Neue', 'rgba': TEXT_RGBA}, 'text'),
            ]:
                # Phase 12E-2b: a typed artwork addition lands at the slot
                # the ONE rule names, which for the UI's default is ABOVE the
                # active entry — the same placement the byte path has always
                # had, rather than the top root the typed path used to take.
                # With layer 0 active the new record is index 1, so the
                # entries below it keep their indices and the entries above it
                # move up by exactly one.
                replay('set_active_layer', index=0)
                previous_layers = artwork_fields(replay)
                reply = replay(tool, **arguments)
                new_index = reply['layer']
                state = replay('get_document')
                assert new_index == 1 and state['active_layer'] == new_index
                assert reply['position'] == 'above'
                assert description_key in layer(replay, new_index)
                fields = artwork_fields(replay)
                # Below the insertion nothing moves at all; above it the SAME
                # records are renumbered by exactly one, so the comparison
                # there drops the index and asserts the numbering separately.
                dropping_index = lambda rows: [{k: v for k, v in row.items() if k != 'index'}
                                               for row in rows]
                assert fields[:1] == previous_layers[:1]
                assert dropping_index(fields[2:]) == dropping_index(previous_layers[1:])
                assert [row['index'] for row in fields] == list(range(len(fields)))
                replay('undo')
                assert artwork_fields(replay) == previous_layers
                assert exported(replay, folder / f'{description_key}-below-top-undo.png', depth)['pixels'] == initial['pixels']
            groups.append('below_top_active_add_lands_above_active_and_preserves_prior_layers')

        call('transform_layer', layer=shape_index, scale=0.25, pivot_x=0, pivot_y=0, sampler='bilinear')
        call('transform_layer', layer=shape_index, scale=4, pivot_x=0, pivot_y=0, sampler='bilinear')
        call('transform_layer', layer=text_index, scale=0.25, pivot_x=0, pivot_y=0, sampler='bilinear')
        call('transform_layer', layer=text_index, scale=4, pivot_x=0, pivot_y=0, sampler='bilinear')
        validate_description(call, shape_index, text_index)
        assert exported(call, folder / 'source-rerender.png', depth)['pixels'] == initial['pixels']
        groups.append('down_up_rerenders_from_retained_description_without_accumulated_loss')

        # Whole-document geometry has a distinct path from a layer transform.
        for tool, arguments, inverse in [
            ('rotate', {'degrees': 90}, {'degrees': 270}),
            ('flip', {'axis': 'horizontal'}, {'axis': 'horizontal'}),
            ('image_size', {'width': WIDTH // 2, 'height': HEIGHT // 2, 'filter': 'bilinear'},
                           {'width': WIDTH, 'height': HEIGHT, 'filter': 'bilinear'}),
        ]:
            call(tool, **arguments)
            assert 'shape' in layer(call, shape_index) and 'text' in layer(call, text_index)
            call(tool, **inverse)
            validate_description(call, shape_index, text_index)
        assert exported(call, folder / 'whole-geometry.png', depth)['pixels'] == initial['pixels']
        groups.append('whole_document_geometry_retains_and_rerenders_artwork')

        for tool in ['assign_profile', 'convert_profile']:
            call(tool, profile='display_p3')
            validate_description(call, shape_index, text_index)
            p3 = exported(call, folder / f'{tool}-p3.png', depth)
            assert p3['pixels'][30 * WIDTH + 30][:3] != initial['pixels'][30 * WIDTH + 30][:3]
            call(tool, profile='srgb')
            validate_description(call, shape_index, text_index)
            assert exported(call, folder / f'{tool}-srgb.png', depth)['pixels'][30 * WIDTH + 30] == initial['pixels'][30 * WIDTH + 30]
        groups.append('assign_and_convert_profiles_rerender_continuous_authored_colors')

        call('select_rect', x=20, y=20, width=64, height=30)
        call('add_layer_mask', layer=shape_index, kind='from_selection')
        call('deselect')
        call('set_layer_properties', index=shape_index, opacity=0.625)
        masked = layer(call, shape_index)
        # Both depths report opacity rounded to two decimals.
        assert masked['has_mask'] and masked['opacity'] == 0.63
        before_edit = exported(call, folder / 'before-edit.png', depth)
        before_fields = artwork_fields(call)
        call('edit_shape_layer', layer=shape_index, w=97.625, fill_rgba=[0.817234567, 0.313456789, 0.217654321, 0.875])
        after_shape = layer(call, shape_index)
        assert after_shape['has_mask'] and after_shape['opacity'] == masked['opacity']
        if 'mask' in masked:
            assert after_shape['mask'] == masked['mask']
        after_edit = exported(call, folder / 'after-edit.png', depth)
        assert after_edit['pixels'] != before_edit['pixels']
        after_fields = artwork_fields(call)
        call('undo')
        assert artwork_fields(call) == before_fields
        assert exported(call, folder / 'undo.png', depth)['pixels'] == before_edit['pixels']
        call('redo')
        assert artwork_fields(call) == after_fields
        assert exported(call, folder / 'redo.png', depth)['pixels'] == after_edit['pixels']
        groups.append('reedit_preserves_mask_opacity_and_exact_undo_redo')

        # A parallelogram keeps the editable description. Perspective requires
        # an explicit, independently undoable rasterization in typed documents.
        info = layer(call, shape_index)
        x, y, w, h = (info[key] for key in ['offset_x', 'offset_y', 'width', 'height'])
        affine = [[x + 3, y + 2], [x + w + 3, y + 2], [x + w + 3, y + h + 2], [x + 3, y + h + 2]]
        perspective = [[x, y], [x + w, y + 2], [x + w - 5, y + h], [x + 2, y + h - 1]]
        call('distort_layer', layer=shape_index, corners=affine, sampler='bilinear')
        assert 'shape' in layer(call, shape_index)
        call('undo')
        assert artwork_fields(call) == after_fields
        if depth == 16:
            unchanged = history_identity(call)
            error = call('distort_layer', layer=shape_index, corners=perspective, sampler='bilinear', fail=True)
            assert 'rasterize' in error.lower()
            assert history_identity(call) == unchanged and artwork_fields(call) == after_fields
        profile = call('get_color_profile')
        app.call('start_recording')
        call('rasterize_artwork', layer=shape_index)
        raster_action = app.call('stop_recording', name=f'Rasterize artwork {depth}')
        assert [step['tool'] for step in raster_action['steps']] == ['rasterize_artwork']
        assert 'shape' not in layer(call, shape_index)
        assert layer(call, shape_index)['has_mask'] and layer(call, shape_index)['opacity'] == masked['opacity']
        assert call('get_color_profile') == profile
        assert exported(call, folder / 'rasterized.png', depth)['pixels'] == after_edit['pixels']
        unchanged = history_identity(call)
        call('rasterize_artwork', layer=shape_index, fail=True)
        assert history_identity(call) == unchanged
        call('undo')
        assert artwork_fields(call) == after_fields
        call('redo')
        assert 'shape' not in layer(call, shape_index)
        call('distort_layer', layer=shape_index, corners=perspective, sampler='bilinear')
        assert 'shape' not in layer(call, shape_index)
        call('undo')
        assert exported(call, folder / 'perspective-undo.png', depth)['pixels'] == after_edit['pixels']
        call('undo')
        assert artwork_fields(call) == after_fields
        replay('run_action', name=f'Rasterize artwork {depth}')
        assert 'shape' not in layer(replay, shape_index)
        assert exported(replay, folder / 'rasterize-action-replay.png', depth)['pixels'] == initial['pixels']
        replay('undo')
        assert artwork_fields(replay) == initial_fields
        groups += ['parallelogram_retention_and_typed_perspective_refusal',
                   'explicit_rasterize_preserves_pixels_mask_profile_with_undo_redo_and_action_replay']

        coverage = native_alpha_coverage(app, folder) if depth == 16 else None
        if coverage:
            groups.append('shape_and_ordinary_text_exceed_256_stored_alpha_values_after_native_reopen')

        call('save_copy', path=str(native))
        assert call('get_document')['unsaved_changes'], 'Save Copy marked a newer master saved'
        reopened_id = app.open(native)
        reopened = call_for(app, reopened_id, folder / 'native-calls.jsonl')
        assert not reopened('get_document')['unsaved_changes']
        assert artwork_fields(reopened) == after_fields
        assert exported(reopened, folder / 'native.png', depth)['pixels'] == after_edit['pixels']
        reopened('edit_text_layer', layer=text_index, text='B😀A', color_runs=RUNS)
        assert layer(reopened, text_index)['text']['string'] == 'B😀A'
        assert reopened('get_document')['unsaved_changes']
        groups.append('native_reopen_retains_editability_and_saved_dirty_identity')
        expected_fields = artwork_fields(reopened)
        expected_pixels = exported(reopened, folder / 'checkpoint.png', depth)['pixels']
        checkpoint = reopened('write_recovery', wait=True)
        checkpoint = wait_for(lambda: reopened('get_recovery_status')['document'],
            lambda value: value.get('state') == 'protected' and not value.get('pending'),
            timeout=20, detail='durable artwork recovery')
        recovery_id = checkpoint['recovery_id']
        app.crash()
    assert digest(source) == source_hash

    # Actual restart, with no prior in-memory owners or imported source URL.
    source.unlink()
    replay_source.unlink()
    with ArtworkApp(folder / 'recovered', recovery, executable) as app:
        processes.append(app.process.pid); owned.append(app.process)
        available = app.call('get_recovery_status')['available']
        assert recovery_id in {entry['recovery_id'] for entry in available}
        document = app.call('recover_document', recovery_id=recovery_id, show=True)['document']['id']
        call = call_for(app, document, folder / 'recovered-calls.jsonl')
        state = call('get_document')
        assert state['path'] is None and state['unsaved_changes']
        assert artwork_fields(call) == expected_fields
        assert exported(call, folder / 'recovered.png', depth)['pixels'] == expected_pixels
        call('edit_text_layer', layer=text_index, text='A😀B', color_runs=RUNS)
        call('edit_shape_layer', layer=shape_index, radius=5.375)
        assert layer(call, shape_index)['shape']['radius'] == 5.375
        assert layer(call, text_index)['text']['string'] == 'A😀B'
        groups.append('real_crash_recovery_preserves_native_artwork_and_subsequent_edits')
    assert all(process.poll() is not None for process in owned)
    return {'passed': True, 'depth': depth, 'groups': groups, 'catalog_count': len(catalog),
            'process_ids': processes, 'all_owned_processes_exited': True,
            'recovery_id': recovery_id, 'native_sha256': digest(native),
            'native_alpha_coverage': coverage,
            'initial_pixel_sha256': pixel_digest(initial)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=APP, help='Qualified Release app executable.')
    parser.add_argument('--report', type=Path, default=Path('/tmp/rz-phase8e-mcp-report.json'))
    parser.add_argument('--keep', action='store_true')
    args = parser.parse_args()
    executable = args.app.resolve()
    assert executable.is_file(), executable
    folder = Path(tempfile.mkdtemp(prefix='rasterize-phase8e-mcp-'))
    print('Artwork scratch:', folder, flush=True)
    before = dependencies()
    write_report(folder / 'source-provenance.json', before)
    app_hash = digest(executable)
    archive = ROOT / 'core/target/release/libdoctored_core.a'
    archive_hash = digest(archive)
    report = {'passed': False, 'scratch': str(folder), 'app': str(executable), 'app_sha256': app_hash,
              'archive_sha256': archive_hash, 'cases': []}
    try:
        for depth in [8, 16]:
            case = scenario(folder / f'rgba{depth}', depth, executable)
            report['cases'].append(case)
            print(f'PASS: rgba{depth}, {len(case["groups"])} workflow groups', flush=True)
        current = dependencies()
        changed = sorted(name for name in before.keys() | current.keys() if before.get(name) != current.get(name))
        report['source_check'] = {'files': len(before), 'file_set_exact': before.keys() == current.keys(), 'changes': changed}
        assert not changed, changed
        assert app_hash == digest(executable), 'App changed during qualification'
        assert archive_hash == digest(archive), 'Core archive changed during qualification'
        report['passed'] = True
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        report['process_ids'] = [owner.process.pid for owner in ArtworkApp.owners if owner.process is not None]
        report['all_owned_processes_exited'] = all(owner.process is None or owner.process.poll() is not None
                                                  for owner in ArtworkApp.owners)
        write_report(folder / 'report.json', report)
        write_report(args.report, report)
    if not args.keep:
        shutil.rmtree(folder)


if __name__ == '__main__':
    main()
