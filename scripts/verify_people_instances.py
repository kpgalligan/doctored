#!/usr/bin/env python3
"""Select People over MCP: `model "people"` on `inspect_subjects` and
`select_subjects`, at 8 bits and at 32-bit float beside a 16-bit twin.

Runs on an owned `phase5_support.App` (the Debug app by default; see
`scripts/app_under_test.py`) on its own scratch MCP socket. The
four-adult group photo is read IN PLACE from
`~/devel/doctored-fixtures/multiple-people/` (`--people-dir`); it is one of
the user's private photos, so `--output` must lie outside the repository and
nothing derived from it is ever committed or published.

The oracle is Vision's own answer: `scripts/people-instances-oracle` is built
into the output folder and run on the same file in the same run, with no app
code (controller ruling 1, 2026-10-12). Every expectation is fixed here before
anything is measured, and never widened:

- 8 bits, part 1: `inspect_subjects {model:"people"}` lists as many people as
  the oracle has instances, with each `bounds.x` larger than the one before.
  At the centre pixel of every oracle core cell where `select_people`'s
  coverage is hard, person k (`select_subjects {instances:[k]}`) is hard on at
  least RECALL of the oracle's k-th instance from the left and on at most
  LEAK of the other instances' cells, each core holding at least MIN_CELLS.
- 8 bits, part 2: faces 1, 2 and 4 (FACES) are each hard in exactly one
  person, three distinct people numbered left to right; face 3 is hard in
  `select_people` (it is not pinned to a person: Vision merges two of the
  four adults). Person k is at least INSIDE at its own pinned face and at
  most OUTSIDE at the other pinned faces. `select_subjects` at face 2's
  point selects that face's person, one history step; `undo` empties the
  selection.
- fur.png: `inspect_subjects` and `select_subjects` with `model "people"`
  refuse with NO_PEOPLE, the selection stays empty; `model "faces"` refuses
  with BAD_MODEL.
- 32 and 16 bits: the 8-bit document's `save_copy` PNG becomes a float TIFF
  (each code decoded by IEC 61966-2.1, sRGB profile) and a 16-bit PNG twin,
  as `verify_phase11f_float.gate_f43` makes them. Both open at their depth,
  list the oracle's count of people, and each person's hard set on a
  STRIDE lattice has IoU at least IOU against the twin's same person.

Usage: python3 scripts/verify_people_instances.py --output DIR
[--people-dir DIR] [--photos-dir DIR]. Exit 0 only when every case passes.
"""
import argparse
import array
import base64
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase11e_linear import png16, read_png  # noqa: E402
from verify_phase11f_float import SYSTEM_SRGB, srgb_encode, write_float_tiff  # noqa: E402

# ---- fixed before anything is measured, never widened ----------------------
PHOTO = 'c482d87d-45dd-4e96-b2f3-71ed170a6259.JPG'
SIZE = (1512, 2016)
FACES = [(0.319, 0.297), (0.444, 0.328), (0.583, 0.328), (0.688, 0.344)]
PINNED = [0, 1, 3]          # faces pinned to distinct people; face 3 (index 2) to All People only
RECALL, LEAK, MIN_CELLS = 0.90, 0.05, 20
INSIDE, OUTSIDE = 32768, 655  # coverage16: at least half; at most 1 %
STRIDE, IOU = 8, 0.9
NO_PEOPLE = 'No people were found in this image.'
BAD_MODEL = 'model must be "objects" or "people".'


def coverage(app, document, points):
    """`get_selection`'s `coverage16` at `points`, 4096 a call (its limit)."""
    out = []
    for start in range(0, len(points), 4096):
        reply = app.call('get_selection', document_id=document,
                         points=[{'x': x, 'y': y} for x, y in points[start:start + 4096]])
        out.extend(row['coverage16'] for row in reply.get('samples') or [])
    assert len(out) == len(points), (len(out), len(points))
    return out


def oracle(output, photo):
    binary = output / 'people-instances-oracle'
    subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
                    str(ROOT / 'scripts/people-instances-oracle/main.swift'), '-o', str(binary)],
                   check=True)
    reply = json.loads(subprocess.run([str(binary), str(photo)], check=True, capture_output=True,
                                      text=True).stdout)
    for instance in reply['instances']:
        instance['core'] = base64.b64decode(instance['core'])
    return reply


def history(app, document):
    return app.call('list_history', document_id=document)['cursor']


def face_pixel(face, width, height):
    return (min(width - 1, int(face[0] * width)), min(height - 1, int(face[1] * height)))


def person_masks(app, document, count, points):
    masks = []
    for k in range(1, count + 1):
        app.call('select_subjects', document_id=document, model='people', instances=[k])
        masks.append(coverage(app, document, points))
    return masks


def case_8bit(app, photo, seen, report):
    doc = app.open(photo)
    state = app.call('get_document', document_id=doc)
    width, height = state['width'], state['height']
    detail = dict(size=[width, height])
    rows = {'size': (width, height) == SIZE}
    inspect = app.call('inspect_subjects', document_id=doc, model='people')
    count = inspect['instance_count']
    xs = [row['bounds']['x'] for row in inspect['instances']]
    detail.update(count=count, oracle_count=len(seen['instances']), bounds_x=xs,
                  model=inspect.get('model'), segmenter=inspect.get('segmenter'))
    rows['count'] = count == len(seen['instances']) and inspect.get('model') == 'people'
    rows['bounds_x'] = all(a < b for a, b in zip(xs, xs[1:]))

    # Part 1: the oracle's core cells, sampled at their centre pixels.
    mw, mh = seen['mask_width'], seen['mask_height']
    cells = sorted({cell for instance in seen['instances']
                    for cell, on in enumerate(instance['core']) if on})
    centre = [(min(width - 1, int((cell % mw + 0.5) * width / mw)),
               min(height - 1, int((cell // mw + 0.5) * height / mh))) for cell in cells]
    faces = [face_pixel(face, width, height) for face in FACES]
    points = centre + faces
    app.call('select_people', document_id=doc)
    aggregate = coverage(app, doc, points)
    masks = person_masks(app, doc, count, points)
    owner_of = {}
    for index, instance in enumerate(seen['instances']):
        for cell, on in enumerate(instance['core']):
            if on:
                owner_of[cell] = index
    compared = []
    rows['oracle'] = count == len(seen['instances'])
    for k in range(min(count, len(seen['instances']))):
        own = own_hit = other = other_hit = 0
        for i, cell in enumerate(cells):
            if aggregate[i] < INSIDE:
                continue
            if owner_of[cell] == k:
                own += 1
                own_hit += masks[k][i] >= INSIDE
            else:
                other += 1
                other_hit += masks[k][i] >= INSIDE
        recall = own_hit / own if own else 0.0
        leak = other_hit / other if other else 0.0
        compared.append(dict(id=seen['instances'][k]['id'], recall=round(recall, 4),
                             leak=round(leak, 4), cells=own))
        rows['oracle'] &= own >= MIN_CELLS and recall >= RECALL and leak <= LEAK
    detail['oracle'] = compared

    # Part 2: the face pins.
    base = len(centre)
    at_faces = [[mask[base + f] for f in range(len(FACES))] for mask in masks]
    hits = []
    for f in PINNED:
        holders = [k + 1 for k in range(count) if at_faces[k][f] >= INSIDE]
        hits.append(holders[0] if len(holders) == 1 else None)
    detail.update(face_hits=hits, face_coverage=at_faces, aggregate_faces=aggregate[base:])
    rows['faces'] = (None not in hits and all(a < b for a, b in zip(hits, hits[1:])))
    if rows['faces']:
        for f, person in zip(PINNED, hits):
            others = [g for g in PINNED if g != f]
            rows['faces'] &= (at_faces[person - 1][f] >= INSIDE
                              and all(at_faces[person - 1][g] <= OUTSIDE for g in others))
    rows['face_3_all_people'] = aggregate[base + 2] >= INSIDE

    # A point, one history step, one undo.
    app.call('deselect', document_id=doc)
    before = history(app, doc)
    reply = app.call('select_subjects', document_id=doc, model='people', point=list(faces[1]))
    after = history(app, doc)
    face2 = coverage(app, doc, [faces[f] for f in PINNED])
    expected = hits[1] if len(hits) > 1 else None
    rows['point'] = (reply.get('instances') == [expected] and after == before + 1
                     and face2[1] >= INSIDE and face2[0] <= OUTSIDE and face2[2] <= OUTSIDE)
    app.call('undo', document_id=doc)
    empty = app.call('get_selection', document_id=doc)['selection_empty']
    rows['undo'] = empty is True and history(app, doc) == before
    detail['point'] = dict(reply=reply, cursor=[before, after], coverage=face2, undo_empty=empty)
    report['cases']['8-bit'] = dict(rows=rows, detail=detail)
    return doc, width, height, count


def case_fur(app, fur, report):
    doc = app.open(fur)
    rows, detail = {}, {}
    inspect = app.call('inspect_subjects', document_id=doc, model='people', fail=True)
    select = app.call('select_subjects', document_id=doc, model='people', fail=True)
    empty = app.call('get_selection', document_id=doc)['selection_empty']
    bad = app.call('inspect_subjects', document_id=doc, model='faces', fail=True)
    rows['no_people'] = inspect == NO_PEOPLE and select == NO_PEOPLE and empty is True
    rows['bad_model'] = bad == BAD_MODEL
    detail.update(inspect=inspect, select=select, selection_empty=empty, bad_model=bad)
    report['cases']['fur'] = dict(rows=rows, detail=detail)


def decode(code):
    c = code / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def case_float(app, output, doc, width, height, count, report):
    flat = output / 'c482-8bit.png'
    app.call('save_copy', document_id=doc, path=str(flat), format='png')
    data = flat.read_bytes()
    assert data[24] == 8, 'an 8-bit save_copy PNG'
    png_width, png_height, pixels = read_png(data)
    assert (png_width, png_height) == (width, height), (png_width, png_height)
    # Each code's linear value, rounded through f32 as the file stores it.
    linear_of = array.array('f', [decode(code) for code in range(256)])
    code16_of = [round(65535 * srgb_encode(v)) for v in linear_of]
    linear = array.array('f')
    codes = array.array('H')
    for pixel in pixels:
        r, g, b = pixel[0], pixel[1], pixel[2]
        linear.extend((linear_of[r], linear_of[g], linear_of[b], 1.0))
        codes.extend((code16_of[r], code16_of[g], code16_of[b], 65535))
    del pixels
    float_doc = app.open(write_float_tiff(output / 'c482-float.tiff', width, height, linear,
                                          SYSTEM_SRGB.read_bytes()))
    del linear
    png16(output / 'c482-twin16.png', width, height, codes)
    del codes
    twin = app.open(output / 'c482-twin16.png')
    precisions = [app.call('get_document', document_id=d).get('precision')
                  for d in (float_doc, twin)]
    counts = [app.call('inspect_subjects', document_id=d, model='people')['instance_count']
              for d in (float_doc, twin)]
    lattice = [(x, y) for y in range(0, height, STRIDE) for x in range(0, width, STRIDE)]
    deep = person_masks(app, float_doc, counts[0], lattice)
    wide = person_masks(app, twin, counts[1], lattice)
    ious = []
    for a, b in zip(deep, wide):
        both = sum(1 for p, q in zip(a, b) if p >= INSIDE and q >= INSIDE)
        either = sum(1 for p, q in zip(a, b) if p >= INSIDE or q >= INSIDE)
        ious.append(round(both / either, 4) if either else 0.0)
    rows = dict(precision=precisions == ['rgba32_float', 'rgba16'],
                count=counts == [count, count],
                iou=len(ious) == count and all(v >= IOU for v in ious))
    report['cases']['32-bit'] = dict(rows=rows, detail=dict(precisions=precisions, counts=counts,
                                                            iou=ious, lattice=len(lattice)))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--people-dir', type=Path,
                        default=Path.home() / 'devel/doctored-fixtures/multiple-people')
    parser.add_argument('--photos-dir', type=Path,
                        default=Path.home() / 'devel/doctored-fixtures/phase6/photos')
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository (the photos are private)')
    output.mkdir(parents=True, exist_ok=True)
    photo = (arguments.people_dir.expanduser() / PHOTO).resolve()
    fur = (arguments.photos_dir.expanduser() / 'fur.png').resolve()
    report = dict(bounds=dict(recall=RECALL, leak=LEAK, min_cells=MIN_CELLS, inside=INSIDE,
                              outside=OUTSIDE, stride=STRIDE, iou=IOU), cases={})
    seen = oracle(output, photo)
    report['oracle'] = dict(size=[seen['width'], seen['height']],
                            mask=[seen['mask_width'], seen['mask_height']],
                            instances=[dict(id=i['id'], cells=i['cells'], cx=round(i['cx'], 4))
                                       for i in seen['instances']])
    try:
        with App(output / 'app', output / 'recovery') as app:
            doc, width, height, count = case_8bit(app, photo, seen, report)
            case_fur(app, fur, report)
            case_float(app, output, doc, width, height, count, report)
    except Exception as error:      # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'[:2000]
    log = (output / 'app' / 'app.log')
    alerts = log.is_file() and '[AutomationAlerts]' in log.read_text(errors='replace')
    report['automation_alerts'] = alerts
    passed = 'error' not in report and not alerts and len(report['cases']) == 3
    for name, case in report['cases'].items():
        ok = all(case['rows'].values())
        passed &= ok
        print(f'{"PASS" if ok else "FAIL"}: {name} {json.dumps(case["rows"])} '
              f'{json.dumps(case["detail"], default=str)[:1500]}', flush=True)
    if 'error' in report:
        print(f'FAIL: {report["error"]}', flush=True)
    if alerts:
        print('FAIL: [AutomationAlerts] in the app log', flush=True)
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
