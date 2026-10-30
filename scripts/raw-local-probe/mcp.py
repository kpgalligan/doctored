"""External Phase 8G workflows through the actual Release app.

Native readers are independent of the product. Every original, process,
credential placeholder, Action, preset and recovery directory is private.
"""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import struct
import sys
import time

sys.dont_write_bytecode = True

from support import HostApp, call_for, wait_for
from native import (_legacy_body, read_native, local_asset, read_mask, mutate_attachment,
                    png_samples, fallback_pngs, independent_mask_digests)


class TracedHostApp(HostApp):
    def call(self, tool_name, **arguments):
        record = dict(tool=tool_name, arguments=arguments)
        try:
            result = super().call(tool_name, **arguments)
            record['result'] = result
            return result
        except BaseException as error:
            record['error'] = repr(error)
            raise
        finally:
            with (self.folder / 'global-calls.jsonl').open('a') as stream:
                stream.write(json.dumps(record) + '\n')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def history(call):
    value = call('list_history')
    return value['current_state_id'], value['undo_count'], value['redo_count']


def opened(app, path, depth, trace):
    value = app.call('open_document', path=str(path), precision='rgba8' if depth == 8 else 'preserve')
    identifier = value['document']['id']
    call = call_for(app, identifier, trace)
    call.identifier = identifier
    assert call('get_precision')['mode'] == f'rgba{depth}'
    return call


def source(call):
    return call('get_raw_source', layer=0)['source']


def locals_info(call):
    return call('get_raw_locals', layer=0)


def fingerprints(call):
    return [node['mask']['sha256'] for node in locals_info(call)['nodes']]


def pixels(call, path, depth):
    call('save_copy', path=str(path))
    value = png_samples(Path(path).read_bytes())
    assert value['depth'] == depth
    return value


def save_native(call, path):
    call('save_copy', path=str(path))
    return local_asset(path)


def exact_native(left, right):
    a, aa, am = local_asset(left)
    b, ba, bm = local_asset(right)
    assert aa['samples'] == ba['samples'] and aa['profile'] == ba['profile']
    assert aa['raw']['descriptor'] == ba['raw']['descriptor']
    assert a['resources'] == b['resources']
    assert a['bindings'] == b['bindings']
    assert [mask['samples'] for mask in am] == [mask['samples'] for mask in bm]
    assert fallback_pngs(left) == fallback_pngs(right)
    return len(aa['samples'])


def geometry_migrations(host, folder, executable, depth):
    matching = [group for group in host.get('geometry', [])
                if any(row.get('depth') == depth for row in group.get('groups', []))]
    assert len(matching) == 1
    ui_report = matching[0].get('ui_replay_reopen', {})
    assert ui_report.get('passed') and ui_report.get('name') == 'registered-vignette-ui-replay-reopen'
    rows = [row for group in matching
            for row in group.get('groups', []) if row.get('depth') == depth]
    assert {row['name'] for row in rows} == {
        'registered-vignette-crop', 'registered-vignette-pad',
        'registered-vignette-exif', 'registered-vignette-affine-chain'}
    folder.mkdir(parents=True, exist_ok=True)
    groups = []
    with TracedHostApp(folder / 'actions', folder / 'recovery', executable) as app:
        for row in rows:
            name = row['name']
            call = opened(app, row['historical_native'], depth, folder / f'{name}.jsonl')
            independent = None
            if name == 'registered-vignette-affine-chain':
                old_width, old_height = row['old_dimensions']
                call('select_rect', x=31, y=43, width=old_width - 76, height=old_height - 91)
                call('add_layer_mask', layer=0, kind='from_selection')
                call('set_mask_properties', layer=0, linked=False, density=.71, feather=1.25)
                call('deselect')
                corners = [[2, 4], [old_width - 3, 9],
                           [old_width - 12, old_height - 6], [8, old_height - 3]]
                call('distort_layer', layer=0, corners=corners, sampler='bilinear')
                retouch = call('add_shape_layer', kind='ellipse', x=71.25, y=62.5,
                               w=61.5, h=43.25, fill_rgba=[.21, .42, .63, .85],
                               stroke_rgba=None)['layer']
                call('rasterize_artwork', layer=retouch)
                call('brush_stroke', layer=retouch, points=[[82, 74], [112, 91]],
                     size=9, color='#a53b71', opacity=.65)
                call('select_rect', x=58, y=51, width=141, height=109)
                call('add_layer_mask', layer=retouch, kind='from_selection')
                call('set_mask_properties', layer=retouch, linked=True,
                     density=.83, feather=.75)
                call('deselect')
                call('set_active_layer', index=0)
                before_structural = folder / f'{name}-projective-before.rz'
                call('save_copy', path=str(before_structural))
                independent = read_native(before_structural)
                assert independent['bindings'][0]['transform_kind'] == 'projective'
                assert len(fallback_pngs(before_structural)) >= 2
                mask_digests = independent_mask_digests(before_structural)
                assert len(mask_digests) == 2 and mask_digests[0] != mask_digests[1]
                legacy_body, offset, projective = _legacy_body(before_structural.read_bytes())
                assert offset in (4, 8) and projective.get(0)
                legacy_path = folder / f'{name}-legacy-{21 if depth == 8 else 22}.rz'
                legacy_path.write_bytes(legacy_body)
                legacy = read_native(legacy_path)
                assert legacy['document']['version'] == (21 if depth == 8 else 22)
                assert fallback_pngs(legacy_path) == fallback_pngs(before_structural)
                assert independent_mask_digests(legacy_path) == mask_digests
                _, legacy_asset, legacy_masks = local_asset(legacy_path, recipe_versions=(4,))
                assert legacy_asset['samples'] == independent['assets'][
                    independent['bindings'][0]['asset']]['samples']
                assert legacy_masks[0]['sha256'] == row['before_masks'][0]
                groups.append('legacy-8g21-22-native-reader-compatibility')
                assert not call('get_mask_properties', layer=0)['linked']
                assert call('get_mask_properties', layer=retouch)['linked']
            saved = source(call)
            assert saved['decoder_status'] == 'unavailable'
            width, height = row['new_dimensions']
            matrix = row['inverse']
            geometry = dict(version=1, kind='affine', width=width, height=height,
                            inverse=matrix, filter='bilinear' if name.endswith('affine-chain') else 'bicubic')
            malformed = dict(geometry, unknown_nested_field=1)
            before_refusal = history(call)
            call('preview_raw_decoder_migration', layer=0, geometry=malformed, fail=True)
            assert history(call) == before_refusal
            review, images = inspect(app, call, folder / f'{name}-review.jsonl',
                                     'preview_raw_decoder_migration', geometry=geometry)
            assert review['before_width'] == row['old_dimensions'][0]
            assert review['before_height'] == row['old_dimensions'][1]
            assert review['after_width'] == width and review['after_height'] == height
            assert review['geometry'] == geometry and len(images) == 2
            assert review['before_masks'] == row['before_masks']
            app.call('start_recording')
            call('migrate_raw_decoder', layer=0, preview_id=review['preview_id'])
            action_name = f'{name}-{depth}-geometry-migration'
            recorded = app.call('stop_recording', name=action_name)
            step = recorded['steps'][0]['arguments']['migration']
            assert step['geometry'] == geometry and step['after_masks'] == row['after_masks']
            actual = source(call)
            recipe = actual['raw']['recipe']
            assert recipe['version'] == 4
            assert recipe['vignetteReference']['referenceSize'] == row['old_dimensions']
            assert recipe['vignetteReference']['referenceFromSource'] == matrix
            assert fingerprints(call) == row['after_masks']
            accepted = folder / f'{name}-accepted.rz'
            call('save_copy', path=str(accepted))
            _, asset, masks = local_asset(accepted, recipe_versions=(4,))
            assert hashlib.sha256(asset['samples'].tobytes()).hexdigest() == row['reviewed_sha256']
            assert masks[0]['sha256'] == row['after_masks'][0]
            if independent is not None:
                structural = read_native(accepted)
                assert structural['bindings'][0]['transform_kind'] == 'projective'
                assert structural['bindings'][0]['projective_transform'] != \
                    independent['bindings'][0]['projective_transform']
                assert structural['bindings'][1:] == independent['bindings'][1:]
                assert independent_mask_digests(accepted) == mask_digests
                assert fallback_pngs(accepted)[1:] == fallback_pngs(before_structural)[1:]
                assert not call('get_mask_properties', layer=0)['linked']
                assert call('get_mask_properties', layer=retouch)['linked']
                groups.append('registered-projective-retouch-linked-unlinked-mask-migration')
            call('undo')
            assert source(call)['raw'] == saved['raw']
            assert call('run_action', name=action_name)['ok']
            assert source(call)['raw'] == actual['raw']
            call('undo'); call('redo')
            assert source(call)['raw'] == actual['raw']
            reopened = opened(app, accepted, depth, folder / f'{name}-reopened.jsonl')
            assert source(reopened)['raw'] == actual['raw']
            resaved = folder / f'{name}-resaved.rz'
            reopened('save_copy', path=str(resaved))
            assert Path(accepted).read_bytes() == Path(resaved).read_bytes()
            if name == 'registered-vignette-affine-chain':
                canonical_path = folder / f'{name}-canonical.drmask'
                canonical = reopened('get_raw_local_mask', layer=0, id='geometry-local',
                                     path=str(canonical_path))
                assert canonical['sha256'] == row['after_masks'][0]
                developed = {}
                for channel in ['red', 'green', 'blue']:
                    path = folder / f'{name}-{channel}.drmask'
                    info = reopened('get_raw_local_mask', layer=0, id='geometry-local',
                                    space='developed', channel=channel, path=str(path))
                    plane = read_mask(path.read_bytes())
                    assert info['derived'] and info['space'] == 'developed'
                    assert (plane['width'], plane['height']) == (width, height)
                    assert plane['sha256'] == info['sha256']
                    developed[channel] = plane['sha256']
                assert len(set(developed.values())) == 3
                default_green = reopened('get_raw_local_mask', layer=0,
                    id='geometry-local', space='developed')
                assert default_green['channel'] == 'green'
                assert default_green['sha256'] == developed['green']
                for field in ('space', 'channel'):
                    for malformed_value in (None, True, 7, 1.25, [], {}):
                        refusal = history(reopened)
                        refusal_text = reopened('get_raw_local_mask', layer=0,
                            id='geometry-local', fail=True, **{field: malformed_value})
                        assert f'RAW local mask {field}' in refusal_text
                        assert history(reopened) == refusal
                groups.append('get-raw-local-mask-strict-enum-types')
                refusal = history(reopened)
                reopened('edit_raw_development', layer=0,
                         raw=dict(source_geometry=dict(recipe['sourceGeometry'],
                             unknown_nested_field=1)), fail=True)
                assert history(reopened) == refusal
                flip = dict(version=1, kind='exif', orientation=2)
                app.call('start_recording')
                reopened('edit_raw_development', layer=0,
                         operations=[dict(op='remap', id='geometry-local', map=flip)])
                mask_after = fingerprints(reopened)
                assert mask_after != row['after_masks']
                remap_name = f'{name}-{depth}-native-remap'
                remap_action = app.call('stop_recording', name=remap_name)
                assert remap_action['steps'][0]['arguments']['operations'][0]['map'] == flip
                reopened('undo')
                assert fingerprints(reopened) == row['after_masks']
                assert reopened('run_action', name=remap_name)['ok']
                assert fingerprints(reopened) == mask_after
                reopened('undo')
                groups.append('strict-nested-geometry-and-developed-channel-export-native-remap-replay')
            patch = dict(exposure=.125)
            reopened('edit_raw_development', layer=0, raw=patch)
            edited = source(reopened)['raw']['recipe']
            assert edited['vignetteReference'] == recipe['vignetteReference']
            assert edited['enhancements']['vignette'] == recipe['enhancements']['vignette']
            groups.append(name)
    groups.append('registered-vignette-ui-replay-reopen')
    return dict(groups=groups, passed=True)


def refuse(call, arguments, tool='edit_raw_development'):
    before = history(call)
    value = call(tool, fail=True, **arguments)
    assert history(call) == before, (tool, arguments, value)
    return value


def edit(call, operations=None, raw=None):
    args = dict(layer=0)
    if operations is not None:
        args['operations'] = operations
    if raw is not None:
        args['raw'] = raw
    return call('edit_raw_development', **args)


def radial(identifier='radial', width=384, height=512, **controls):
    return dict(op='create', id=identifier, name=identifier.title(),
        mask=dict(op='radial', center=[width * .47, height * .51],
                  radius=[width * .39, height * .36], hardness=.137), **controls)


def inspect(app, call, trace, tool='inspect_raw_development', **arguments):
    deadline = time.monotonic() + 60
    while True:
        request = dict(document_id=call.identifier, layer=0,
                       region=dict(x=31, y=47, width=32, height=24), **arguments)
        result = app.client.rpc('tools/call', dict(name=tool, arguments=request))
        message = next(block['text'] for block in result['content'] if block['type'] == 'text')
        if result.get('isError') and 'Another background operation is running.' in message and time.monotonic() < deadline:
            time.sleep(.05)
            continue
        with Path(trace).open('a') as stream:
            stream.write(json.dumps(dict(tool=tool, arguments=request, result=message,
                                         error=result.get('isError', False))) + '\n')
        assert not result.get('isError'), message
        images = [base64.b64decode(block['data']) for block in result['content'] if block['type'] == 'image']
        assert len(images) == 2
        return json.loads(message), images


def mutate_raw(source_path, destination, mutate):
    parsed = read_native(source_path, pixels=False)
    raw = parsed['assets'][0]['raw']
    descriptor = raw['descriptor']
    mutate(descriptor)
    value = json.dumps(descriptor, separators=(',', ':'), sort_keys=True).encode()
    start, end = raw['descriptor_range']
    data = Path(source_path).read_bytes()
    Path(destination).write_bytes(data[:start] + struct.pack('<I', len(value)) + value + data[end:])


def mutate_mask(source_path, destination, *, future=False, correct_hash=True):
    parsed = read_native(source_path, pixels=False)
    attachment = parsed['assets'][0]['attachment']
    resource = attachment['resources'][0]
    start, end = parsed['resource_ranges'][resource]
    data = bytearray(Path(source_path).read_bytes())
    if future:
        struct.pack_into('<I', data, start + 4, 2)
    else:
        data[start + 16 + 2 * 107] ^= 1
    if correct_hash:
        descriptor = attachment['descriptor']
        descriptor['resources'][0]['sha256'] = hashlib.sha256(data[start:end]).hexdigest()
        value = json.dumps(descriptor, separators=(',', ':'), sort_keys=True).encode()
        low, high = attachment['descriptor_range']
        data = data[:low] + struct.pack('<I', len(value)) + value + data[high:]
    Path(destination).write_bytes(data)


def action_failure(call, name):
    before = history(call)
    result = call('run_action', name=name)
    assert not result['ok'] and result['failures'], result
    assert history(call) == before
    return result


def strict_cases(call):
    cases = [dict(layer=value) for value in [None, True, .5, -1, 999]]
    cases += [dict(layer=0, operations=value) for value in [None, {}, [None]]]
    cases += [dict(layer=0, unexpected=1), dict(layer=0, before_masks=['0' * 64]),
              dict(layer=0, before_recipe='{}')]
    bad_ops = [dict(op='update', id='radial', density=True),
        dict(op='update', id='radial', enabled=1), dict(op='update', id='radial', feather=257),
        dict(op='update', id='radial', exposure_offset=4.001),
        dict(op='update', id='radial', white_balance='custom', temperature=5000),
        dict(op='update', id='radial', unsupported=0), dict(op='remove', id='missing'),
        dict(op='reorder', id='radial', index=.5), dict(op='create', id='not valid', mask=dict(op='empty')),
        dict(op='create', id='extra_empty', mask=dict(op='empty', unknown=1)),
        dict(op='mask', id='radial', command=dict(op='empty', unknown=1)),
        dict(op='mask', id='radial', command=dict(op='invert', unknown=1)),
        dict(op='mask', id='radial', command=dict(op='brush', mode='add', dabs=[
            dict(center=[1, 1], radius=3, hardness=.5, opacity=1)] * 4097)),
        dict(op='mask', id='radial', command=dict(op='radial', center=[1, 1], radius=[0, 2], hardness=.5))]
    cases.extend(dict(layer=0, operations=[value]) for value in bad_ops)
    cases.append(dict(layer=0, operations=[dict(op='update', id='radial', density=.5)] * 257))
    cases.extend(dict(layer=0, raw=value) for value in [
        dict(baseline_compensation=True), dict(baseline_compensation=4.1), dict(moire=1.1),
        dict(vignette=dict(edge_exposure=1, center=[.5, .5], falloff=.249)),
        dict(vignette=dict(edge_exposure=1, center=[False, .5], falloff=2))])
    return [refuse(call, arguments) for arguments in cases]


def migration_cases(app, call, folder, depth, valid, groups):
    unavailable = folder / 'migration-unavailable.rz'
    mutate_raw(valid, unavailable, lambda value: value['decoder'].update(
        implementation='unavailable-independent-phase8g-probe'))
    old = opened(app, unavailable, depth, folder / 'migration.jsonl')
    saved = source(old)
    assert saved['decoder_status'] == 'unavailable'
    refuse(old, dict(layer=0, raw=dict(exposure=.5)), 'redevelop_raw')
    before_report, before_images = inspect(app, old, folder / 'inspection.jsonl')
    assert before_images[0] == before_images[1]
    review0, _ = inspect(app, old, folder / 'inspection.jsonl', 'preview_raw_decoder_migration')
    review1, _ = inspect(app, old, folder / 'inspection.jsonl', 'preview_raw_decoder_migration')
    refuse(old, dict(layer=0, preview_id=review0['preview_id']), 'migrate_raw_decoder')
    old('select_rect', x=2, y=3, width=7, height=9)
    refuse(old, dict(layer=0, preview_id=review1['preview_id']), 'migrate_raw_decoder')
    old('deselect')
    review, reviewed_images = inspect(app, old, folder / 'inspection.jsonl', 'preview_raw_decoder_migration')
    assert review['before_descriptor']['recipe']['enhancements']['locals']
    assert review['committed'] is False and reviewed_images[0] == before_images[0]
    hashes = fingerprints(old)
    app.call('start_recording')
    old('migrate_raw_decoder', layer=0, preview_id=review['preview_id'])
    name = f'Local migration {depth}'
    recorded = app.call('stop_recording', name=name)
    (folder / 'migration-action.json').write_text(json.dumps(recorded, indent=2))
    assert [step['tool'] for step in recorded['steps']] == ['migrate_raw_decoder']
    assert recorded['steps'][0]['arguments']['migration']['masks'] == hashes
    migrated = source(old)
    assert {key: value for key, value in migrated['raw'].items() if key != 'encoded_bytes'} == review['after_descriptor']
    accepted, images = inspect(app, old, folder / 'inspection.jsonl')
    assert images[0] == images[1] == reviewed_images[1]
    assert accepted['before_histogram'] == review['after_histogram']
    path = folder / 'migration-accepted.rz'
    parsed, asset, _ = save_native(old, path)
    assert asset['raw']['descriptor'] == review['after_descriptor']
    assert fingerprints(old) == hashes
    old('undo')
    assert source(old)['raw'] == saved['raw']
    assert old('run_action', name=name)['ok']
    assert source(old)['raw'] == migrated['raw']
    old('undo'); old('redo')
    assert source(old)['raw'] == migrated['raw']
    groups.append('reviewed_local_migration_exact_pixels_histogram_descriptor_native_and_single_undo_action')
    changed = folder / 'migration-changed-mask.rz'
    mutate_mask(unavailable, changed)
    other = opened(app, changed, depth, folder / 'migration-mask-guard.jsonl')
    assert source(other)['raw'] == saved['raw']
    assert fingerprints(other) != hashes
    failure = action_failure(other, name)
    assert 'coverage' in json.dumps(failure).lower() or 'mask' in json.dumps(failure).lower()
    groups.append('migration_action_refuses_changed_coverage_with_identical_camera_recipe')
    return len(asset['samples'])


def unknown_cases(app, folder, depth, valid, expected, groups):
    changes = {
        'future-recipe': lambda value: value['recipe'].update(version=99),
        'future-evaluator': lambda value: value['recipe']['enhancements'].update(evaluation='future-local-evaluator'),
    }
    for name, mutation in changes.items():
        mutate_raw(valid, folder / (name + '.rz'), mutation)
    mutate_attachment(valid, folder / 'future-state.rz', lambda value: value['state'].update(schema_version=2))
    mutate_mask(valid, folder / 'future-mask.rz', future=True)
    mutate_mask(valid, folder / 'corrupt-mask.rz', correct_hash=False)
    for name in [*changes, 'future-state', 'future-mask', 'corrupt-mask']:
        path = folder / (name + '.rz')
        call = opened(app, path, depth, folder / (name + '.jsonl'))
        assert pixels(call, folder / (name + '.png'), depth) == expected
        refuse(call, dict(layer=0), 'get_raw_locals')
        refuse(call, dict(layer=0, raw=dict(exposure=.25)), 'redevelop_raw')
        refused = call('preview_raw_decoder_migration', layer=0, fail=True)
        assert refused
        copy = folder / (name + '-saved.rz')
        call('save_copy', path=str(copy))
        a = read_native(path, verify_hashes=False)
        b = read_native(copy, verify_hashes=False)
        assert a['resources'] == b['resources']
        assert a['assets'][0]['raw']['descriptor'] == b['assets'][0]['raw']['descriptor']
        assert a['assets'][0]['attachment']['descriptor'] == b['assets'][0]['attachment']['descriptor']
        assert a['assets'][0]['samples'] == b['assets'][0]['samples']
    groups.append('future_recipe_evaluator_state_and_payload_preserve_cached_pixels_and_refuse_semantic_use')
    groups.append('same_length_mask_corruption_preserves_cache_but_sha_guard_refuses_coverage_use')


def small_scenario(fixtures, folder, executable, depth):
    folder.mkdir(parents=True, exist_ok=True)
    original = folder / 'source.dng'
    shutil.copyfile(fixtures / 'source.dng', original)
    groups, components = [], 0
    with TracedHostApp(folder / 'app', folder / 'recovery', executable) as app:
        catalog = {item['name'] for item in app.client.rpc('tools/list', {})['tools']}
        assert {'edit_raw_development', 'get_raw_locals', 'get_raw_local_mask', 'copy_raw_local_settings',
                'preview_raw_decoder_migration', 'migrate_raw_decoder', 'sync_raw_settings'} <= catalog
        call = opened(app, original, depth, folder / 'locals.jsonl')
        info = locals_info(call)
        assert (info['source_width'], info['source_height']) == (384, 512) and info['nodes'] == []
        before = pixels(call, folder / 'before.png', depth)
        h = history(call)
        assert edit(call, [])['unchanged'] and history(call) == h
        # One request exercises all four native nodes and the camera-relative
        # baseline/moiré/manual lens controls without extra publication steps.
        operations = [radial(exposure_offset=.7),
            dict(op='create', id='linear', mask=dict(op='linear', **{'from': [0, 0], 'to': [384, 512]}),
                 exposure_offset=-.4, density=.613, feather=1.25),
            radial('custom', exposure_offset=.2, white_balance='custom', temperature=5600, tint=7),
            dict(op='create', id='brush', mask=dict(op='empty'), exposure_offset=.3)]
        settings = dict(baseline_compensation=.375, moire=.5,
            vignette=dict(edge_exposure=.6, center=[.43, .57], falloff=1.7))
        app.call('start_recording')
        assert not edit(call, operations, settings)['unchanged']
        name = f'Native locals {depth}'
        recorded = app.call('stop_recording', name=name)
        (folder / 'local-action.json').write_text(json.dumps(recorded, indent=2))
        assert [step['tool'] for step in recorded['steps']] == ['edit_raw_development']
        args = recorded['steps'][0]['arguments']
        assert args['before_masks'] == [] and json.loads(args['before_recipe'])['content_sha256'] == digest(original)
        after = pixels(call, folder / 'after.png', depth)
        assert after != before and history(call)[1] == h[1] + 1
        call('undo')
        assert pixels(call, folder / 'undo.png', depth) == before
        assert call('run_action', name=name)['ok']
        assert pixels(call, folder / 'action-replayed.png', depth) == after
        assert len(locals_info(call)['nodes']) == 4
        stack = call('get_document')['layers'][0]
        assert stack['image_source']['raw']['content_sha256'] == digest(original)
        assert 'recipe' not in stack['image_source']['raw']
        groups.append('four_native_locals_camera_controls_one_publication_undo_and_guarded_action_replay')
        h = history(call)
        assert edit(call, [], settings)['unchanged'] and history(call) == h
        assert edit(call, [dict(op='update', id='linear', density=.613)])['unchanged'] and history(call) == h
        refuse(call, dict(layer=0, operations=[radial('fifth')]))
        failures = strict_cases(call)
        groups.append('strict_limits_types_unknown_fields_and_equivalent_no_history_refusals')
        initial_masks = fingerprints(call)
        gestures = [dict(op='mask', id='brush', command=dict(op='brush', mode='add', dabs=[
            dict(center=[71.25, 83.5], radius=41.75, hardness=.2, opacity=.731),
            dict(center=[89.5, 106.25], radius=26.5, hardness=.4, opacity=.517)])),
            dict(op='mask', id='brush', command=dict(op='brush', mode='subtract', dabs=[
                dict(center=[85.25, 93.5], radius=15.75, hardness=.1, opacity=.391)]))]
        app.call('start_recording')
        edit(call, gestures)
        brush_name = f'Native brush {depth}'
        brush_action = app.call('stop_recording', name=brush_name)
        assert brush_action['steps'][0]['arguments']['before_masks'] == initial_masks
        masks = fingerprints(call)
        assert masks[:3] == initial_masks[:3] and masks[3] != initial_masks[3]
        call('undo')
        assert fingerprints(call) == initial_masks
        assert call('run_action', name=brush_name)['ok'] and fingerprints(call) == masks
        action_failure(call, brush_name)
        native_path = folder / 'four-locals.rz'
        parsed, asset, mask_values = save_native(call, native_path)
        assert parsed['version'] == (21 if depth == 8 else 22)
        assert parsed['resources'][asset['raw']['resource']] == original.read_bytes()
        for index, node in enumerate(locals_info(call)['nodes']):
            path = folder / (node['id'] + '.drmask')
            exported = call('get_raw_local_mask', layer=0, id=node['id'], path=str(path))
            assert digest(path) == exported['sha256'] == masks[index]
            assert path.read_bytes() == parsed['resources'][asset['attachment']['resources'][index]]
        levels = len(set(mask_values[0]['samples']))
        assert levels > 256 and sum(value % 257 != 0 for value in set(mask_values[0]['samples'])) > 256
        groups.append('native_radial_linear_brush_add_subtract_resources_export_and_more_than_256_levels')
        edit(call, [dict(op='mask', id='radial', command=dict(op='invert'))])
        edit(call, [dict(op='mask', id='radial', command=dict(op='invert'))])
        assert fingerprints(call) == masks
        edit(call, [dict(op='reorder', id='brush', index=0)])
        assert fingerprints(call) == [masks[3], *masks[:3]]
        call('undo')
        assert fingerprints(call) == masks
        h = history(call)
        assert edit(call, [dict(op='reorder', id='radial', index=0)])['unchanged'] and history(call) == h
        groups.append('exact_invert_twice_ordered_reorder_and_undo_preserve_native_planes')
        app.call('start_recording')
        edit(call, [dict(op='update', id='linear', enabled=False),
                    dict(op='reorder', id='brush', index=0), dict(op='remove', id='radial')])
        structure_name = f'Local enable order removal {depth}'
        app.call('stop_recording', name=structure_name)
        structure = locals_info(call)['nodes']
        assert len(structure) == 3 and structure[0]['id'] == 'brush'
        call('undo')
        assert fingerprints(call) == masks
        assert call('run_action', name=structure_name)['ok']
        assert locals_info(call)['nodes'] == structure
        call('undo')
        assert fingerprints(call) == masks
        groups.append('enabled_state_reorder_remove_and_brush_actions_replay_native_state_and_refuse_changed_start')
        target_before = locals_info(call)['nodes'][1]
        app.call('start_recording')
        call('copy_raw_local_settings', layer=0, id='linear', source_layer=0, source_id='custom')
        copy_name = f'Local controls {depth}'
        copy_action = app.call('stop_recording', name=copy_name)
        assert copy_action['steps'][0]['tool'] == 'edit_raw_development'
        target_after = locals_info(call)['nodes'][1]
        for key in ['id', 'name', 'density', 'feather', 'mask']:
            assert target_before[key] == target_after[key]
        assert target_after['temperature'] == 5600 and target_after['tint'] == 7
        call('undo')
        assert call('run_action', name=copy_name)['ok']
        assert locals_info(call)['nodes'][1] == target_after
        groups.append('copy_local_settings_preserves_receiving_coverage_and_records_resolved_controls')
        # Global presets remain separate from per-image coverage, including
        # after changing the named preset which an Action originally resolved.
        masks = fingerprints(call)
        preset_settings = dict(baseline_compensation=.125, moire=.25,
            vignette=dict(edge_exposure=-.4, center=[.5, .5], falloff=2))
        app.call('save_raw_preset', name='Global lens controls', raw=preset_settings)
        app.call('start_recording')
        call('apply_raw_preset', name='Global lens controls', layer=0)
        preset_action = app.call('stop_recording', name=f'Lens preset {depth}')
        assert preset_action['steps'][0]['tool'] == 'apply_raw_settings'
        assert fingerprints(call) == masks
        recipe = source(call)['raw']['recipe']
        call('undo')
        app.call('save_raw_preset', name='Global lens controls', raw=dict(baseline_compensation=-.5), replace=True)
        assert call('run_action', name=f'Lens preset {depth}')['ok']
        assert source(call)['raw']['recipe'] == recipe and fingerprints(call) == masks
        groups.append('global_enhancement_presets_leave_masks_and_record_immutable_resolved_settings')
        # Invalid final sync target must be rejected before the first target is
        # changed; camera-specific capability refusal is independently tested
        # by the actual-source host probe.
        h = history(call)
        call('sync_raw_settings', layer=0, fields=['baseline_compensation', 'moire', 'vignette'],
             targets=[dict(document_id=call.identifier, layer=0),
                      dict(document_id=call.identifier, layer=999)], fail=True)
        assert history(call) == h and fingerprints(call) == masks
        groups.append('multi_target_global_sync_preflights_final_target_before_publication')
        valid = folder / 'valid-final.rz'
        save_native(call, valid)
        expected = pixels(call, folder / 'valid-final.png', depth)
        # Same recipe but altered coverage must reject a canonical local Action.
        call('undo')
        call('redo')
        components += migration_cases(app, call, folder, depth, valid, groups)
        unknown_cases(app, folder, depth, valid, expected, groups)
        # The last removal explicitly removes only coverage attachments; RAW
        # original identity and all global development controls survive.
        source_before = source(call)
        edit(call, [dict(op='remove', id=node['id']) for node in locals_info(call)['nodes']])
        assert locals_info(call)['nodes'] == []
        assert source(call)['raw']['content_sha256'] == source_before['raw']['content_sha256']
        assert source(call)['source_id'] == source_before['source_id']
        no_locals = folder / 'last-local-removed.rz'
        call('save_copy', path=str(no_locals))
        restored = opened(app, no_locals, depth, folder / 'removed-reopen.jsonl')
        assert locals_info(restored)['nodes'] == []
        call('undo')
        assert fingerprints(call) == masks
        groups.append('last_local_removal_keeps_raw_original_and_global_recipe_native_reopen_and_undo')
    return dict(groups=groups, strict_refusals=len(failures), native_mask_levels=levels,
                exact_source_components=components)


def camera_scenario(fixtures, folder, executable, depth):
    folder.mkdir(parents=True, exist_ok=True)
    original = folder / 'camera.NEF'
    shutil.copyfile(fixtures / 'camera.NEF', original)
    original_sha = digest(original)
    groups, components = [], 0
    recovery = folder / 'recovery'
    with TracedHostApp(folder / 'initial-app', recovery, executable) as app:
        call = opened(app, original, depth, folder / 'camera.jsonl')
        info = locals_info(call)
        width, height = info['source_width'], info['source_height']
        assert (width, height) == (3008, 2000)
        edit(call, [radial('subject', width, height, exposure_offset=.625, density=.823, feather=1.5)])
        # Cross-camera transfers copy user intent, never the source camera's
        # calibration or coverage. Both cameras retain their own native masks.
        donor_path = folder / 'donor.dng'
        shutil.copyfile(fixtures / 'source.dng', donor_path)
        donor = opened(app, donor_path, depth, folder / 'donor.jsonl')
        edit(donor, [radial('donor', exposure_offset=.3)], raw=dict(baseline_compensation=.25,
            moire=.25, vignette=dict(edge_exposure=.2, center=[.4, .6], falloff=1.5)))
        donor_masks, camera_masks = fingerprints(donor), fingerprints(call)
        camera_nodes = locals_info(call)['nodes']
        donor_recipe = source(donor)['raw']['recipe']
        camera_recipe = source(call)['raw']['recipe']
        donor('sync_raw_settings', layer=0, fields=['baseline_compensation', 'moire', 'vignette'],
              targets=[dict(document_id=call.identifier, layer=0)])
        synced = source(call)['raw']['recipe']
        assert synced['enhancements']['baselineReference'] == camera_recipe['enhancements']['baselineReference']
        assert synced['enhancements']['baselineReference'] != donor_recipe['enhancements']['baselineReference']
        assert synced['enhancements']['baselineCompensation'] == .25
        # These two recipe fields are declared Float, whose Codable decimal
        # text is the shortest binary32 roundtrip (e.g. -0.55). Compare their
        # actual binary32 values, not an unintended binary64 interpretation.
        f32 = lambda value: struct.unpack('<f', struct.pack('<f', value))[0]
        expected_baseline = f32(f32(camera_recipe['enhancements']['baselineReference']) + .25)
        assert f32(synced['baselineExposure']) == expected_baseline
        assert fingerprints(call) == camera_masks and fingerprints(donor) == donor_masks
        assert locals_info(call)['nodes'] == camera_nodes
        call('copy_raw_local_settings', layer=0, id='subject', source_document_id=donor.identifier,
             source_layer=0, source_id='donor')
        copied = locals_info(call)['nodes'][0]
        assert copied['exposure_offset'] == .3
        for key in ['id', 'name', 'mask', 'density', 'feather']:
            assert copied[key] == camera_nodes[0][key]
        call('undo')
        assert locals_info(call)['nodes'] == camera_nodes
        unsupported_path = folder / 'unsupported-moire.rz'
        donor_native = folder / 'donor.rz'
        save_native(donor, donor_native)
        def unsupported(value):
            value['recipe']['moireSupported'] = False
        mutate_raw(donor_native, unsupported_path, unsupported)
        unsupported_call = opened(app, unsupported_path, depth, folder / 'unsupported.jsonl')
        before_camera, before_unsupported = history(call), history(unsupported_call)
        donor('sync_raw_settings', layer=0, fields=['moire'], targets=[
            dict(document_id=call.identifier, layer=0), dict(document_id=unsupported_call.identifier, layer=0)], fail=True)
        assert history(call) == before_camera and history(unsupported_call) == before_unsupported
        assert fingerprints(call) == camera_masks and fingerprints(donor) == donor_masks
        groups.append('cross_camera_sync_keeps_relative_calibration_and_target_masks_and_copy_keeps_target_coverage')
        groups.append('unsupported_final_camera_control_preflight_leaves_all_targets_unchanged')
        call('set_layer_properties', index=0, name='Camera original', opacity=.8125)
        call('transform_layer', layer=0, scale=.85, rotate=7, translate_x=13, translate_y=9)
        call('select_rect', x=37, y=51, width=2700, height=1700)
        call('add_layer_mask', layer=0, kind='from_selection')
        call('set_mask_properties', layer=0, linked=False, density=.87, feather=1.25)
        call('deselect')
        properties = call('get_mask_properties', layer=0)
        retouch = call('add_shape_layer', kind='ellipse', x=97.25, y=103.5, w=47.75, h=39.25,
            fill_rgba=[.173, .419, .631, .8123], stroke_rgba=None)['layer']
        call('rasterize_artwork', layer=retouch)
        call('brush_stroke', layer=retouch, points=[[111, 119], [137, 137]], size=13, color='#4982ad', opacity=.63)
        call('set_active_layer', index=0)
        before_path = folder / 'before-redevelop.rz'
        before, before_asset, _ = save_native(call, before_path)
        before_pixels = pixels(call, folder / 'before-redevelop.png', depth)
        masks = fingerprints(call)
        call('redevelop_raw', layer=0, raw=dict(exposure=.375))
        final_path = folder / 'retouched-locals.rz'
        after, after_asset, _ = save_native(call, final_path)
        assert before['bindings'][0]['transform'] == after['bindings'][0]['transform']
        assert before_asset['source_id'] == after_asset['source_id'] and before_asset['revision_id'] != after_asset['revision_id']
        assert after_asset['raw']['descriptor']['content_sha256'] == original_sha
        assert after['resources'][after_asset['raw']['resource']] == original.read_bytes()
        assert fingerprints(call) == masks and call('get_mask_properties', layer=0) == properties
        assert fallback_pngs(before_path)[1:] == fallback_pngs(final_path)[1:]
        expected = pixels(call, folder / 'after-redevelop.png', depth)
        assert expected != before_pixels
        call('undo')
        assert pixels(call, folder / 'redevelop-undo.png', depth) == before_pixels
        call('redo')
        assert pixels(call, folder / 'redevelop-redo.png', depth) == expected
        groups.append('genuine_camera_native_locals_preserve_original_affine_independent_mask_and_retouch_through_redevelop')
        snapshot = call('create_snapshot', name='Portable local RAW')['snapshot_id']
        snapshot_path = folder / 'portable.drsnapshot'
        call('save_snapshot', snapshot_id=snapshot, path=str(snapshot_path))
        edit(call, [dict(op='update', id='subject', density=.413)])
        snapshot_id = call('load_snapshot', path=str(snapshot_path))['snapshot_id']
        call('restore_snapshot', snapshot_id=snapshot_id)
        assert pixels(call, folder / 'snapshot-restored.png', depth) == expected
        restored_path = folder / 'snapshot-restored.rz'
        save_native(call, restored_path)
        components += exact_native(final_path, restored_path)
        groups.append('portable_snapshot_retains_exact_camera_master_masks_profile_and_retouch')
        call('write_recovery', wait=True)
        checkpoint = wait_for(lambda: call('get_recovery_status')['document'],
            lambda value: value.get('state') == 'protected' and not value.get('pending'),
            timeout=60, detail='native RAW local crash checkpoint')
        recovery_id = checkpoint['recovery_id']
        app.crash()
    original.unlink()
    donor_path.unlink()
    with TracedHostApp(folder / 'native-app', folder / 'native-recovery', executable) as app:
        call = opened(app, final_path, depth, folder / 'native-reopened.jsonl')
        assert pixels(call, folder / 'native-reopened.png', depth) == expected
        reopened_path = folder / 'native-reopened.rz'
        save_native(call, reopened_path)
        components += exact_native(final_path, reopened_path)
        h = history(call)
        assert edit(call, [])['unchanged'] and history(call) == h
        edit(call, [dict(op='update', id='subject', exposure_offset=.75)])
        assert source(call)['raw']['content_sha256'] == original_sha and fingerprints(call) == masks
        call('undo')
        assert pixels(call, folder / 'postdelete-undo.png', depth) == expected
        groups.append('fresh_process_native_reopen_and_local_redecode_work_after_camera_url_deletion')
    with TracedHostApp(folder / 'recovery-app', recovery, executable) as app:
        assert recovery_id in {item['recovery_id'] for item in app.call('get_recovery_status')['available']}
        identifier = app.call('recover_document', recovery_id=recovery_id, show=True)['document']['id']
        call = call_for(app, identifier, folder / 'recovered.jsonl')
        document = call('get_document')
        assert document['path'] is None and document['unsaved_changes']
        assert pixels(call, folder / 'recovered.png', depth) == expected
        recovered_path = folder / 'recovered.rz'
        save_native(call, recovered_path)
        components += exact_native(final_path, recovered_path)
        edit(call, [dict(op='mask', id='subject', command=dict(op='brush', mode='subtract',
            dabs=[dict(center=[1101.25, 913.5], radius=103.75, hardness=.2, opacity=.123)]))])
        assert fingerprints(call) != masks and source(call)['raw']['content_sha256'] == original_sha
        call('undo')
        assert pixels(call, folder / 'recovery-mask-undo.png', depth) == expected
        groups.append('real_crash_recovery_restores_and_edits_native_local_coverage_without_original_url')
    return dict(groups=groups, exact_source_components=components, recovery_id=recovery_id)


def manifest(fixtures: Path, reference: Path, host_report=None) -> dict:
    root = Path(__file__).resolve().parents[2]
    inputs = [root / path for path in ['core/Cargo.toml', 'core/Cargo.lock', 'Makefile', 'project.yml',
        'scripts/phase5_support.py', 'scripts/verification_isolation.py', 'scripts/verify_phase1.py',
        'scripts/verify_phase2.py', 'scripts/app_under_test.py',
        'scripts/verify_phase8b.py', 'scripts/raw-probe/native.py', 'scripts/precision-probe/fixtures.py',
        'scripts/precision-probe/retained_fixtures.py', 'scripts/precision-probe/retained_linked_fixtures.py']]
    for pattern in ['app/Sources/*.swift', 'app/Bridging/*.h', 'core/src/**/*.rs',
                    'core/tests/**/*.rs', 'core/include/*.h', 'scripts/host-source-probe/*.py',
                    'scripts/raw-local-probe/*.py']:
        inputs.extend(root.glob(pattern))
    inputs.extend(path for directory in [fixtures, reference] for path in directory.rglob('*') if path.is_file())
    if host_report:
        inputs.extend(Path(row[key]) for group in host_report.get('geometry', [])
            for row in group.get('groups', []) for key in ('historical_native', 'native'))
    return {str(path): digest(path) for path in sorted(set(inputs))}


def run(fixtures: Path, reference: Path, output: Path, executable: Path, host_report=None) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[2]
    before = manifest(fixtures, reference, host_report)
    (output / 'inputs-before.json').write_text(json.dumps(before, indent=2))
    executable_hash = digest(executable)
    archive = root / 'core/target/release/libdoctored_core.a'
    archive_hash = digest(archive)
    start_owners = len(HostApp.owners)
    scenarios = []
    for depth in [8, 16]:
        small = small_scenario(fixtures, output / f'rgba{depth}' / 'synthetic', executable, depth)
        camera = camera_scenario(fixtures, output / f'rgba{depth}' / 'camera', executable, depth)
        geometry = geometry_migrations(host_report, output / f'rgba{depth}' / 'geometry', executable, depth) \
            if host_report else dict(groups=[], passed=False)
        scenario = dict(document_depth=depth, groups=small['groups'] + camera['groups'] + geometry['groups'],
            strict_refusals=small['strict_refusals'], native_mask_levels=small['native_mask_levels'],
            exact_source_components=small['exact_source_components'] + camera['exact_source_components'],
            recovery_id=camera['recovery_id'])
        scenarios.append(scenario)
        (output / f'rgba{depth}' / 'report.json').write_text(json.dumps(scenario, indent=2))
    owners = HostApp.owners[start_owners:]
    assert all(owner.process.poll() is not None for owner in owners)
    after = manifest(fixtures, reference, host_report)
    assert before == after, dict(changed_inputs=[name for name in before if before[name] != after.get(name)],
                                 new_inputs=sorted(set(after) - set(before)))
    assert digest(executable) == executable_hash and digest(archive) == archive_hash
    result = dict(passed=True, scenarios=scenarios, groups_passed=sum(len(item['groups']) for item in scenarios),
        exact_source_components=sum(item['exact_source_components'] for item in scenarios),
        tool_calls=sum(len(path.read_text().splitlines()) for path in output.rglob('*.jsonl')),
        process_ids=[owner.process.pid for owner in owners], all_owned_processes_exited=True,
        tracked_input_count=len(before), inputs_unchanged=True, input_manifest=str(output / 'inputs-before.json'),
        executable_sha256=executable_hash, archive_sha256=archive_hash)
    (output / 'report.json').write_text(json.dumps(result, indent=2) + '\n')
    return result
