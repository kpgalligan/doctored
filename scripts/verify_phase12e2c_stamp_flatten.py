#!/usr/bin/env python3
"""The Phase 12E-2c MCP drive: typed STAMP VISIBLE and FLATTEN IMAGE.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. ONE instance is owned by this file for every gate (`HostApp`: a
free port, a dummy API key and a PRIVATE directory for every preference
library the app reads) and is terminated by its own context manager; no
Doctored process this file did not start is ever signalled.

- **C1/C2 Stamp and Flatten at 8 bits, 16 bits AND 32-bit float.** The
  structure table this file states, the COMPOSITE unchanged within the fixed
  tolerance, ONE `undo` restoring the previous stack and `redo` reapplying it.
  The fixture's bottom layer is fully OPAQUE on purpose: the stamped layer IS
  the projection, so laying it over the unchanged stack is an identity only
  when the projection is opaque, and the float fixture is duplicated until
  its composite alpha is exactly 1.0 (asserted, not assumed).
- **C3 an all-hidden stack is a NO-OP** at 8 bits, 16 bits AND 32-bit float,
  not an error and not an undo step: the reply says `changed: false` and the
  history cursor has not moved.
- **C4 a flatten DROPS the layer comps and KEEPS the chrome**, at 8 and 16:
  `list_layer_comps` goes empty, one `undo` brings the comp back, and the
  guides and the saved channel are still there afterwards.
- **C5 both ops replay**: a recorded `stamp_visible` + `flatten_image` pair
  run through `run_action` on a fresh 16-bit document gives the same table.
- **C6 the `.rz` round trip is byte-identical**: save -> reopen -> resave has
  the same SHA-256, at 16 bits and at float, for a stamped and a flattened
  document.
- **C7 `get_precision` lists both tools** at 16 bits and at float.
- **C8 the FLOAT selector whitelist's MCP proxy.** The 9H probe has no float
  opener, so the eight 12E-1b selectors this run whitelisted at float are
  covered here instead: `set_layer_lock`, `link_layers`, `unlink_layers`,
  `merge_down` and `merge_visible` each answer non-error on a float document
  with one `undo` restoring. The float UI-source gate, missing when this
  drive was written, now exists in scripts/native-paint-probe/stampflatten.swift.

Nothing here derives an expectation from Doctored: every table, every
tolerance and every comparison below is written in this file.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the expectations and tolerances, fixed BEFORE any measurement -------
CANVAS = (64, 48)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

# C1/C2's tolerances.
#  * 8 bits: the `render` PNG's SHA-256 must be EQUAL. A stamp of an opaque
#    projection over the stack it came from is an exact identity at byte
#    depth, so anything less than equality is a defect rather than rounding.
#  * 16 bits: `magick compare -metric PAE` over the PNG16 exports, at most
#    ONE code of 65535 — the single rounding a u16 store can add.
#  * float: `sample_pixel` at the fixed probes below, relative 1e-5 against
#    the scale max(|want|, 1.0) — two orders above f32's own 6e-8 epsilon.
PAE_TOLERANCE_CODES = 1.0
FLOAT_TOLERANCE = 1e-5
PROBES = [(3, 3), (CANVAS[0] // 3, CANVAS[1] // 3), (CANVAS[0] // 2, 7)]
FLOAT_PROBES = [(2, 2), (16, 11), (31, 21), (45, 29)]
# How many times the float scene is duplicated before C1 stamps it. Each
# duplicate composites ONCE, so n copies of a layer whose alpha is at least
# 0.5 leave 1 - a <= 0.5^n; f32 spacing just below 1.0 is 5.96e-8, so the
# accumulator rounds to exactly 1.0 from n = 25 onwards. Thirty is taken for
# margin and the result is ASSERTED at every probe rather than trusted.
FLOAT_DUPLICATES = 30

# C1/C2's 8- and 16-bit fixture, bottom-first. The BOTTOM layer is opaque.
LAYERS = [
    ('Background', None, 1.0),
    ('Middle', '#3f7fbf', 0.5),
    ('Top', '#bf3f7f', 0.25),
]
# The tables this file expects, written out before anything runs.
STAMP_TABLE = [('Background', 0), ('Middle', 0), ('Top', 0), ('Stamp', 0)]
FLATTEN_TABLE = [('Background', 0)]
BASE_TABLE = [(name, 0) for name, _, _ in LAYERS]

# C7's tools.
TOOLS = ['stamp_visible', 'flatten_image']
# C8's float proxy set: the eight 12E-1b selectors reach these five tools.
FLOAT_PROXY = ['set_layer_lock', 'link_layers', 'unlink_layers', 'merge_down',
               'merge_visible']


# -------------------------------------------------------------- utilities --

def structure(app, document):
    """(name, depth) rows in the order `get_document` lists them."""
    summary = app.call('get_document', document_id=document)
    return [(layer.get('name'), int(layer.get('depth', 0))) for layer in summary['layers']]


def history(app, document):
    return app.call('list_history', document_id=document)['cursor']


def render_digest(app, document):
    """The SHA-256 of `render`'s PNG bytes. The raw RPC, because the shared
    client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def png16(app, document, path):
    app.call('export_image', document_id=document, path=str(path),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    return path


def peak_absolute_error(left, right):
    """`magick compare -metric PAE` in CODES of 65535, from a Q16 build.

    The metric is printed on stderr as "<absolute> (<normalized>)"; the
    NORMALIZED figure is read, because it is the one that does not depend on
    the build's quantum depth, and multiplied back by 65535 here.
    """
    magick = shutil.which('magick')
    assert magick, 'ImageMagick is required for the 16-bit comparison'
    completed = subprocess.run(
        [magick, 'compare', '-metric', 'PAE', str(left), str(right), 'null:'],
        capture_output=True, text=True, check=False)
    text = (completed.stderr or '') + (completed.stdout or '')
    match = re.search(r'\(([0-9.eE+-]+)\)', text)
    assert match, ('magick compare produced no normalized metric', text)
    return float(match.group(1)) * 65535.0


def probe_samples(app, document, points):
    """The COMPOSITE at each point, as four f64. A 16-bit document reports
    `center_rgba_codes` (u16) and a float one `center_rgba32`; both are
    normalized here."""
    out = []
    for x, y in points:
        reply = app.call('sample_pixel', document_id=document, x=x, y=y)
        if reply.get('center_rgba32') is not None:
            out.append([float(v) for v in reply['center_rgba32']])
        else:
            out.append([float(v) / 65535.0 for v in reply['center_rgba_codes']])
    return out


def close_float(got, want, what):
    for i, (g, w) in enumerate(zip(got, want)):
        for channel in range(4):
            scale = max(abs(w[channel]), 1.0)
            difference = abs(g[channel] - w[channel])
            assert difference <= FLOAT_TOLERANCE * scale, (
                what, i, channel, g, w, difference, FLOAT_TOLERANCE)


def build_stack(app, depth):
    """Three flat layers, the bottom one OPAQUE (see the tolerance note)."""
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='white')
    document = created['document']['id']
    for _ in range(len(LAYERS) - 1):
        app.call('duplicate_layer', document_id=document, index=0)
    for index, (name, colour, opacity) in enumerate(LAYERS):
        app.call('set_layer_properties', document_id=document, index=index, name=name,
                 opacity=opacity)
        if colour is not None:
            app.call('fill', document_id=document, x=PROBES[1][0], y=PROBES[1][1],
                     color=colour, tolerance=1.0, contiguous=False, layer=index)
    assert structure(app, document) == BASE_TABLE, structure(app, document)
    return document


def build_float(app, folder, name, duplicates=FLOAT_DUPLICATES):
    path = folder / f'{name}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, path)
    document = app.call('open_document', path=str(path))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    for _ in range(duplicates):
        app.call('duplicate_layer', document_id=document, index=0)
    return document


# ------------------------------------------------------------------ gates --

def composite_probe(app, document, depth, folder, tag):
    """Whatever "the composite" means at this depth, in the form C1/C2
    compares: a digest at 8 bits, a PNG16 path at 16, probe samples at 32."""
    if depth == 8:
        return render_digest(app, document)
    if depth == 16:
        return png16(app, document, folder / f'{tag}.png')
    return probe_samples(app, document, FLOAT_PROBES)


def composite_equal(before, after, depth, what, findings):
    if depth == 8:
        assert before == after, (what, before, after)
        findings['render_sha256'] = before
    elif depth == 16:
        worst = peak_absolute_error(before, after)
        assert worst <= PAE_TOLERANCE_CODES, (what, worst, PAE_TOLERANCE_CODES)
        findings['png16_pae_codes'] = worst
    else:
        close_float(after, before, what)
        findings['float_probes'] = {'before': before, 'after': after}


def run_op(app, document, depth, folder, tag, call, expected_rows, findings):
    """One op: the table this file states, the composite unchanged within
    tolerance, one undo restoring and a redo reapplying."""
    start = structure(app, document)
    before = composite_probe(app, document, depth, folder, f'{tag}_before')
    cursor = history(app, document)
    reply = call()
    rows = structure(app, document)
    assert rows == expected_rows, (tag, rows, expected_rows)
    assert history(app, document) == cursor + 1, (tag, 'one history step')
    after = composite_probe(app, document, depth, folder, f'{tag}_after')
    composite_equal(before, after, depth, tag, findings)
    app.call('undo', document_id=document)
    assert structure(app, document) == start, (tag, 'undo', structure(app, document))
    app.call('redo', document_id=document)
    assert structure(app, document) == expected_rows, (tag, 'redo')
    app.call('undo', document_id=document)
    assert structure(app, document) == start, (tag, 'undo after redo')
    findings.update(rows=[list(r) for r in rows], start=[list(r) for r in start],
                    reply_ok=bool(reply.get('ok', True)))
    return findings


def gate_c1(app, folder, receipts):
    gate = 'c1_stamp_visible_at_every_depth'
    findings = {'tolerances': {'8': 'render PNG sha256 EQUAL',
                               '16': f'magick PAE <= {PAE_TOLERANCE_CODES} code of 65535',
                               '32': f'sample_pixel relative {FLOAT_TOLERANCE}'}}
    for depth in (8, 16):
        document = build_stack(app, depth)
        findings[str(depth)] = run_op(
            app, document, depth, folder, f'c1_{depth}',
            lambda d=document: app.call('stamp_visible', document_id=d, above=2, name='Stamp'),
            STAMP_TABLE, {})
    if FLOAT_FIXTURE.exists():
        document = build_float(app, folder, 'c1_float')
        rows = structure(app, document)
        alphas = [sample[3] for sample in probe_samples(app, document, FLOAT_PROBES)]
        assert all(alpha == 1.0 for alpha in alphas), (
            'the float composite is not opaque, so a stamp is not an identity', alphas)
        top = len(rows) - 1
        expected = rows + [('Stamp', 0)]
        findings['32'] = run_op(
            app, document, 32, folder, 'c1_float',
            lambda d=document, t=top: app.call('stamp_visible', document_id=d, above=t,
                                               name='Stamp'),
            expected, {'composite_alpha': alphas})
    else:
        findings['32'] = 'NOT RUN: the 11F float fixture is absent'
    receipts[gate] = findings
    print('PASS c1: stamp_visible answers at 8, 16 and 32 bits and changes no composite',
          flush=True)


def gate_c2(app, folder, receipts):
    gate = 'c2_flatten_image_at_every_depth'
    findings = {}
    for depth in (8, 16):
        document = build_stack(app, depth)
        findings[str(depth)] = run_op(
            app, document, depth, folder, f'c2_{depth}',
            lambda d=document: app.call('flatten_image', document_id=d),
            FLATTEN_TABLE, {})
    if FLOAT_FIXTURE.exists():
        document = build_float(app, folder, 'c2_float')
        findings['32'] = run_op(
            app, document, 32, folder, 'c2_float',
            lambda d=document: app.call('flatten_image', document_id=d),
            FLATTEN_TABLE, {})
    else:
        findings['32'] = 'NOT RUN: the 11F float fixture is absent'
    receipts[gate] = findings
    print('PASS c2: flatten_image answers at 8, 16 and 32 bits and changes no composite',
          flush=True)


def gate_c3(app, folder, receipts):
    gate = 'c3_an_all_hidden_stack_is_a_no_op'
    findings = {}
    for depth in (8, 16):
        document = build_stack(app, depth)
        for index in range(len(LAYERS)):
            app.call('set_layer_properties', document_id=document, index=index, visible=False)
        cursor = history(app, document)
        reply = app.call('stamp_visible', document_id=document, above=2, name='Stamp')
        assert reply.get('changed') is False, (depth, reply)
        assert structure(app, document) == BASE_TABLE, (depth, structure(app, document))
        assert history(app, document) == cursor, (depth, 'the no-op moved the history')
        findings[str(depth)] = {'note': reply.get('note'), 'cursor': cursor}
    # The float floor is the SAME `Content::Raster` count, so it gets the same
    # gate: one record, hidden, and the reply must still be a no-op.
    if FLOAT_FIXTURE.exists():
        document = build_float(app, folder, 'c3_float', duplicates=0)
        rows = structure(app, document)
        for index in range(len(rows)):
            app.call('set_layer_properties', document_id=document, index=index, visible=False)
        cursor = history(app, document)
        reply = app.call('stamp_visible', document_id=document, above=len(rows) - 1,
                         name='Stamp')
        assert reply.get('changed') is False, ('32', reply)
        assert structure(app, document) == rows, ('32', structure(app, document), rows)
        assert history(app, document) == cursor, ('32', 'the no-op moved the history')
        findings['32'] = {'note': reply.get('note'), 'cursor': cursor}
    else:
        findings['32'] = 'NOT RUN: the 11F float fixture is absent'
    receipts[gate] = findings
    print('PASS c3: a stamp with nothing to stamp is a no-op at 8, 16 and 32 bits',
          flush=True)


def gate_c4(app, folder, receipts):
    gate = 'c4_a_flatten_drops_the_comps_and_keeps_the_chrome'
    findings = {}
    for depth in (8, 16):
        document = build_stack(app, depth)
        app.call('add_guide', document_id=document, orientation='vertical', position=12.0)
        app.call('select_all', document_id=document)
        app.call('save_selection', document_id=document, name='Coverage')
        app.call('deselect', document_id=document)
        app.call('save_layer_comp', document_id=document, name='A')
        before_comps = app.call('list_layer_comps', document_id=document)['comps']
        assert len(before_comps) == 1, (depth, before_comps)
        guides_before = app.call('list_guides', document_id=document)['guides']
        channels_before = [c['name'] for c in
                           app.call('list_channels', document_id=document)['channels']]
        assert 'Coverage' in channels_before, (depth, channels_before)

        app.call('flatten_image', document_id=document)
        after_comps = app.call('list_layer_comps', document_id=document)['comps']
        assert after_comps == [], (depth, after_comps)
        guides_after = app.call('list_guides', document_id=document)['guides']
        assert guides_after == guides_before, (depth, guides_after, guides_before)
        channels_after = [c['name'] for c in
                          app.call('list_channels', document_id=document)['channels']]
        assert channels_after == channels_before, (depth, channels_after, channels_before)

        app.call('undo', document_id=document)
        restored = app.call('list_layer_comps', document_id=document)['comps']
        assert len(restored) == 1, (depth, restored)
        findings[str(depth)] = {'comps_before': len(before_comps), 'comps_after': 0,
                                'comps_restored': len(restored),
                                'guides': guides_after, 'channels': channels_after}
    receipts[gate] = findings
    print('PASS c4: a flatten drops the comps and keeps the guides and channels', flush=True)


def gate_c5(app, folder, receipts):
    gate = 'c5_both_ops_replay_through_run_action'
    document = build_stack(app, 16)
    app.call('start_recording', document_id=document)
    app.call('stamp_visible', document_id=document, above=2, name='Stamp')
    app.call('flatten_image', document_id=document)
    recorded = structure(app, document)
    app.call('stop_recording', document_id=document, name='StampThenFlatten')
    assert recorded == FLATTEN_TABLE, recorded

    fresh = build_stack(app, 16)
    app.call('run_action', document_id=fresh, name='StampThenFlatten')
    replayed = structure(app, fresh)
    assert replayed == recorded, (replayed, recorded)
    receipts[gate] = {'recorded': [list(r) for r in recorded],
                      'replayed': [list(r) for r in replayed]}
    print('PASS c5: a recorded stamp + flatten replays on a fresh 16-bit document',
          flush=True)


def gate_c6(app, folder, receipts):
    gate = 'c6_the_rz_round_trip_is_byte_identical'
    findings = {}

    def round_trip(document, tag):
        first = folder / f'{tag}_a.rz'
        second = folder / f'{tag}_b.rz'
        app.call('save_copy', document_id=document, path=str(first))
        reopened = app.call('open_document', path=str(first))['document']['id']
        app.call('save_copy', document_id=reopened, path=str(second))
        left = hashlib.sha256(first.read_bytes()).hexdigest()
        right = hashlib.sha256(second.read_bytes()).hexdigest()
        assert left == right, (tag, left, right)
        return {'sha256': left, 'rows': [list(r) for r in structure(app, reopened)]}

    document = build_stack(app, 16)
    app.call('stamp_visible', document_id=document, above=2, name='Stamp')
    findings['16_stamped'] = round_trip(document, 'c6_16_stamped')
    document = build_stack(app, 16)
    app.call('flatten_image', document_id=document)
    findings['16_flattened'] = round_trip(document, 'c6_16_flattened')

    if FLOAT_FIXTURE.exists():
        document = build_float(app, folder, 'c6_float', duplicates=1)
        app.call('stamp_visible', document_id=document, above=1, name='Stamp')
        findings['32_stamped'] = round_trip(document, 'c6_32_stamped')
        document = build_float(app, folder, 'c6_float2', duplicates=1)
        app.call('flatten_image', document_id=document)
        findings['32_flattened'] = round_trip(document, 'c6_32_flattened')
    else:
        findings['32'] = 'NOT RUN: the 11F float fixture is absent'
    receipts[gate] = findings
    print('PASS c6: a stamped and a flattened .rz reopen and resave byte-identically',
          flush=True)


def gate_c7(app, folder, receipts):
    gate = 'c7_get_precision_lists_both_tools'
    findings = {}
    document = build_stack(app, 16)
    report = app.call('get_precision', document_id=document)
    tools = set(report['supported_tools']) | set(report.get('shared_tools', []))
    missing = set(TOOLS) - tools
    assert not missing, ('16', sorted(missing))
    findings['16'] = sorted(TOOLS)
    if FLOAT_FIXTURE.exists():
        document = build_float(app, folder, 'c7_float', duplicates=0)
        report = app.call('get_precision', document_id=document)
        tools = set(report['supported_tools']) | set(report.get('shared_tools', []))
        missing = set(TOOLS) - tools
        assert not missing, ('32', sorted(missing))
        findings['32'] = sorted(TOOLS)
    else:
        findings['32'] = 'NOT RUN: the 11F float fixture is absent'
    receipts[gate] = findings
    print('PASS c7: get_precision lists stamp_visible and flatten_image at both depths',
          flush=True)


def gate_c8(app, folder, receipts):
    gate = 'c8_the_float_selector_whitelists_mcp_proxy'
    if not FLOAT_FIXTURE.exists():
        receipts[gate] = 'NOT RUN: the 11F float fixture is absent'
        print('NOT RUN c8: the 11F float fixture is absent', flush=True)
        return
    findings = {'why': 'the 9H probe has no float opener, so the eight 12E-1b selectors '
                       'this run whitelisted at float are proved through their MCP twins '
                       'here; the UI-source gap is recorded under 12E-3'}
    document = build_float(app, folder, 'c8_float', duplicates=1)
    start = structure(app, document)

    def once(tool, expect, **arguments):
        cursor = history(app, document)
        reply = app.call(tool, document_id=document, **arguments)
        assert history(app, document) == cursor + 1, (tool, 'one history step')
        value = expect(reply)
        app.call('undo', document_id=document)
        assert structure(app, document) == start, (tool, 'undo', structure(app, document))
        return value

    summary_locks = once('set_layer_lock',
                         lambda _: app.call('get_document',
                                            document_id=document)['layers'][0].get('locks'),
                         layer=0, locks=['pixels'])
    assert summary_locks, ('set_layer_lock', 'the reply carries no locks', summary_locks)
    findings['set_layer_lock'] = summary_locks

    link = once('link_layers',
                lambda _: app.call('get_document',
                                   document_id=document)['layers'][0].get('link'),
                layers=[0, 1])
    assert link, ('link_layers', 'no link id', link)
    findings['link_layers'] = link

    # `unlink_layers` needs a link to clear, so it is measured from a linked
    # state and the two steps are undone together.
    cursor = history(app, document)
    app.call('link_layers', document_id=document, layers=[0, 1])
    app.call('unlink_layers', document_id=document, layers=[0, 1])
    assert not app.call('get_document', document_id=document)['layers'][0].get('link')
    app.call('undo', document_id=document)
    app.call('undo', document_id=document)
    assert history(app, document) == cursor, 'the link/unlink pair did not undo'
    findings['unlink_layers'] = 'cleared'

    findings['merge_down'] = once('merge_down', lambda _: structure(app, document), index=1)
    findings['merge_visible'] = once('merge_visible', lambda _: structure(app, document))
    findings['merge_down'] = [list(r) for r in findings['merge_down']]
    findings['merge_visible'] = [list(r) for r in findings['merge_visible']]
    findings['tools'] = FLOAT_PROXY
    receipts[gate] = findings
    print('PASS c8: the float whitelist\'s five MCP twins all answer with one undo',
          flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'expectations': 'fixed in scripts/verify_phase12e2c_stamp_flatten.py before '
                                'any measurement: the three tables, the 8-bit digest '
                                'equality, the 1-code PAE bound at 16 bits and the relative '
                                '1e-5 float bound are all written down before the first '
                                'call'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        gate_c1(app, folder, receipts)
        gate_c2(app, folder, receipts)
        gate_c3(app, folder, receipts)
        gate_c4(app, folder, receipts)
        gate_c5(app, folder, receipts)
        gate_c6(app, folder, receipts)
        gate_c7(app, folder, receipts)
        gate_c8(app, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
