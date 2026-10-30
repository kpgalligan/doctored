#!/usr/bin/env python3
"""MCP drive for File > Automate > Merge Panorama... (`merge_panorama`)
end to end: Load Files into Stack, Auto-Align with the middle file as the
reference, Auto-Blend Panorama with Seamless on, and its Content-Aware Fill
Transparent Areas, into ONE new document.

The script owns its app instance (`verify_v1_stack_align.App`: the Debug app or
`DR_VERIFY_APP`, its own scratch socket, `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB pin, private `DR_*_DIR`s) and terminates only
that process. Every expected number is computed here, never read back.

THE PHOTO and THE VIEWS are `scripts/reach-probe/main.swift`'s, with its sizes:
`core/tests/fixtures/registration_photo/sky.jpg` converted by `sips` and read
as grey (`verify_v1_stack_reach.Grey`), wrapped on a cylinder of radius f 600
(cylinder (u, v) is photo point (1024 + u, 450 + v)). A W x H view yawed psi
shows, at pixel (x, y) (xc = x + 0.5 - W/2, yc = y + 0.5 - H/2), cylinder point
(f (psi + atan2(xc, f)), f yc / hypot(xc, f)). A yaw row's pair is the
REFERENCE at -delta/2 and the FLOATING view at +delta/2; a planar row's pair is
two 600 x 400 photo crops, the reference at (724, 250), the floating one shifted
((1 - o) 600, 20) and, for Collage, turned +5 degrees about its centre. THE
LENS: p = c + R q (1 + k |q|^2), q = (u - c)/R, c the frame centre,
R = max(W, H)/2, k -0.05, on both views of a lens row.

THE ROWS (the reach probe's 27 plain rows): A-delta, 600 x 400, delta 5...40 in
5 degree steps; B-delta-o, delta in {20, 30, 40} x overlap o in {15, 30, 50,
70} %, W = round(2 f tan(delta / (2 (1 - o)))), H = min(round(2 W / 3), 800),
W past 2048 omitted (B-40-70); P-reposition-o and P-collage-o, o as above.
Each runs as ONE merge_panorama call on [reference, floating], so the middle file
(index (2 - 1) // 2 = 0) is the reference: yaw rows with the default layout
(Auto), planar rows with `reposition` or `collage`.

TRUTH and ERROR as the reach probe's: the worst |H(p) - T(p)| over the 33 x 33
grid of floating points p = (0.5 + i (W - 1)/32, 0.5 + j (H - 1)/32) whose T(p)
lands in the reference frame [0, W] x [0, H].

BOUNDS, fixed before measuring and never widened:
(a) every row (all 27 register): ok; `align.aligned[0]` is layer 1
    with its `homography` within 2.0 px of the truth; two `blend.layers`;
    exactly one new document, whose history is its initial state (one state,
    cursor 0). A row in REFUSED (currently none) instead answers an in-band
    error starting "Merge Panorama stopped at Auto-Align:" that names the floating
    file, and opens no document.
(b) A-20 with the lens: `distortion` false refuses with that prefix and "the
    frames carry lens distortion; turn on Geometric Distortion"; true answers ok
    with `align.surface` "spherical", |align.distortion.coefficient + 0.05| <=
    0.01 and `align.unregistered` [].
(c) the 600 x 400 views yawed -30, 0, +30 (`left`, `centre`, `right`),
    `cylindrical` + `fill`: `align.reference` 1, `align.focal` within 1 % of 600,
    four layers, the top named "right (merged)", and the composite's alpha 255 at
    every pixel (a PNG `save_copy`).
(d) the same views, `perspective`, Blend on, no fill, against the three tools
    one by one on a `load_files_into_stack` of the same files:
    `auto_align_layers` {reference 1, projection perspective, sampler bicubic}
    then `auto_blend_layers` {panorama, seamless, align false}: the same canvas,
    layer names, visibility and masks, and identical pixels per layer
    (`export_layers` PNGs, decoded). Its composite has transparent pixels, so
    (c)'s alpha check is not vacuous.
(e) `left`, `centre` and a disjoint 600 x 400 photo crop at (1400, 900)
    (`crop`, on top): ok, the crop in `align.unregistered`, hidden, and named in
    `note`; two `blend.layers`. The same with `fill` true: four layers, index 2
    "centre (merged)" visible directly above "centre" (index 1), the hidden crop
    at index 3.
(f) refusals before any load (the paths do not exist, so a load would refuse
    first): `fill` without `blend` answers "Content-Aware Fill Transparent Areas
    needs Blend Images Together."; `vignette` with `collage` names Collage. No
    document opened either time.
(g) Actions: a recording of (c)'s `merge_panorama` and then a `new_layer` on the
    document it made holds two steps, `merge_panorama` and the placeholder "Edits in
    a document made by Merge Panorama"; `run_action` of a saved one-step action
    holding the recorded `merge_panorama` step, on an open document, opens one new
    document with (c)'s canvas and layer count.
(h) Legacy action: an Action file saved before the rename, written into
    `<instance>/actions/` with one step under the tool's earlier name
    `{"tool": "photomerge", "arguments": (c)'s arguments}`, loads: `run_action` on
    an open document opens one new document with (c)'s canvas and layer count.

Usage: python3 scripts/verify_v1_merge_panorama.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_stack_align import App, close, documents, load, rows  # noqa: E402
from verify_v1_stack_reach import PHOTO, Grey  # noqa: E402
from verify_v1_stack_projection import decode_png  # noqa: E402
from verify_phase1 import png  # noqa: E402

FOCAL = 600.0
LENS_K = -0.05
PLAIN_BOUND = 2.0
O0 = (724.0, 250.0)
OVERLAPS = (15, 30, 50, 70)
REFUSED = set()
PREFIX = 'Merge Panorama stopped at Auto-Align:'
LENS_SENTENCE = 'the frames carry lens distortion; turn on Geometric Distortion'
PLACEHOLDER = 'Edits in a document made by Merge Panorama'


def half_up(x):
    """Swift's `.rounded()` for a positive number."""
    return int(math.floor(x + 0.5))


# --------------------------------------------------------------- geometry --

class Lens:
    def __init__(self, width, height, k):
        self.cx, self.cy = width / 2, height / 2
        self.radius = max(width, height) / 2
        self.k = k

    def distort(self, x, y):
        qx, qy = (x - self.cx) / self.radius, (y - self.cy) / self.radius
        s = 1 + self.k * (qx * qx + qy * qy)
        return self.cx + self.radius * qx * s, self.cy + self.radius * qy * s

    def one_to_one(self, x, y):
        qx, qy = (x - self.cx) / self.radius, (y - self.cy) / self.radius
        return 1 + 3 * self.k * (qx * qx + qy * qy) > 0

    def undistort(self, x, y):
        qx, qy = (x - self.cx) / self.radius, (y - self.cy) / self.radius
        rd = math.hypot(qx, qy)
        if rd == 0:
            return x, y
        ru = rd
        for _ in range(30):
            ru -= (ru * (1 + self.k * ru * ru) - rd) / (1 + 3 * self.k * ru * ru)
        s = ru / rd
        return self.cx + self.radius * qx * s, self.cy + self.radius * qy * s


class Pair:
    """The reach probe's `Pair`: two views' sources and the truth between them."""

    def __init__(self, width, height, ideal, reference, floating, lens=False):
        self.width, self.height = width, height
        self.ideal, self.reference, self.floating = ideal, reference, floating
        self.lens = Lens(width, height, LENS_K) if lens else None

    def truth(self, x, y):
        u = self.lens.undistort(x, y) if self.lens else (x, y)
        t = self.ideal(*u)
        if t is None or self.lens is None:
            return t
        if not self.lens.one_to_one(*t):
            return None
        return self.lens.distort(*t)

    def pairs(self):
        out = []
        for j in range(33):
            for i in range(33):
                p = (0.5 + i * (self.width - 1) / 32, 0.5 + j * (self.height - 1) / 32)
                t = self.truth(*p)
                if t is not None and 0 <= t[0] <= self.width and 0 <= t[1] <= self.height:
                    out.append((p, t))
        return out


def cylinder(width, height, psi):
    def source(x, y):
        xc, yc = x - width / 2, y - height / 2
        return 1024 + FOCAL * (psi + math.atan2(xc, FOCAL)), 450 + FOCAL * yc / math.hypot(xc, FOCAL)
    return source


def yaw_pair(width, height, delta, lens=False):
    a = math.radians(delta)

    def ideal(x, y):
        xc, yc = x - width / 2, y - height / 2
        px = xc * math.cos(a) + FOCAL * math.sin(a)
        z = -xc * math.sin(a) + FOCAL * math.cos(a)
        if z <= 1e-9:
            return None
        return FOCAL * px / z + width / 2, FOCAL * yc / z + height / 2
    return Pair(width, height, ideal, cylinder(width, height, -a / 2),
                cylinder(width, height, a / 2), lens)


def planar_pair(shift, turn):
    width, height = 600, 400
    c, s = math.cos(math.radians(turn)), math.sin(math.radians(turn))

    def turned(x, y):
        dx, dy = x - width / 2, y - height / 2
        return c * dx - s * dy + width / 2, s * dx + c * dy + height / 2

    def ideal(x, y):
        q = turned(x, y)
        return q[0] + shift[0], q[1] + shift[1]

    def floating(x, y):
        q = turned(x, y)
        return q[0] + O0[0] + shift[0], q[1] + O0[1] + shift[1]
    return Pair(width, height, ideal, lambda x, y: (x + O0[0], y + O0[1]), floating)


def apply(m, x, y):
    w = m[6] * x + m[7] * y + m[8]
    if not (math.isfinite(w) and w > 0):
        return None
    return (m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w


def worst_error(m, pairs):
    assert len(pairs) > 100, ('too few grid points in the overlap', len(pairs))
    worst = 0.0
    for p, t in pairs:
        q = apply(m, *p)
        if q is None:
            return math.inf
        worst = max(worst, math.hypot(q[0] - t[0], q[1] - t[1]))
    return worst


# ---------------------------------------------------------------- fixtures --

def write_view(path, grey, width, height, source, lens=None):
    """A W x H grey PNG whose pixel p shows the photo at source(D^-1(p))."""
    if path.is_file():
        return path
    out = bytearray(b'\xff' * (width * height * 4))
    for y in range(height):
        for x in range(width):
            p = (x + 0.5, y + 0.5)
            u = lens.undistort(*p) if lens else p
            code = int(min(max(grey.sample(*source(*u)), 0.0), 255.0) + 0.5)
            i = (y * width + x) * 4
            out[i] = out[i + 1] = out[i + 2] = code
    staged = path.with_suffix('.partial')
    png(staged, width, height, bytes(out))
    staged.rename(path)
    return path


def rows_table():
    """(id, pair, layout) for the 27 plain rows, in the reach probe's order."""
    table = []
    for delta in range(5, 45, 5):
        table.append((f'A-{delta}', yaw_pair(600, 400, delta), None))
    for delta in (20, 30, 40):
        for overlap in OVERLAPS:
            o = overlap / 100
            width = half_up(2 * FOCAL * math.tan(math.radians(delta) / (2 * (1 - o))))
            if width > 2048:
                continue
            height = min(half_up(2 * width / 3), 800)
            table.append((f'B-{delta}-{overlap}', yaw_pair(width, height, delta), None))
    for how, turn in (('reposition', 0.0), ('collage', 5.0)):
        for overlap in OVERLAPS:
            shift = ((1 - overlap / 100) * 600, 20.0)
            table.append((f'P-{how}-{overlap}', planar_pair(shift, turn), how))
    assert len(table) == 27, len(table)
    return table


def pair_files(folder, grey, row_id, pair):
    files = []
    for role, source in (('ref', pair.reference), ('float', pair.floating)):
        files.append(write_view(folder / f'{row_id}-{role}.png', grey, pair.width, pair.height,
                                source, pair.lens))
    return files


def author(folder):
    folder.mkdir(parents=True, exist_ok=True)
    converted = folder / 'sky.png'
    if not converted.is_file():
        subprocess.run(['/usr/bin/sips', '-s', 'format', 'png', str(PHOTO), '--out',
                        str(converted)], check=True, capture_output=True, timeout=120)
    return Grey(converted)


# ----------------------------------------------------------------- plumbing --

def merge_panorama(app, paths, **arguments):
    return app.call('merge_panorama', paths=[str(path) for path in paths], **arguments)


def refused(app, paths, *needles, **arguments):
    text = app.call('merge_panorama', fail=True, paths=[str(path) for path in paths], **arguments)
    for needle in needles:
        assert needle in text, (needle, text)
    return text


def new_document(app, before, reply):
    """The one document the call opened: its id, checked against the reply."""
    after = documents(app)
    added = [entry for entry in after if entry not in before]
    assert len(added) == 1, ('one new document', before, after)
    assert reply['document']['id'] == added[0], (reply['document'], added)
    return added[0]


def initial_history(app, document):
    report = app.call('list_history', document_id=document)
    assert report['cursor'] == 0 and len(report['states']) == 1, (
        'the history is its initial state', report['cursor'], len(report['states']))


def composite_alpha(app, document, path):
    """Every alpha sample of a PNG `save_copy` of the composite."""
    app.call('save_copy', document_id=document, path=str(path))
    _, _, pixels = decode_png(path.read_bytes())
    return [p[3] for row in pixels for p in row]


def exported_layers(app, document, folder):
    """Each layer's pixels, from `export_layers` PNGs at canvas size."""
    folder.mkdir(parents=True, exist_ok=True)
    reply = app.call('export_layers', document_id=document, items='all', trimmed=False,
                     settings={'format': 'png', 'bit_depth': 8}, destination=str(folder),
                     pattern='layer-{index}')
    assert reply['ok'] is True, reply
    out = {}
    for entry in reply['items']:
        _, _, pixels = decode_png(Path(entry['path']).read_bytes())
        out[entry.get('name') or entry['path']] = pixels
    return out


# -------------------------------------------------------------------- cases --

def row_case(app, files, row_id, pair, layout):
    before = documents(app)
    extra = {'layout': layout} if layout else {}
    if row_id in REFUSED:
        text = refused(app, files, PREFIX, files[1].stem, **extra)
        assert text.startswith(PREFIX), text
        assert documents(app) == before, 'a refusal opens nothing'
        return dict(refused=text[:300])
    reply = merge_panorama(app, files, **extra)
    document = new_document(app, before, reply)
    try:
        assert reply['ok'] is True, reply
        align = reply['align']
        assert align['reference'] == 0 and align['unregistered'] == [], align
        assert [row['layer'] for row in align['aligned']] == [1], align['aligned']
        error = worst_error(align['aligned'][0]['homography'], pair.pairs())
        assert error <= PLAIN_BOUND, ('alignment error', error)
        assert len(reply['blend']['layers']) == 2, reply['blend']
        initial_history(app, document)
        return dict(error=round(error, 3), method=align['aligned'][0]['method'])
    finally:
        close(app, document)


def lens_case(app, files):
    before = documents(app)
    text = refused(app, files, PREFIX, LENS_SENTENCE, files[1].stem)
    assert text.startswith(PREFIX), text
    assert documents(app) == before, 'a refusal opens nothing'
    reply = merge_panorama(app, files, distortion=True)
    document = new_document(app, before, reply)
    try:
        align = reply['align']
        assert align.get('surface') == 'spherical', align.get('surface')
        coefficient = align['distortion']['coefficient']
        assert abs(coefficient - LENS_K) <= 0.01, ('coefficient', coefficient)
        assert align['unregistered'] == [], align['unregistered']
        initial_history(app, document)
        return dict(refused=text[:200], coefficient=coefficient)
    finally:
        close(app, document)


def cylindrical_fill_case(app, views, output):
    before = documents(app)
    reply = merge_panorama(app, views, layout='cylindrical', fill=True)
    document = new_document(app, before, reply)
    try:
        align = reply['align']
        assert align['reference'] == 1, align['reference']
        assert abs(align['focal'] - FOCAL) <= 0.01 * FOCAL, ('focal', align['focal'])
        listed = rows(app, document)
        assert len(listed) == 4, [row['name'] for row in listed]
        assert listed[3]['name'] == 'right (merged)', listed[3]['name']
        assert reply['fill']['layer'] == 3, reply['fill']
        alpha = composite_alpha(app, document, output / 'c-composite.png')
        assert min(alpha) == 255, ('composite alpha', min(alpha), alpha.count(0))
        initial_history(app, document)
        canvas = (reply['canvas']['width'], reply['canvas']['height'])
        return dict(focal=align['focal'], canvas=canvas, layers=len(listed),
                    covered=reply['fill']['covered_pixels'])
    finally:
        close(app, document)


def one_by_one_case(app, views, output):
    before = documents(app)
    reply = merge_panorama(app, views, layout='perspective')
    merged = new_document(app, before, reply)
    stack = load(app, views)['document']['id']
    try:
        aligned = app.call('auto_align_layers', document_id=stack, layers=[0, 1, 2], reference=1,
                           projection='perspective', sampler='bicubic')
        assert aligned['unregistered'] == [], aligned
        blended = app.call('auto_blend_layers', document_id=stack, layers=[0, 1, 2],
                           method='panorama', seamless=True, align=False)
        assert blended['changed'] is True, blended
        one = rows(app, merged)
        two = rows(app, stack)
        shape = [(row['name'], row['visible'], row['has_mask']) for row in one]
        assert shape == [(row['name'], row['visible'], row['has_mask']) for row in two], (one, two)
        summary = [app.call('get_document', document_id=d) for d in (merged, stack)]
        assert (summary[0]['width'], summary[0]['height']) == (
            summary[1]['width'], summary[1]['height']), summary
        first = exported_layers(app, merged, output / 'd-merge-panorama')
        second = exported_layers(app, stack, output / 'd-one-by-one')
        assert len(first) == 3 and len(second) == 3, (list(first), list(second))
        for a, b in zip(first.values(), second.values()):
            assert a == b, 'identical pixels per layer'
        alpha = composite_alpha(app, merged, output / 'd-composite.png')
        assert min(alpha) == 0, 'the unfilled panorama has transparent pixels'
        return dict(canvas=(summary[0]['width'], summary[0]['height']), layers=shape,
                    transparent=alpha.count(0))
    finally:
        close(app, merged)
        close(app, stack)


def disjoint_case(app, files, fill):
    before = documents(app)
    reply = merge_panorama(app, files, fill=fill)
    document = new_document(app, before, reply)
    try:
        align = reply['align']
        assert align['reference'] == 1, align
        assert [row['layer'] for row in align['unregistered']] == [2], align['unregistered']
        assert 'crop' in reply.get('note', ''), reply.get('note')
        assert len(reply['blend']['layers']) == 2, reply['blend']
        listed = rows(app, document)
        names = [(row['name'], row['visible']) for row in listed]
        if fill:
            assert names == [('left', True), ('centre', True), ('centre (merged)', True),
                             ('crop', False)], names
        else:
            assert names == [('left', True), ('centre', True), ('crop', False)], names
        return dict(layers=names, reason=align['unregistered'][0]['reason'])
    finally:
        close(app, document)


def refusals_case(app, output):
    missing = [output / 'missing-a.png', output / 'missing-b.png']
    before = documents(app)
    text = refused(app, missing, blend=False, fill=True)
    assert text == 'Content-Aware Fill Transparent Areas needs Blend Images Together.', text
    lens = refused(app, missing, 'Collage', layout='collage', vignette=True)
    assert documents(app) == before, 'no document opened'
    return dict(fill=text, lens=lens)


def action_case(app, views, canvas, layers):
    name = 'v1 merge panorama'
    single = 'v1 merge panorama step'
    before = documents(app)
    app.call('start_recording', discard=True)
    reply = merge_panorama(app, views, layout='cylindrical', fill=True)
    made = new_document(app, before, reply)
    app.call('new_layer', document_id=made, name='edit')
    recorded = app.call('stop_recording', name=name)
    close(app, made)
    tools = [step['tool'] for step in recorded['steps']]
    assert tools == ['merge_panorama', '__unrecorded'], recorded['steps']
    assert recorded['steps'][1]['arguments'].get('action') == PLACEHOLDER, recorded['steps'][1]
    step = recorded['steps'][0]
    app.call('save_action', name=single, overwrite=True,
             steps=[{'tool': 'merge_panorama', 'arguments': step['arguments']}])
    host = load(app, views[:2])['document']['id']
    try:
        start = documents(app)
        app.call('run_action', document_id=host, name=single)
        after = documents(app)
        added = [entry for entry in after if entry not in start]
        assert len(added) == 1, ('the replay opens one document', start, after)
        summary = app.call('get_document', document_id=added[0])
        assert (summary['width'], summary['height']) == canvas, (summary['width'],
                                                                 summary['height'], canvas)
        assert len(summary['layers']) == layers, len(summary['layers'])
        close(app, added[0])
        return dict(steps=tools, arguments=step['arguments'])
    finally:
        close(app, host)


def legacy_action_case(app, views, canvas, layers):
    """A file action saved before the rename: its one step names the tool `photomerge`."""
    name = 'v1 legacy photomerge'
    actions = app.folder / 'actions'
    actions.mkdir(parents=True, exist_ok=True)
    arguments = {'paths': [str(path) for path in views], 'layout': 'cylindrical', 'fill': True}
    (actions / 'legacy-photomerge.json').write_text(json.dumps(
        {'doctored_action': 1, 'name': name,
         'steps': [{'tool': 'photomerge', 'arguments': arguments}]}) + '\n')
    host = load(app, views[:2])['document']['id']
    try:
        start = documents(app)
        app.call('run_action', document_id=host, name=name)
        after = documents(app)
        added = [entry for entry in after if entry not in start]
        assert len(added) == 1, ('the legacy action opens one document', start, after)
        summary = app.call('get_document', document_id=added[0])
        assert (summary['width'], summary['height']) == canvas, (summary['width'],
                                                                 summary['height'], canvas)
        assert len(summary['layers']) == layers, len(summary['layers'])
        close(app, added[0])
        return dict(canvas=canvas, layers=layers)
    finally:
        close(app, host)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-merge-panorama-'))
    output.mkdir(parents=True, exist_ok=True)
    fixtures = output / 'fixtures'
    grey = author(fixtures)
    table = [(row_id, pair, layout, pair_files(fixtures, grey, row_id, pair))
             for row_id, pair, layout in rows_table()]
    lens_pair = yaw_pair(600, 400, 20, lens=True)
    lens_files = pair_files(fixtures, grey, 'A-20-lens', lens_pair)
    views = [write_view(fixtures / f'{name}.png', grey, 600, 400, cylinder(600, 400,
                                                                             math.radians(yaw)))
             for name, yaw in (('left', -30.0), ('centre', 0.0), ('right', 30.0))]
    crop = write_view(fixtures / 'crop.png', grey, 600, 400, lambda x, y: (x + 1400, y + 900))
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}: {json.dumps(results[name]["detail"], default=str)}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for row_id, pair, layout, files in table:
            run(f'(a) {row_id}', lambda: row_case(app, files, row_id, pair, layout))
        run('(b) A-20-lens', lambda: lens_case(app, lens_files))
        run('(c) cylindrical + fill', lambda: cylindrical_fill_case(app, views, output))
        run('(d) perspective against the three tools',
            lambda: one_by_one_case(app, views, output))
        disjoint = [views[0], views[1], crop]
        run('(e) disjoint crop', lambda: disjoint_case(app, disjoint, False))
        run('(e) disjoint crop + fill', lambda: disjoint_case(app, disjoint, True))
        run('(f) refusals before any load', lambda: refusals_case(app, output))
        detail = results.get('(c) cylindrical + fill', {}).get('detail') or {}
        run('(g) actions', lambda: action_case(app, views, tuple(detail['canvas']),
                                               detail['layers']))
        run('(h) legacy action', lambda: legacy_action_case(app, views, tuple(detail['canvas']),
                                                            detail['layers']))
    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
