"""Actual Release MCP qualification of Phase 9D retouch sampling.

Every check here is about what 9D ADDS: the three sampling modes and what each
one actually reads, the ignore-adjustments control, output to a new retouch
layer, and the compatibility rules that let a call recorded before this slice
replay to the same pixels.

The point the whole slice turns on is `double_application`: cloning under a
strong adjustment layer with `sample: "all"` bakes the grade in and the
composite applies it twice, and `current_and_below` does not. That is the
carryover's own acceptance sentence, made falsifiable — every other scenario
here is about not breaking something while delivering it.

Pixels are read back through the independent DRDC reader, never through the
product. Every app is an owned process with private Actions, recovery and
preset directories.
"""
import json
from pathlib import Path
import shutil
import sys

from support import (HostApp, call_for, read_native, png_samples, digest,
                     fixtures as fixture_module)

sys.dont_write_bytecode = True

BLEMISH = fixture_module.BLEMISH
SOURCE = fixture_module.SOURCE
WIDTH, HEIGHT = fixture_module.WIDTH, fixture_module.HEIGHT


class TracedApp(HostApp):
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


def catalog(app, folder):
    """The published contract, asserted as a SUBSET plus the new vocabulary.

    Deliberately not a tool COUNT: 9D adds no tool, but a count assertion in a
    probe is a tripwire for every later slice that does, and the 9A probe's
    `assert len(names) == 178` is exactly that (it asserts a 9B number today).
    """
    value = app.client.rpc('tools/list', {})
    tools = {entry['name']: entry for entry in value['tools']}
    required = {'clone_stamp', 'heal_stroke', 'spot_heal_stroke', 'patch_region',
                'content_aware_fill'}
    assert required <= tools.keys(), sorted(required - tools.keys())

    sampling = {'current', 'current_and_below', 'all'}
    for name in required:
        properties = tools[name]['inputSchema']['properties']
        assert 'sample' in properties, name
        assert set(properties['sample']['enum']) == sampling, (name, properties['sample'])
        assert 'ignore_adjustments' in properties, name
        # The legacy spelling must still be published, or a model that learned
        # it from an older build has no way to know it still works.
        assert 'sample_all_layers' in properties, name
    for name in ('clone_stamp', 'heal_stroke', 'spot_heal_stroke', 'content_aware_fill'):
        assert set(tools[name]['inputSchema']['properties']['output']['enum']) \
            == {'layer', 'new_layer'}, name
    # …and the one without a new-layer form must NOT publish one.
    assert 'output' not in tools['patch_region']['inputSchema']['properties'], 'patch_region'

    path = folder / 'tools-list.json'
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
    return dict(path=str(path), sha256=digest(path.read_bytes()), tools=len(tools))


def opened(app, path, depth, trace):
    value = app.call('open_document', path=str(path), precision='rgba8' if depth == 8 else 'preserve')
    identifier = value['document']['id']
    call = call_for(app, identifier, trace)
    call.identifier = identifier
    call.layer = 0
    assert call('get_precision')['mode'] == f'rgba{depth}', depth
    return call


def document(app, fixture_folder, folder, depth, name):
    """A fresh copy of the photograph, opened at `depth`."""
    base = folder / f'{name}-photo.png'
    base.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(fixture_folder / f'photo-{depth}.png', base)
    return opened(app, base, depth, folder / f'{name}.jsonl')


def history(call):
    info = call('list_history')
    return info['current_state_id'], info['undo_count'], info['redo_count']


def refuse(call, tool, arguments):
    """A refusal must leave history untouched — it is not an edit."""
    before = history(call)
    result = call(tool, fail=True, **arguments)
    assert history(call) == before, (tool, arguments, result)
    return result


def samples(call, folder, name):
    """The document's own pixels, read back through the independent reader.

    `png_samples` returns a dict, so the samples have to be pulled out:
    `tuple(...)` on the dict itself yields its key names, which are identical
    for every render and silently turn a difference check into a tautology
    (the defect recorded in the 9B probe).
    """
    path = folder / f'{name}.png'
    call('save_copy', path=str(path))
    decoded = png_samples(path.read_bytes())
    return (decoded['width'], decoded['height'], decoded['depth'], tuple(decoded['samples']))


def at(shape, x, y):
    """One RGBA sample out of a `samples()` tuple."""
    width, _, _, values = shape
    i = (y * width + x) * 4
    return tuple(values[i:i + 4])


def add_grade(call, above, brightness=0.12):
    """A STRONG adjustment layer above `above` — the thing that double-applies.

    Brightness is the honest choice for this measurement: it moves every
    covered pixel in one direction by a known amount, so "applied twice" is a
    number rather than an impression.
    """
    result = call('add_adjustment_layer', op='bcs', params={'brightness': brightness},
                  layer=above)
    return result.get('layer', result.get('new_layer_index'))


# ---------------------------------------------------------------- scenarios --

def double_application(app, fixture_folder, folder, depth):
    """THE carryover sentence: retouch under a strong adjustment without
    applying it twice.

    Stack: [0 Photo] [1 Brightness +60]. The blemish is cloned out from a clean
    patch the same distance away. With `all` the sampled pixels already carry
    the grade and the composite grades them AGAIN; with `current_and_below`
    they do not.
    """
    call = document(app, fixture_folder, folder, depth, 'double')
    before = samples(call, folder, 'double-before')
    grade_layer = add_grade(call, 0)
    assert grade_layer == 1, grade_layer

    graded = samples(call, folder, 'double-graded')
    clean_x, clean_y = SOURCE[0] + 2, SOURCE[1] + 2
    plain = at(before, clean_x, clean_y)[0]
    lifted = at(graded, clean_x, clean_y)[0]
    assert lifted > plain, ('the grade must actually lift the picture', plain, lifted)

    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)

    # ALL: the sampled pixels carry the grade, which the composite applies
    # again. The patch reads BRIGHTER than its surroundings.
    call('clone_stamp', sample='all', **stroke)
    twice = samples(call, folder, 'double-all')
    call('undo')

    # CURRENT_AND_BELOW: the sampled pixels are the ungraded photograph, so
    # the composite grades them exactly once and the patch matches.
    call('clone_stamp', sample='current_and_below', **stroke)
    once = samples(call, folder, 'double-below')
    call('undo')

    patch_x, patch_y = BLEMISH[0] + 3, BLEMISH[1] + 4
    neighbour = at(graded, clean_x, patch_y)[0]
    double = at(twice, patch_x, patch_y)[0]
    single = at(once, patch_x, patch_y)[0]

    assert abs(single - neighbour) <= 1, \
        ('current_and_below must match the graded surroundings', single, neighbour)
    assert double > single, ('all must over-apply the grade', double, single)
    # Not clipped: a measurement that saturated at 255 would report a floor
    # rather than the size of the error, and would keep reporting one however
    # much worse the double application got.
    assert double < 255, ('the over-application must be measurable, not clipped', double)
    return dict(group='double application', depth=depth,
                ungraded=plain, graded=lifted, neighbour=neighbour,
                sample_all=double, sample_current_and_below=single,
                over_application_codes=double - single)


def modes(app, fixture_folder, folder, depth):
    """What each mode actually reads, and the identity that keeps `all` the
    historic `sample_all_layers: true`."""
    call = document(app, fixture_folder, folder, depth, 'modes')
    # A layer BELOW the target and a grade above it, so all three modes read
    # something different: `current` sees only the target's own pixels,
    # `current_and_below` adds the layer underneath, `all` adds the grade. A
    # fixture where two of them coincided would pass while the implementation
    # conflated them.
    # Three layers, arranged so the three modes read three DIFFERENT pictures
    # in the region the stroke actually samples from — a fixture where two of
    # them coincided there would pass while the implementation conflated them.
    #
    #   [0] the photograph, recoloured, which only current_and_below and all see
    #   [1] the target: the photograph with a HOLE over half the source region
    #   [2] a strong grade, which only all sees
    #
    # The hole is what makes `current` differ from `current_and_below`: through
    # it the target has no pixels of its own, so `current` samples nothing
    # there while the other two sample the layer underneath.
    call('fill', layer=0, x=1, y=1, color='#1A26B3', contiguous=False, tolerance=255)
    target = call('duplicate_layer', layer=0)['layers'][0]
    call('select_rect', x=SOURCE[0], y=SOURCE[1],
         width=(SOURCE[2] - SOURCE[0]) // 2, height=SOURCE[3] - SOURCE[1])
    call('clear_selection', layer=target)
    call('deselect')
    # The target's own photograph, restored over the rest of the source region
    # so the stroke still has real texture to clone.
    call('fill', layer=target, x=1, y=1, color='#8C8C8C', contiguous=False, tolerance=255)
    call('select_rect', x=SOURCE[0], y=SOURCE[1],
         width=(SOURCE[2] - SOURCE[0]) // 2, height=SOURCE[3] - SOURCE[1])
    call('clear_selection', layer=target)
    call('deselect')
    add_grade(call, target)
    stroke = dict(layer=target,
                  points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)

    results = {}
    for mode in ('current', 'current_and_below', 'all'):
        call('clone_stamp', sample=mode, **stroke)
        results[mode] = samples(call, folder, f'modes-{mode}')
        call('undo')
    # The legacy boolean must reproduce the modes it names, BYTE FOR BYTE:
    # this is the whole compatibility claim.
    call('clone_stamp', sample_all_layers=True, **stroke)
    legacy_true = samples(call, folder, 'modes-legacy-true')
    call('undo')
    call('clone_stamp', sample_all_layers=False, **stroke)
    legacy_false = samples(call, folder, 'modes-legacy-false')
    call('undo')
    # …and an omitted argument must reproduce the tool's own historic default,
    # which for clone_stamp is the composite.
    call('clone_stamp', **stroke)
    omitted = samples(call, folder, 'modes-omitted')
    call('undo')

    assert legacy_true == results['all'], 'sample_all_layers=true is sample=all'
    assert legacy_false == results['current'], 'sample_all_layers=false is sample=current'
    assert omitted == results['all'], 'clone_stamp with no sampling argument still clones all'
    distinct = {results['current'], results['current_and_below'], results['all']}
    assert len(distinct) == 3, \
        'the three modes must read three different pictures on this fixture, or a ' \
        'conflation of two of them would pass unnoticed'
    return dict(group='sampling modes', depth=depth, distinct=len(distinct))


def ignore_adjustments(app, fixture_folder, folder, depth):
    """`ignore_adjustments` drops the grade from an `all` plate, and is an
    identity in the modes that already exclude everything above the target."""
    call = document(app, fixture_folder, folder, depth, 'ignore')
    add_grade(call, 0)
    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)

    call('clone_stamp', sample='all', ignore_adjustments=True, **stroke)
    ignored = samples(call, folder, 'ignore-all-ignored')
    call('undo')
    call('clone_stamp', sample='current_and_below', **stroke)
    below = samples(call, folder, 'ignore-below')
    call('undo')
    call('clone_stamp', sample='all', **stroke)
    kept = samples(call, folder, 'ignore-all-kept')
    call('undo')

    assert ignored == below, \
        'with one adjustment above the target, all+ignore is current_and_below'
    assert ignored != kept, 'and it is not the same as leaving the grade in'

    # The IDENTITY: in the two modes that already cut everything above the
    # target, the flag can change nothing, because an adjustment below the
    # target never reaches the target's own pixels.
    identities = {}
    for mode in ('current', 'current_and_below'):
        call('clone_stamp', sample=mode, ignore_adjustments=True, **stroke)
        with_flag = samples(call, folder, f'ignore-identity-{mode}-on')
        call('undo')
        call('clone_stamp', sample=mode, **stroke)
        without = samples(call, folder, f'ignore-identity-{mode}-off')
        call('undo')
        assert with_flag == without, (mode, 'ignore_adjustments must be an identity here')
        identities[mode] = True
    return dict(group='ignore adjustments', depth=depth, identities=sorted(identities))


def new_layer_output(app, fixture_folder, folder, depth):
    """The repair lands on its own layer, the original pixels survive, and the
    picture is the one the in-place repair produces."""
    call = document(app, fixture_folder, folder, depth, 'output')
    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0] - 2, BLEMISH[2] + 2)],
                  source_x=SOURCE[0] - 2, source_y=SOURCE[1] + 4, size=14)

    call('clone_stamp', sample='current_and_below', **stroke)
    inplace = samples(call, folder, 'output-inplace')
    assert len(call('get_document')['layers']) == 1
    call('undo')

    result = call('clone_stamp', sample='current_and_below', output='new_layer', **stroke)
    assert result['applied']['output'] == 'new_layer', result
    layered = samples(call, folder, 'output-new-layer')
    layers = call('get_document')['layers']
    assert len(layers) == 2, layers
    assert layers[1]['name'] == 'Retouch', layers[1]

    # The new layer composites to the in-place picture, to within ONE code on
    # the stroke's anti-aliased rim — and no further.
    #
    # That bound is arithmetic, not slack. In place, the overlay is composited
    # into the layer's bytes and rounded once; on its own layer it is rounded
    # into the deposit and rounded AGAIN when the compositor quantizes the
    # projection. Wherever the stroke is fully opaque or fully clear the two
    # roundings agree exactly, so the difference can only appear on the
    # partial-coverage rim CoreGraphics draws. The Rust suite proves the
    # hard-edged case byte-exact, which is what pins this to double
    # quantization rather than to the deposit's alpha or its domain.
    width, height, _, left = layered
    right = inplace[3]
    differences = [abs(a - b) for a, b in zip(left, right)]
    worst = max(differences)
    moved = sum(1 for value in differences if value)
    assert worst <= 1, ('the new layer must match the in-place picture', worst)
    assert moved * 200 < len(differences), \
        ('only the rim may differ, not the patch', moved, len(differences))
    # One Undo removes the whole gesture, layer and all.
    call('undo')
    assert len(call('get_document')['layers']) == 1
    return dict(group='new layer output', depth=depth, worst_code_difference=worst,
                samples_differing=moved, samples=len(differences))


def compatibility(app, fixture_folder, folder, depth):
    """The rules that decide whether an existing recording still means what it
    meant, and whether a new one can be read by an older build."""
    call = document(app, fixture_folder, folder, depth, 'compat')
    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)
    refusals = 0

    # Co-presence is ACCEPTED when the two spellings agree — the app's own
    # recorder writes both, so that a step recorded now still replays on a
    # build that predates `sample`.
    call('clone_stamp', sample='all', sample_all_layers=True, **stroke)
    call('undo')
    call('clone_stamp', sample='current', sample_all_layers=False, **stroke)
    call('undo')

    # …and REFUSED when they contradict, rather than one silently winning.
    for sample, legacy in (('all', False), ('current', True), ('current_and_below', True),
                           ('current_and_below', False)):
        message = refuse(call, 'clone_stamp',
                         dict(sample=sample, sample_all_layers=legacy, **stroke))
        assert 'contradict' in message, message
        refusals += 1

    message = refuse(call, 'clone_stamp', dict(sample='sideways', **stroke))
    assert 'current_and_below' in message, message
    refusals += 1

    # The tool with no new-layer form must REFUSE `output`, not ignore it: a
    # silently dropped argument writes into the photograph the caller asked to
    # leave alone and still reports ok.
    for tool, arguments in (
            ('patch_region', dict(layer=0, offset_x=8, offset_y=0,
                                  points=[[BLEMISH[0], BLEMISH[1]], [BLEMISH[2], BLEMISH[1]],
                                          [BLEMISH[2], BLEMISH[3]], [BLEMISH[0], BLEMISH[3]]])),):
        message = refuse(call, tool, dict(output='new_layer', **arguments))
        assert 'new-layer form' in message, (tool, message)
        refusals += 1

    # A blend mode has nothing to blend with on an empty layer.
    message = refuse(call, 'clone_stamp',
                     dict(output='new_layer', blend_mode='Multiply', **stroke))
    assert 'nothing for blend_mode' in message, message
    refusals += 1

    # Every result reports the mode it used, and reports the legacy boolean
    # only where that boolean can tell the truth.
    result = call('clone_stamp', sample='current_and_below', **stroke)
    applied = result['applied']
    assert applied['sample'] == 'current_and_below', applied
    assert 'sample_all_layers' not in applied, \
        'the boolean cannot express current_and_below, so it must not be reported'
    call('undo')
    result = call('clone_stamp', sample='all', **stroke)
    assert result['applied']['sample_all_layers'] is True, result['applied']
    call('undo')
    return dict(group='compatibility', depth=depth, refusals=refusals)


def defaults(app, fixture_folder, folder, depth):
    """Each tool's omitted-argument default is the one it always had. A single
    design-wide default would silently rewrite recorded calls."""
    call = document(app, fixture_folder, folder, depth, 'defaults')
    add_grade(call, 0)
    results = {}
    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)

    for tool, expected in (('clone_stamp', 'all'), ('heal_stroke', 'all')):
        arguments = dict(stroke)
        if tool == 'heal_stroke':
            arguments['opacity'] = 100
        result = call(tool, **arguments)
        assert result['applied']['sample'] == expected, (tool, result['applied'])
        results[tool] = result['applied']['sample']
        call('undo')

    spot = call('spot_heal_stroke', layer=0, size=10,
                points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])])
    assert spot['applied']['sample'] == 'current', spot['applied']
    results['spot_heal_stroke'] = spot['applied']['sample']
    call('undo')
    return dict(group='per-tool defaults', depth=depth, defaults=results)


def typed_refusals(app, fixture_folder, folder):
    """9D-2 enables the sampling choices formerly refused by 9D-1.

    The contradiction gate is still required for BOTH legacy spellings.
    Numerical typed/transform/output qualification lives in verify_phase9d_native.
    """
    call = document(app, fixture_folder, folder, 16, 'typed')
    stroke = dict(layer=0, points=[[x, BLEMISH[1] + 4] for x in range(BLEMISH[0], BLEMISH[2])],
                  source_x=SOURCE[0], source_y=SOURCE[1] + 4, size=10)
    refusals = 0
    for arguments in (dict(sample='all'), dict(sample='current_and_below'),
                      dict(sample_all_layers=True), dict(output='new_layer'),
                      dict(ignore_adjustments=True)):
        result = call('clone_stamp', **arguments, **stroke)
        assert result.get('changed', True), (arguments, result)
        call('undo')
    message = refuse(call, 'clone_stamp', dict(sample='current', sample_all_layers=True, **stroke))
    assert 'contradict' in message, message
    refusals += 1
    # And the mode typed retouch DOES have works, with the argument omitted —
    # which the old rule made impossible, since it demanded an explicit
    # sample_all_layers=false whose only legal value was its own default.
    call('clone_stamp', **stroke)
    call('undo')
    call('clone_stamp', sample='current', **stroke)
    call('undo')
    return dict(group='typed sampling migration and compatibility', depth=16, refusals=refusals)


def depth_case(fixture_folder, folder, executable, depth):
    folder.mkdir(parents=True, exist_ok=True)
    groups, refusals, catalogs = [], 0, []
    with TracedApp(folder / 'app', folder / 'recovery', executable) as app:
        catalogs.append(catalog(app, folder))
        for scenario in (double_application, modes, ignore_adjustments, new_layer_output,
                         compatibility, defaults):
            record = scenario(app, fixture_folder, folder, depth)
            refusals += record.get('refusals', 0)
            groups.append(record)
    return dict(depth=depth, groups=groups, group_count=len(groups), refusals=refusals,
                catalog=catalogs)


def run(fixture_folder, output, executable):
    output.mkdir(parents=True, exist_ok=True)
    cases = []
    for depth in (8, 16):
        print('Phase 9D retouch sampling:', depth, flush=True)
        if depth == 16:
            # Native numerical/output checks have their own extended probe;
            # retain the historical compatibility scenario here.
            folder = output / 'rgba16'
            folder.mkdir(parents=True, exist_ok=True)
            with TracedApp(folder / 'app', folder / 'recovery', executable) as app:
                catalog(app, folder)
                record = typed_refusals(app, Path(fixture_folder), folder)
            cases.append(dict(depth=16, groups=[record], group_count=1,
                              refusals=record['refusals']))
            continue
        cases.append(depth_case(Path(fixture_folder), output / f'rgba{depth}', executable, depth))
    return dict(passed=True, cases=cases,
                group_count=sum(case['group_count'] for case in cases),
                refusals=sum(case['refusals'] for case in cases),
                all_owned_apps_exited=all(owner.process is None or owner.process.poll() is not None
                                          for owner in HostApp.owners))
