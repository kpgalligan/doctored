#!/usr/bin/env python3
"""The Phase 12E-1c MCP drive: guides and the ruler origin at high depth.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. The instance is owned by this file (`HostApp`: a free port, a
dummy API key and a PRIVATE directory for every preference library the app
reads) and is terminated by its own context manager; no Doctored process
this file did not start is ever signalled.

TOLERANCE: EXACT. Guide positions and the ruler origin are quantized to four
decimals on BOTH sides — the core quantizes what it stores, and every
expectation here is computed in Python and rounded the same way — so there is
nothing for a tolerance to absorb and none is granted.

- **G1 the five tools answer at 16 bits.** A 64x48 `new_document depth:16`
  takes three `add_guide` calls (vertical 10.5, vertical 20, horizontal 7.25)
  and one `set_ruler_origin` (3, 4); `list_guides` and `get_document` both
  report them; four `undo` empty the list and four `redo` restore it.
- **G2 every typed canvas op moves them, by formulas restated here.** Crop
  (4, 2, 40, 30), `rotate degrees:90`, `flip axis:horizontal` and two
  `image_size` calls, each compared against `doc_guide.rs`'s arithmetic
  written out in this file, each undone in one step.
- **G3 they persist.** `save_copy` to `.rz` then `open_document` reports the
  same list; `write_recovery` then opening the checkpoint's own `image.rz`
  reports the same list; and a guide-less 16-bit save contains no `LGDE`.
- **G4 the same at 32-bit float,** on the committed float fixture: add,
  remove, clear, origin, save/reopen, recovery and undo.
- **G5 a depth conversion carries them.** An 8-bit document with guides and a
  moved origin keeps both through `convert_precision mode:rgba16
  flatten:true`.

Nothing here derives an expectation from Doctored: every table below is
written in this file.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the expectations, fixed BEFORE any measurement ----------------------
CANVAS = (64, 48)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

# G1's three guides and origin, in the order they are added. The core sorts
# by (orientation, position) — horizontal before vertical — so the REPORTED
# order is the sorted one, which this file states separately.
ADDED = [('vertical', 10.5), ('vertical', 20.0), ('horizontal', 7.25)]
ORIGIN = (3.0, 4.0)
SORTED_START = [('horizontal', 7.25), ('vertical', 10.5), ('vertical', 20.0)]

# G2's crop window and the two Image Size targets.
CROP = (4, 2, 40, 30)
# Two Image Size targets, both different from the 64x48 canvas and from each
# other: one that shrinks x while growing y, one that does the opposite. A
# resize to the size the document already has is a no-op the app refuses, so
# neither target may be the canvas.
SIZES = [(32, 96), (96, 36)]


# ------------------------------------------------- the independent oracle --

def q4(value):
    """The core's four-decimal quantization, restated here."""
    return round(value + 0.0, 4)


def ordered(guides):
    """The model's own order: by orientation (horizontal first, as the enum
    numbers them 0 and 1), then by position."""
    rank = {'horizontal': 0, 'vertical': 1}
    return sorted(((o, q4(p)) for o, p in guides), key=lambda g: (rank[g[0]], g[1]))


def cropped(guides, origin, x, y, w, h):
    """`doc_guide::cropped_guides` / `cropped_origin`: shift by (-x, -y) and
    DROP what left the window; the origin CLAMPS instead."""
    kept = []
    for orientation, position in guides:
        shift = x if orientation == 'vertical' else y
        extent = w if orientation == 'vertical' else h
        moved = q4(position - shift)
        if 0.0 <= moved <= extent:
            kept.append((orientation, moved))
    return ordered(kept), (clamp(q4(origin[0] - x), w), clamp(q4(origin[1] - y), h))


def turned(guides, origin, op, w, h):
    """`doc_guide::geometry_guides` / `geometry_origin`, with `w`/`h` the OLD
    canvas — the five maps written out from the geometry itself."""
    out = []
    for orientation, position in guides:
        if op == 'rotate90':
            pair = ('horizontal', position) if orientation == 'vertical' \
                else ('vertical', h - position)
        elif op == 'rotate180':
            pair = (orientation, (w if orientation == 'vertical' else h) - position)
        elif op == 'rotate270':
            pair = ('horizontal', w - position) if orientation == 'vertical' \
                else ('vertical', position)
        elif op == 'flip_horizontal':
            pair = ('vertical', w - position) if orientation == 'vertical' \
                else ('horizontal', position)
        elif op == 'flip_vertical':
            pair = ('vertical', position) if orientation == 'vertical' \
                else ('horizontal', h - position)
        else:
            raise AssertionError(op)
        out.append(pair)
    if op == 'rotate90':
        moved, size = (h - origin[1], origin[0]), (h, w)
    elif op == 'rotate180':
        moved, size = (w - origin[0], h - origin[1]), (w, h)
    elif op == 'rotate270':
        moved, size = (origin[1], w - origin[0]), (h, w)
    elif op == 'flip_horizontal':
        moved, size = (w - origin[0], origin[1]), (w, h)
    else:
        moved, size = (origin[0], h - origin[1]), (w, h)
    new_w, new_h = size
    clamped = [(o, clamp(q4(p), new_w if o == 'vertical' else new_h)) for o, p in out]
    return ordered(clamped), (clamp(q4(moved[0]), new_w), clamp(q4(moved[1]), new_h))


def resized(guides, origin, old, new):
    """`doc_guide::resized_guides` / `resized_origin`: scale by the canvas's
    own factors WITHOUT rounding, then clamp."""
    fx, fy = new[0] / old[0], new[1] / old[1]
    out = []
    for orientation, position in guides:
        factor = fx if orientation == 'vertical' else fy
        extent = new[0] if orientation == 'vertical' else new[1]
        out.append((orientation, clamp(q4(position * factor), extent)))
    return ordered(out), (clamp(q4(origin[0] * fx), new[0]), clamp(q4(origin[1] * fy), new[1]))


def clamp(value, extent):
    return min(max(value, 0.0), float(extent))


# -------------------------------------------------------------- utilities --

def guides_of(app, document):
    reply = app.call('list_guides', document_id=document)
    return [(g['orientation'], q4(float(g['position']))) for g in reply['guides']]


def origin_of(app, document):
    ruler = app.call('list_guides', document_id=document)['ruler']
    return (q4(float(ruler['origin_x'])), q4(float(ruler['origin_y'])))


def document_view(app, document):
    """The same two things as `get_document` reports them, so the two
    reporters are checked against each other as well as against the oracle."""
    summary = app.call('get_document', document_id=document)
    rows = summary.get('guides') or []
    ruler = summary.get('ruler') or {}
    return ([(g['orientation'], q4(float(g['position']))) for g in rows],
            (q4(float(ruler.get('origin_x', 0))), q4(float(ruler.get('origin_y', 0)))))


def seed(app, document):
    """The three guides and the moved origin, as four separate edits — so the
    four `undo` steps G1 asserts have four things to undo."""
    for orientation, position in ADDED:
        app.call('add_guide', document_id=document, orientation=orientation, position=position)
    app.call('set_ruler_origin', document_id=document, x=ORIGIN[0], y=ORIGIN[1])


def state(app, document):
    return guides_of(app, document), origin_of(app, document)


# ------------------------------------------------------------------ gates --

def gate_g1(app, receipts):
    gate = 'g1_the_five_tools_at_16_bits'
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=16,
                       background='white')
    document = created['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    seed(app, document)

    listed, listed_origin = state(app, document)
    assert listed == SORTED_START, (listed, SORTED_START)
    assert listed_origin == ORIGIN, (listed_origin, ORIGIN)
    summary, summary_origin = document_view(app, document)
    assert summary == SORTED_START, (summary, SORTED_START)
    assert summary_origin == ORIGIN, (summary_origin, ORIGIN)

    for _ in range(4):
        app.call('undo', document_id=document)
    assert guides_of(app, document) == [], 'four undo steps empty the list'
    assert origin_of(app, document) == (0.0, 0.0), 'and put the origin back'
    for _ in range(4):
        app.call('redo', document_id=document)
    assert state(app, document) == (SORTED_START, ORIGIN), 'four redo steps restore them'

    receipts[gate] = {'expected': SORTED_START, 'origin': list(ORIGIN),
                      'measured': listed, 'get_document': summary}
    print('PASS g1: the five guide tools answer on a 16-bit document', flush=True)
    return document


def gate_g2(app, receipts):
    gate = 'g2_typed_canvas_geometry_moves_them'
    findings = {}
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=16,
                       background='white')
    document = created['document']['id']
    seed(app, document)
    start = state(app, document)

    def run(label, call, expected, expected_origin):
        call()
        measured = state(app, document)
        assert measured == (expected, expected_origin), (label, measured, expected,
                                                         expected_origin)
        findings[label] = {'expected': expected, 'origin': list(expected_origin),
                           'measured': measured[0]}
        app.call('undo', document_id=document)
        assert state(app, document) == start, (label, 'one undo step restores them')

    x, y, w, h = CROP
    want, want_origin = cropped(SORTED_START, ORIGIN, x, y, w, h)
    run('crop', lambda: app.call('crop', document_id=document, x=x, y=y, width=w, height=h),
        want, want_origin)

    want, want_origin = turned(SORTED_START, ORIGIN, 'rotate90', *CANVAS)
    run('rotate90', lambda: app.call('rotate', document_id=document, degrees=90),
        want, want_origin)

    want, want_origin = turned(SORTED_START, ORIGIN, 'flip_horizontal', *CANVAS)
    run('flip_horizontal',
        lambda: app.call('flip', document_id=document, axis='horizontal'), want, want_origin)

    for size in SIZES:
        want, want_origin = resized(SORTED_START, ORIGIN, CANVAS, size)
        run(f'image_size_{size[0]}x{size[1]}',
            lambda s=size: app.call('image_size', document_id=document,
                                    width=s[0], height=s[1]),
            want, want_origin)

    receipts[gate] = findings
    print('PASS g2: every typed canvas op moves the chrome by the byte formulas', flush=True)


def persistence(app, folder, document, name, before):
    """The two persistence paths this drive demands, each compared against
    the pre-write pair (12E-1 s1's pattern: a recovery is checked through the
    checkpoint's OWN bytes, since `recover_document` only offers abandoned
    copies from a previous launch)."""
    out = {}
    path = folder / f'{name}.rz'
    app.call('save_copy', document_id=document, path=str(path))
    reopened = app.call('open_document', path=str(path))['document']['id']
    after = state(app, reopened)
    assert after == before, ('save_copy', after, before)
    out['save_copy'] = {'guides': after[0], 'origin': list(after[1])}

    app.call('write_recovery', document_id=document)
    checkpoints = sorted((folder / 'recovery').rglob('image.rz'),
                         key=lambda p: p.stat().st_mtime)
    assert checkpoints, 'write_recovery wrote no native checkpoint'
    checkpoint = app.call('open_document', path=str(checkpoints[-1]))['document']['id']
    after = state(app, checkpoint)
    assert after == before, ('recovery', after, before)
    out['recovery'] = {'guides': after[0], 'origin': list(after[1])}
    return out, path


def gate_g3(app, folder, receipts):
    gate = 'g3_they_survive_save_reopen_and_recovery'
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=16,
                       background='white')
    document = created['document']['id']
    seed(app, document)
    before = state(app, document)
    findings, path = persistence(app, folder, document, 'g3', before)

    # The table is CONDITIONAL: a guide-less 16-bit save carries no LGDE.
    plain_id = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=16,
                        background='white')['document']['id']
    plain = folder / 'g3_plain.rz'
    app.call('save_copy', document_id=plain_id, path=str(plain))
    assert b'LGDE' not in plain.read_bytes(), 'a guide-less 16-bit save writes no table'
    assert b'LGDE' in path.read_bytes(), 'and a seeded one does'
    findings['conditional'] = {'plain_has_lgde': False, 'seeded_has_lgde': True}

    receipts[gate] = findings
    print('PASS g3: guides and the origin survive .rz and recovery at 16 bits', flush=True)


def gate_g4(app, folder, receipts):
    gate = 'g4_the_same_at_32_bit_float'
    if not FLOAT_FIXTURE.exists():
        receipts[gate] = {'skipped': 'the float fixture is not committed'}
        print('PASS g4: skipped, no float fixture', flush=True)
        return
    source = folder / 'g4.tiff'
    shutil.copyfile(FLOAT_FIXTURE, source)
    document = app.call('open_document', path=str(source))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    summary = app.call('get_document', document_id=document)
    width, height = int(summary['width']), int(summary['height'])

    app.call('add_guide', document_id=document, orientation='vertical', position=width / 4)
    app.call('add_guide', document_id=document, orientation='horizontal', position=height / 2)
    app.call('set_ruler_origin', document_id=document, x=1.0, y=2.0)
    expected = ordered([('vertical', width / 4), ('horizontal', height / 2)])
    before = (expected, (1.0, 2.0))
    assert state(app, document) == before, (state(app, document), before)

    findings, _ = persistence(app, folder, document, 'g4', before)

    # remove, clear and undo, each one step.
    app.call('remove_guide', document_id=document, guide=0)
    assert guides_of(app, document) == expected[1:], 'remove_guide drops the first row'
    app.call('clear_guides', document_id=document)
    assert guides_of(app, document) == [], 'clear_guides empties the list'
    app.call('undo', document_id=document)
    assert guides_of(app, document) == expected[1:], 'undo restores the cleared list'
    app.call('undo', document_id=document)
    assert state(app, document) == before, 'and the removed guide comes back'
    findings['edits'] = {'expected': expected}

    receipts[gate] = findings
    print('PASS g4: the float document carries, persists and undoes them', flush=True)


def gate_g5(app, receipts):
    gate = 'g5_a_depth_conversion_carries_them'
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']
    seed(app, document)
    before = state(app, document)
    assert before == (SORTED_START, ORIGIN), before
    app.call('convert_precision', document_id=document, mode='rgba16', flatten=True)
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    after = state(app, document)
    assert after == before, ('convert_precision', after, before)
    receipts[gate] = {'before': before[0], 'after': after[0], 'origin': list(ORIGIN)}
    print('PASS g5: 8-bit -> 16-bit conversion keeps the guides and the origin', flush=True)


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

    receipts = {'expectations': 'fixed in scripts/verify_phase12e1c_guides.py before any '
                                'measurement; the geometry oracle restates doc_guide.rs in '
                                'this file and the tolerance is EXACT, because positions are '
                                'four-decimal quantized on both sides'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        gate_g1(app, receipts)
        gate_g2(app, receipts)
        gate_g3(app, folder, receipts)
        gate_g4(app, folder, receipts)
        gate_g5(app, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
