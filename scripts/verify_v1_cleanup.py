#!/usr/bin/env python3
"""MCP drive for the v1 cleanup batch: `apply_filter`'s strict flat keys and
the v1 handlers' refusal of JSON null or a boolean where they read a number.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process.

At 8 and 16 bits:
- FLAT FILTERS: each flat filter called with the keys its sheet records
  succeeds where the depth admits it (a 16-bit refusal must be the depth
  gate's "no qualified 16-bit implementation"); the same call plus
  `"sigmaa": 1` is refused naming "sigmaa", with no history step; an
  admitted filter's numeric key as null or true is refused naming it.
- INDEX: `apply_filter` with `index` (the structural layer key) succeeds.
- REPLAY: a saved Action of sheet-shaped `apply_filter` steps (`layer`
  {"$layer": "active"}, `target` "layer", the filter's own keys) replays as
  one history step.

At 8 bits, every (tool, key) the v1 handlers read as a number, given null
and given true, is refused naming the key, with the history and the document
as they were; a boolean point coordinate is refused by `parsePoints`.

Usage: python3 scripts/verify_v1_cleanup.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import traceback

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client  # noqa: E402

W, H = 64, 48


_INSTANCES = itertools.count()  # one pasteboard name per owned instance


class App:
    """One owned instance, on its own scratch socket."""

    def __init__(self, folder):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.process = None
        self.log = None
        self.client = None

    def __enter__(self):
        executable = app_under_test.default_app_executable()
        app_under_test.require_fresh_app(executable)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.socket = scratch_socket()
        environment = dict(
            os.environ, DR_BACKGROUND='1', DR_AGENT_SOCKET=str(self.socket), ANTHROPIC_API_KEY='dummy',
            DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=str(2 * 1024 ** 3),
            DR_ACTIONS_DIR=str(self.folder / 'actions'),
            DR_RECOVERY_DIR=str(self.folder / 'recovery'),
            DR_PASTEBOARD_NAME=f'rz-verify-v1-cleanup-{os.getpid()}-{next(_INSTANCES)}',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = Client(self.socket)
        try:
            wait_ready(self.client, self.process, 30, self.log.name)
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_):
        # Only the Popen process this instance launched is ever signalled.
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if self.log is not None:
            self.log.close()
            self.log = None
        remove_scratch(self.socket)

    def call(self, tool, **arguments):
        return self.client.call(tool, **arguments)

    def attempt(self, tool, **arguments):
        """(is_error, text) without asserting either outcome."""
        result = self.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
        text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
        return bool(result.get('isError')), text


# The keys each flat filter's sheet records (`ActionSteps.filter` callers),
# with values inside the catalog's ranges.
SHEET_SHAPES = {
    'grayscale': {}, 'invert': {}, 'sepia': {}, 'edge_detect': {}, 'emboss': {},
    'blur': dict(sigma=2),
    'sharpen': dict(amount=1),
    'unsharp_mask': dict(radius=1.5, amount=1, threshold=0),
    'high_pass': dict(radius=3),
    'reduce_noise': dict(radius=1, luminance=0.01, detail=0.5, color=0.5),
    'dust_and_scratches': dict(radius=2, threshold=0),
    'adjust': dict(brightness=0.1, contrast=0.1, saturation=0.1, legacy=False),
    'levels': dict(black=0.05, white=0.95, gamma=1.1, output_black=0, output_white=1),
    'hue_rotate': dict(degrees=30),
    'threshold': dict(level=0.5),
    'posterize': dict(levels=4),
    'pixelate': dict(block=4),
    'add_noise': dict(amount=0.1, seed=3),
    'box_blur': dict(radius=2),
    'surface_blur': dict(radius=3, threshold=15),
    'add_grain': dict(amount=25, size=1, roughness=50, seed=1),
    'radial_blur': dict(amount=10, method='spin', quality='draft', center_x=0.5, center_y=0.5),
    'clouds': dict(seed=1, foreground_color='#000000', background_color='#ffffff',
                   high_contrast=False),
    'difference_clouds': dict(seed=1, foreground_color='#000000', background_color='#ffffff',
                              high_contrast=False),
    'offset': dict(horizontal=5, vertical=3, undefined_areas='wrap'),
    'motion_blur': dict(angle=0, distance=5),
    'lens_blur': dict(radius=3, brightness=0, threshold=255),
    'tilt_shift': dict(radius=5, center_x=0.5, center_y=0.5, angle=0, focus=0.15,
                       transition=0.25),
    'lens_correction': dict(distortion=10, chromatic_red_cyan=0, chromatic_blue_yellow=0,
                            vignette_amount=-20, vignette_midpoint=50, vertical_perspective=0,
                            horizontal_perspective=0, angle=0, scale=100),
    'presence': dict(texture=20, clarity=20, dehaze=20),
    'develop_filter': dict(temperature=10, tint=-5, exposure=0.25, contrast=10, highlights=-10,
                       shadows=10, whites=5, blacks=-5, texture=10, clarity=10, dehaze=5,
                       vibrance=10, saturation=5),
}
NON_NUMERIC = {'legacy', 'high_contrast', 'method', 'quality', 'undefined_areas',
               'foreground_color', 'background_color'}
DEPTH_GATE = 'has no qualified 16-bit implementation'


def document(app, depth):
    """A `W` x `H` document at `depth` with a ramp on its layer."""
    doc = app.call('new_document', width=W, height=H, depth=depth, profile='srgb',
                   background='white')['document']['id']
    app.call('gradient', document_id=doc, x0=0, y0=0, x1=W, y1=H, start_color='#203040',
             end_color='#e0c0a0')
    return doc


def state(app, doc):
    """What a refusal must leave as it was: the history and the document."""
    return (json.dumps(app.call('list_history', document_id=doc), sort_keys=True),
            json.dumps(app.call('get_document', document_id=doc), sort_keys=True))


REFUSALS = [0]


def refused(app, doc, needle, tool, **arguments):
    REFUSALS[0] += 1
    before = state(app, doc)
    is_error, text = app.attempt(tool, document_id=doc, **arguments)
    assert is_error, (tool, arguments, 'was accepted', text)
    assert needle in text, (tool, arguments, needle, text)
    assert state(app, doc) == before, (tool, arguments, 'a refusal changed the document')
    return text


def flat_filters(app, depth):
    doc = document(app, depth)
    admitted, gated = [], []
    for name, shape in SHEET_SHAPES.items():
        is_error, text = app.attempt('apply_filter', document_id=doc, filter=name, **shape)
        if is_error:
            assert depth == 16 and DEPTH_GATE in text, (depth, name, text)
            gated.append(name)
            before = state(app, doc)
            assert app.attempt('apply_filter', document_id=doc, filter=name, sigmaa=1,
                               **shape)[0], (name, 'sigmaa accepted')
            assert state(app, doc) == before, (name, 'a gated refusal changed the document')
            continue
        admitted.append(name)
        text = refused(app, doc, '"sigmaa"', 'apply_filter', filter=name, sigmaa=1, **shape)
        assert f'"{name}" has no argument' in text, text
        for key in shape:
            if key in NON_NUMERIC:
                continue
            for bad, word in ((None, 'null'), (True, 'true')):
                arguments = dict(shape)
                arguments[key] = bad
                refused(app, doc, f'apply_filter: {key} must be a number, not {word}',
                        'apply_filter', filter=name, **arguments)
    return dict(admitted=len(admitted), gated=gated)


def index_key(app, depth):
    doc = document(app, depth)
    reply = app.call('apply_filter', document_id=doc, filter='invert', index=0)
    assert reply.get('ok') is True or reply.get('changed') is not None, reply
    return dict(reply_keys=sorted(reply))


def replay(app, depth):
    doc = document(app, depth)
    active = {'$layer': 'active'}
    steps = []
    for name in ('blur', 'unsharp_mask', 'motion_blur', 'add_noise', 'lens_correction'):
        arguments = dict(filter=name, layer=active, target='layer')
        arguments.update(SHEET_SHAPES[name])
        if name == 'lens_correction':
            arguments = {k: (int(v) if isinstance(v, (int, float)) else v)
                         for k, v in arguments.items()}
        steps.append(dict(tool='apply_filter', arguments=arguments,
                          note='Filters / Image ▸ Adjustments'))
    name = f'v1 cleanup sheet steps {depth}'
    app.call('save_action', name=name, steps=steps)
    start = app.call('list_history', document_id=doc)['cursor']
    reply = app.call('run_action', document_id=doc, name=name)
    assert reply['ok'], reply
    assert app.call('list_history', document_id=doc)['cursor'] == start + 1, 'one history step'
    return dict(steps=len(steps))


SQUARE = [{'closed': True, 'anchors': [[10, 10], [40, 10], [40, 30], [10, 30]]}]
STOPS = [{'position': 0, 'color': '#000000'}, {'position': 1, 'color': '#ffffff'}]


def number_cases():
    """(tool, base arguments, key, where, context): `where` is None for a
    top-level key, else a function placing the bad value in a nested object."""
    cases = []

    def top(tool, keys, base=None, context=None):
        for key in keys:
            cases.append((tool, base or {}, key, None, context or tool))

    # apply_filter names its layer the same way for a flat filter, the lens
    # correction and an adjustment op.
    for name in ('invert', 'lens_correction', 'hue_saturation'):
        top('apply_filter', ['layer', 'index'], dict(filter=name))
    top('path_to_selection', ['feather', 'layer'], dict(path='Work Path'))
    top('selection_to_path', ['tolerance'])
    top('fill_path', ['opacity', 'layer'], dict(path='Work Path'))
    top('stroke_path', ['size', 'hardness', 'flow', 'spacing', 'angle', 'roundness', 'opacity',
                        'layer'], dict(path='Work Path'))
    for tool in ('get_vector_mask', 'set_vector_mask', 'set_vector_mask_properties',
                 'remove_vector_mask'):
        top(tool, ['layer'])
    top('liquify', ['layer'])
    top('remove_stroke', ['size', 'seed', 'layer'], dict(points=[[10, 10], [20, 20]]))
    top('content_aware_move', ['offset_x', 'offset_y', 'seed', 'layer'])
    top('object_select', ['feather'], dict(region=dict(x=5, y=5, width=20, height=20)))
    for key in ('x', 'y', 'width', 'height'):
        def region(value, key=key):
            box = dict(x=5, y=5, width=20, height=20)
            box[key] = value
            return dict(region=box)
        cases.append(('object_select', {}, key, region, 'object_select region'))
    top('add_fill_layer', ['layer'], dict(kind='solid', color='#ff0000'))
    top('edit_fill_layer', ['layer'], dict(color='#00ff00'))
    for key in ('angle', 'scale'):
        def gradient(value, key=key):
            body = dict(stops=STOPS, angle=90, scale=1)
            body[key] = value
            return dict(kind='gradient', gradient=body)
        cases.append(('add_fill_layer', {}, key, gradient, 'gradient'))
    for key in ('position', 'opacity'):
        def stop(value, key=key):
            first = dict(STOPS[0], opacity=1)
            first[key] = value
            return dict(kind='gradient', gradient=dict(stops=[first, STOPS[1]]))
        cases.append(('add_fill_layer', {}, key, stop, 'gradient.stops[0]'))
    return cases


# Tools whose numbers are read by their own parser, with its own sentence.
LOCAL_SENTENCE = {
    'quick_select': ['tolerance', 'edge', 'feather', 'brush_size', 'smooth'],
    'refine_selection': ['smooth', 'feather', 'shift', 'contrast', 'edge_radius',
                         'decontaminate_amount'],
}


def null_and_boolean(app):
    doc = document(app, 8)
    app.call('set_path', document_id=doc, subpaths=SQUARE)
    app.call('select_rect', document_id=doc, x=8, y=8, width=30, height=20)
    first = REFUSALS[0]
    for tool, base, key, where, context in number_cases():
        for bad, word in ((None, 'null'), (True, 'true')):
            arguments = dict(base)
            if where is None:
                arguments[key] = bad
            else:
                arguments.update(where(bad))
            refused(app, doc, f'{context}: {key} must be a number, not {word}', tool, **arguments)
    for bad, word in ((None, 'null'), (True, 'true')):
        refused(app, doc, f'refine_selection: layer must be a number, not {word}',
                'refine_selection', layer=bad)
        for tool, keys in LOCAL_SENTENCE.items():
            base = dict(seeds=[[5, 5]]) if tool == 'quick_select' else {}
            for key in keys:
                refused(app, doc, f'{key} must be a finite number', tool, **dict(base, **{key: bad}))
    # parsePoints: a boolean coordinate is not a number.
    refused(app, doc, 'Each point must be', 'object_select', points=[[True, 2], [30, 2], [30, 30]])
    # Quick Mask: Gaussian Blur's sigma and the gradient's numbers.
    app.call('set_quick_mask', document_id=doc, active=True)
    for bad, word in ((None, 'null'), (True, 'true')):
        refused(app, doc, f'apply_filter: sigma must be a number, not {word}', 'apply_filter',
                filter='blur', sigma=bad)
        for key in ('x0', 'y0', 'x1', 'y1', 'opacity'):
            arguments = dict(x0=0, y0=10, x1=W, y1=10, start_color='#000000', end_color='#ffffff')
            arguments[key] = bad
            refused(app, doc, f'gradient: {key} must be a number, not {word}', 'gradient',
                    **arguments)
    app.call('set_quick_mask', document_id=doc, active=False)
    return dict(refusals=REFUSALS[0] - first)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-cleanup-'))
    output.mkdir(parents=True, exist_ok=True)
    results, failures = {}, []

    def run(name, body):
        try:
            results[name] = body()
            print(f'PASS {name}: {results[name]}', flush=True)
        except Exception:  # noqa: BLE001 - every case reports, then the run fails
            failures.append(name)
            print(f'FAIL {name}', flush=True)
            traceback.print_exc()

    with App(output / 'instance') as app:
        for depth in (8, 16):
            run(f'flat filters {depth}-bit', lambda: flat_filters(app, depth))
            run(f'index key {depth}-bit', lambda: index_key(app, depth))
            run(f'sheet-shaped replay {depth}-bit', lambda: replay(app, depth))
        run('null and boolean numbers', lambda: null_and_boolean(app))
    (output / 'receipt.json').write_text(json.dumps(
        dict(results=results, failures=failures), indent=2, sort_keys=True, default=str))
    sys.exit(1 if failures else 0)


if __name__ == '__main__':
    main()
