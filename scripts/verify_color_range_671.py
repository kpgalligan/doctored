#!/usr/bin/env python3
"""The MCP drive: Select ▸ Color Range's hue presets, Invert and
Localized Color Clusters, and the Magic Wand's Sample Size (phase 1), Out Of
Gamut (phase 2) and Skin Tones (phase 3), at 8, 16 and 32 bits and on an 8-bit
CMYK document, plus recorded Actions that replay them.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.
Coverage is read through `get_selection` points (0–255).

Fixture `chart.png` (120 x 40, 8-bit opaque sRGB, written here): the top row is
six 20 x 20 patches, red (255, 0, 0), yellow, green, cyan (0, 255, 255), blue
and magenta from x = 0; the bottom row is grey 128 except a skin patch
(`SKIN_PATCH`) at x 40…59 and a second red patch at x 100…119. The 16-bit leg is `convert_precision rgba16`, the 32-bit leg
`convert_precision rgba32`, the CMYK leg `convert_mode cmyk` (8 bits, the
working CMYK).

- **C1 Reds.** `select_color_range range=reds`: on the 8/16/32 legs the red
  patch's centre (10, 10) reads ≥ 250 and the cyan patch's (70, 10) ≤ 5. On
  the CMYK leg (the view of press red is not (1, 0, 0)) red ≥ cyan + 64, with
  no absolute bound.
- **C2 Invert.** `range=reds invert=true`: on every leg the red and the cyan
  centres each read 255 − their C1 value, ± 1.
- **C3 Localized.** `range=sampled`, one sample (the red patch's own code:
  `samples` [255, 0, 0] at 8 bits, `samples16` [65535, 0, 0] at 16 and 32 bits
  (the 32-bit display code of linear 1), on CMYK `sample_color`'s
  `center_rgba16` there), tolerance 10, softness 0,
  `localized=true`, `points=[[10, 10]]`, `cluster_range=10`: (10, 10) reads
  ≥ 250 and the far red patch's (110, 30) reads 0, on every leg.
- **W1 wand Sample Size.** On `checker.png` (40 x 40, a 2-pixel checker of
  grey 160 and 100, 160 at (0, 0)): `select_magic_wand` at (20, 20),
  tolerance 32, contiguous false, `sample_size=1` selects exactly the drive's
  own oracle region (every 160) and `sample_size=5` exactly the oracle's region
  for the truncating 5 x 5 mean about the seed (131: every pixel), at 8 and at
  16 bits (the oracle on codes × 257 with the tolerance × 257, as the host
  sends it, the mean taken on those codes); every pixel is read. The two
  oracle regions differ at each depth.
- **A1 Action.** `start_recording`, `select_color_range range=reds invert=true`,
  `select_magic_wand sample_size=5`, `stop_recording`: the steps carry
  `invert: true` and `sample_size: 5`; saved and run on a fresh 8-bit chart and
  checker, each replayed selection reads what the direct call read.

Phase 2 (Out Of Gamut; the window has no proof setup and the private preset
folder holds no default preset, so the proof is Working CMYK, the isolated
Color Settings' default `USWebCoatedSWOP.icc`):

- **G1 Out Of Gamut.** `select_color_range range=out_of_gamut`: on the 8, 16
  and 32 legs the blue patch's centre (90, 10) reads ≥ 250 and grey (10, 30)
  ≤ 5, and the reply names `proof_profile` (a non-empty string).
- **G2 Invert.** `range=out_of_gamut invert=true`: on the 8, 16 and 32 legs
  blue and grey each read 255 − their G1 value, ± 1. On the CMYK leg
  `range=out_of_gamut` replies `selection_empty: true` and names
  `proof_profile`, and with `invert=true` grey (10, 30) reads 255.
- **A2 Action.** `start_recording`, `select_color_range range=out_of_gamut
  invert=true`, `stop_recording`: the step carries `range: "out_of_gamut"` and
  `invert: true`; saved and run on a fresh 8-bit chart, blue, grey and (110, 30)
  read what the direct call read.

Phase 3 (Skin Tones; the skin patch is (217, 172, 130), the SK1 colour take3
described Photoshop selecting at 252 with Fuzziness 40, `SKIN_PATCH_SOURCE`):

- **S1 Skin Tones.** `select_color_range range=skin_tones` (Fuzziness 40): on
  the 8, 16 and 32 legs the skin patch's centre (50, 30) reads ≥ 128 and blue
  (90, 10) ≤ 5, the 16 and 32 readings each within ± 2 of the 8-bit one. On
  the CMYK leg the patch reads ≥ blue + 64.
- **S2 Invert.** `range=skin_tones invert=true`: on every leg the patch and
  blue each read 255 − their S1 value, ± 1.
- **S3 Fuzziness.** `fuzziness=200` reads ≥ `fuzziness=0` at the patch and at
  grey (10, 30), on every leg; `range=reds fuzziness=40` replies `isError`
  with "fuzziness applies only to range skin_tones.".
- **A3 Action.** `start_recording`, `select_color_range range=skin_tones
  fuzziness=80 invert=true`, `stop_recording`: the step carries
  `range: "skin_tones"`, `fuzziness: 80` and `invert: true`; saved and run on
  a fresh 8-bit chart, the patch, blue and grey read what the direct call
  read.
"""
import argparse
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
W, H = 120, 40
PATCHES = [(255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255), (0, 0, 255), (255, 0, 255)]
RED, CYAN, FAR = (10, 10), (70, 10), (110, 30)
FULL_MIN, EMPTY_MAX, CMYK_SEPARATION, INVERT_SLACK = 250, 5, 64, 1
BLUE, GREY = (90, 10), (10, 30)
CW = 40
SIZES = (1, 5)
WAND_T = 32
LEGS = ('8', '16', '32', 'cmyk')
SKIN_PATCH = (217, 172, 130)
SKIN_PATCH_SOURCE = 'take3 described.json drive_patch: SK1 (29, 32), Photoshop 252 at F 40'
SKIN = (50, 30)
SKIN_MIN, DEPTH_SLACK, SKIN_FUZZINESS = 128, 2, 80
FUZZINESS_SENTENCE = 'fuzziness applies only to range skin_tones.'


def chart_bytes():
    out = bytearray()
    for y in range(H):
        for x in range(W):
            if y < 20:
                rgb = PATCHES[x // 20]
            elif 40 <= x < 60:
                rgb = SKIN_PATCH
            else:
                rgb = (255, 0, 0) if x >= 100 else (128, 128, 128)
            out += bytes(rgb + (255,))
    return bytes(out)


def checker():
    return [[160 if (x // 2 + y // 2) % 2 == 0 else 100 for x in range(CW)] for y in range(CW)]


def checker_bytes():
    return bytes(b for row in checker() for v in row for b in (v, v, v, 255))


def box_mean(grid, seed, size):
    r = size // 2
    values = [grid[y][x] for y in range(seed[1] - r, seed[1] + r + 1)
              for x in range(seed[0] - r, seed[0] + r + 1) if 0 <= x < CW and 0 <= y < CW]
    return sum(values) // len(values)


def oracle(grid, seed, size, tolerance, scale=1):
    """The global wand region against the truncating box mean (grey: |Δv| ≤ t),
    on codes × `scale` with the tolerance × `scale` (257 at 16 bits)."""
    grid = [[v * scale for v in row] for row in grid]
    reference = box_mean(grid, seed, size)
    return [[abs(v - reference) <= tolerance * scale for v in row] for row in grid]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='color-range-671-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    chart = folder / 'chart.png'
    png(chart, W, H, chart_bytes())
    board = folder / 'checker.png'
    png(board, CW, CW, checker_bytes())
    receipts = {'expectations': 'fixed in scripts/verify_color_range_671.py before any measurement'}
    failures = 0

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def coverage(doc, points):
            reply = call('get_selection', document_id=doc, points=[{'x': x, 'y': y} for x, y in points])
            return [s['coverage'] for s in reply['samples']]

        def everything(doc, size):
            reply = call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for y in range(size) for x in range(size)])
            grid = [[False] * size for _ in range(size)]
            for s in reply['samples']:
                grid[s['y']][s['x']] = s['coverage'] == 255
            return grid

        def leg(depth):
            doc = call('open_document', path=str(chart))['document']['id']
            if depth == 'cmyk':
                call('convert_mode', document_id=doc, mode='cmyk')
            elif depth != '8':
                call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
            return doc

        def red_sample(doc, depth):
            if depth == '8':
                return {'samples': [[255, 0, 0]]}
            if depth != 'cmyk':
                return {'samples16': [[65535, 0, 0]]}
            reply = call('sample_color', document_id=doc, x=RED[0], y=RED[1])
            return {'samples16': [reply['center_rgba16'][:3]]}

        def case(name, run):
            nonlocal failures
            try:
                detail = run()
                receipts[name] = detail
                print(f'PASS {name}: {detail}', flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports
                failures += 1
                receipts[name] = {'failed': repr(error)}
                print(f'FAIL {name}: {error!r}', flush=True)

        for depth in LEGS:
            def colour_range(depth=depth):
                doc = leg(depth)
                call('select_color_range', document_id=doc, range='reds')
                red, cyan = coverage(doc, [RED, CYAN])
                if depth == 'cmyk':
                    assert red >= cyan + CMYK_SEPARATION, ('C1', red, cyan)
                else:
                    assert red >= FULL_MIN and cyan <= EMPTY_MAX, ('C1', red, cyan)
                call('select_color_range', document_id=doc, range='reds', invert=True)
                ired, icyan = coverage(doc, [RED, CYAN])
                assert abs(red + ired - 255) <= INVERT_SLACK, ('C2 red', red, ired)
                assert abs(cyan + icyan - 255) <= INVERT_SLACK, ('C2 cyan', cyan, icyan)
                call('select_color_range', document_id=doc, range='sampled', tolerance=10, softness=0,
                     localized=True, points=[list(RED)], cluster_range=10, **red_sample(doc, depth))
                near, far = coverage(doc, [RED, FAR])
                assert near >= FULL_MIN and far == 0, ('C3', near, far)
                call('close_document', document_id=doc, discard_changes=True)
                return {'reds': [red, cyan], 'inverted': [ired, icyan], 'localized': [near, far]}
            case(f'c1-c3 {depth}', colour_range)

        for depth in LEGS:
            def gamut(depth=depth):
                doc = leg(depth)
                reply = call('select_color_range', document_id=doc, range='out_of_gamut')
                assert isinstance(reply.get('proof_profile'), str) and reply['proof_profile'], reply
                if depth == 'cmyk':
                    assert reply['selection_empty'] is True, ('G2 cmyk', reply)
                    inverted = call('select_color_range', document_id=doc, range='out_of_gamut',
                                    invert=True)
                    grey, = coverage(doc, [GREY])
                    assert grey == 255, ('G2 cmyk invert', grey)
                    call('close_document', document_id=doc, discard_changes=True)
                    return {'empty': reply['selection_empty'], 'inverted_grey': grey,
                            'proof_profile': inverted.get('proof_profile')}
                blue, grey = coverage(doc, [BLUE, GREY])
                assert blue >= FULL_MIN and grey <= EMPTY_MAX, ('G1', blue, grey)
                call('select_color_range', document_id=doc, range='out_of_gamut', invert=True)
                iblue, igrey = coverage(doc, [BLUE, GREY])
                assert abs(blue + iblue - 255) <= INVERT_SLACK, ('G2 blue', blue, iblue)
                assert abs(grey + igrey - 255) <= INVERT_SLACK, ('G2 grey', grey, igrey)
                call('close_document', document_id=doc, discard_changes=True)
                return {'out_of_gamut': [blue, grey], 'inverted': [iblue, igrey],
                        'proof_profile': reply['proof_profile']}
            case(f'g1-g2 {depth}', gamut)

        skin_eight = {}
        for depth in LEGS:
            def skin(depth=depth):
                doc = leg(depth)
                call('select_color_range', document_id=doc, range='skin_tones')
                patch, blue = coverage(doc, [SKIN, BLUE])
                if depth == 'cmyk':
                    assert patch >= blue + CMYK_SEPARATION, ('S1 cmyk', patch, blue)
                else:
                    assert patch >= SKIN_MIN and blue <= EMPTY_MAX, ('S1', patch, blue)
                    if depth == '8':
                        skin_eight['patch'], skin_eight['blue'] = patch, blue
                    else:
                        assert 'patch' in skin_eight, 'S1 needs the 8-bit leg first'
                        assert abs(patch - skin_eight['patch']) <= DEPTH_SLACK, \
                            ('S1 depth', patch, skin_eight['patch'])
                        assert abs(blue - skin_eight['blue']) <= DEPTH_SLACK, \
                            ('S1 depth blue', blue, skin_eight['blue'])
                call('select_color_range', document_id=doc, range='skin_tones', invert=True)
                ipatch, iblue = coverage(doc, [SKIN, BLUE])
                assert abs(patch + ipatch - 255) <= INVERT_SLACK, ('S2 patch', patch, ipatch)
                assert abs(blue + iblue - 255) <= INVERT_SLACK, ('S2 blue', blue, iblue)
                call('select_color_range', document_id=doc, range='skin_tones', fuzziness=0)
                narrow = coverage(doc, [SKIN, GREY])
                call('select_color_range', document_id=doc, range='skin_tones', fuzziness=200)
                wide = coverage(doc, [SKIN, GREY])
                assert all(w >= n for w, n in zip(wide, narrow)), ('S3', narrow, wide)
                refusal = call('select_color_range', document_id=doc, range='reds', fuzziness=40,
                               fail=True)
                assert FUZZINESS_SENTENCE in refusal, ('S3 refusal', refusal)
                call('close_document', document_id=doc, discard_changes=True)
                return {'skin_tones': [patch, blue], 'inverted': [ipatch, iblue],
                        'fuzziness_0': narrow, 'fuzziness_200': wide}
            case(f's1-s3 {depth}', skin)

        for depth in ('8', '16'):
            def wand(depth=depth):
                doc = call('open_document', path=str(board))['document']['id']
                if depth == '16':
                    call('convert_precision', document_id=doc, mode='rgba16', structure='preserve')
                grids = {}
                for size in SIZES:
                    call('select_magic_wand', document_id=doc, x=20, y=20, tolerance=WAND_T,
                         contiguous=False, sample_size=size)
                    got = everything(doc, CW)
                    want = oracle(checker(), (20, 20), size, WAND_T, 257 if depth == '16' else 1)
                    assert got == want, ('W1', depth, size)
                    grids[size] = sum(row.count(True) for row in got)
                scale = 257 if depth == '16' else 1
                assert oracle(checker(), (20, 20), 1, WAND_T, scale) != \
                    oracle(checker(), (20, 20), 5, WAND_T, scale), 'W1 the sizes must part'
                call('close_document', document_id=doc, discard_changes=True)
                return {'selected': grids, 'mean_5x5': box_mean(checker(), (20, 20), 5)}
            case(f'w1 {depth}', wand)

        def action():
            doc = call('open_document', path=str(chart))['document']['id']
            wand_doc = call('open_document', path=str(board))['document']['id']
            assert call('start_recording', discard=True)['ok']
            call('select_color_range', document_id=doc, range='reds', invert=True)
            direct_range = coverage(doc, [RED, CYAN, FAR])
            call('select_magic_wand', document_id=wand_doc, x=20, y=20, tolerance=WAND_T,
                 contiguous=False, sample_size=5)
            direct_wand = everything(wand_doc, CW)
            steps = call('stop_recording')['steps']
            for d in (doc, wand_doc):
                call('close_document', document_id=d, discard_changes=True)
            ranges = [s for s in steps if s['tool'] == 'select_color_range']
            wands = [s for s in steps if s['tool'] == 'select_magic_wand']
            assert len(ranges) == 1 and ranges[0]['arguments'].get('invert') is True, steps
            assert len(wands) == 1 and wands[0]['arguments'].get('sample_size') == 5, steps
            for name, step in (('671-range', ranges[0]), ('671-wand', wands[0])):
                reply = call('save_action', name=name, steps=[step], overwrite=True)
                assert reply['ok'], reply
            fresh = call('open_document', path=str(chart))['document']['id']
            assert call('run_action', document_id=fresh, name='671-range')['ok']
            assert coverage(fresh, [RED, CYAN, FAR]) == direct_range, 'A1 range replay'
            fresh_wand = call('open_document', path=str(board))['document']['id']
            assert call('run_action', document_id=fresh_wand, name='671-wand')['ok']
            assert everything(fresh_wand, CW) == direct_wand, 'A1 wand replay'
            for d in (fresh, fresh_wand):
                call('close_document', document_id=d, discard_changes=True)
            return {'range_step': ranges[0]['arguments'], 'wand_step': wands[0]['arguments']}
        case('a1 action', action)

        def gamut_action():
            doc = call('open_document', path=str(chart))['document']['id']
            assert call('start_recording', discard=True)['ok']
            call('select_color_range', document_id=doc, range='out_of_gamut', invert=True)
            direct = coverage(doc, [BLUE, GREY, FAR])
            steps = call('stop_recording')['steps']
            call('close_document', document_id=doc, discard_changes=True)
            ranges = [s for s in steps if s['tool'] == 'select_color_range']
            assert len(ranges) == 1, steps
            arguments = ranges[0]['arguments']
            assert arguments.get('range') == 'out_of_gamut' and arguments.get('invert') is True, steps
            reply = call('save_action', name='671-gamut', steps=[ranges[0]], overwrite=True)
            assert reply['ok'], reply
            fresh = call('open_document', path=str(chart))['document']['id']
            assert call('run_action', document_id=fresh, name='671-gamut')['ok']
            replayed = coverage(fresh, [BLUE, GREY, FAR])
            assert replayed == direct, ('A2 replay', direct, replayed)
            call('close_document', document_id=fresh, discard_changes=True)
            return {'step': arguments, 'readings': direct}
        case('a2 action', gamut_action)

        def skin_action():
            doc = call('open_document', path=str(chart))['document']['id']
            assert call('start_recording', discard=True)['ok']
            call('select_color_range', document_id=doc, range='skin_tones',
                 fuzziness=SKIN_FUZZINESS, invert=True)
            direct = coverage(doc, [SKIN, BLUE, GREY])
            steps = call('stop_recording')['steps']
            call('close_document', document_id=doc, discard_changes=True)
            ranges = [s for s in steps if s['tool'] == 'select_color_range']
            assert len(ranges) == 1, steps
            arguments = ranges[0]['arguments']
            assert arguments.get('range') == 'skin_tones' and arguments.get('invert') is True \
                and arguments.get('fuzziness') == SKIN_FUZZINESS, steps
            reply = call('save_action', name='671-skin', steps=[ranges[0]], overwrite=True)
            assert reply['ok'], reply
            fresh = call('open_document', path=str(chart))['document']['id']
            assert call('run_action', document_id=fresh, name='671-skin')['ok']
            replayed = coverage(fresh, [SKIN, BLUE, GREY])
            assert replayed == direct, ('A3 replay', direct, replayed)
            call('close_document', document_id=fresh, discard_changes=True)
            return {'step': arguments, 'readings': direct}
        case('a3 action', skin_action)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    if failures:
        print(f'{failures} CASE(S) FAILED', flush=True)
        return 1
    print('ALL CASES PASS', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
