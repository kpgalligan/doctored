#!/usr/bin/env python3
"""Measurement M2: the decoder's tone curve and Shadows on the 32-bit float (linear) RAW development.

    python3 -I -B scripts/raw-probe/float_tone.py --camera <camera.NEF> --output <new folder>

Builds `float_tone.swift` (Core Image only, no app or core source; M1's `float_edr.swift` build),
which renders the pinned camera file at six settings (tone curve b, shadows s, exposure e):
(0,0,0), (0.5,0,0), (1,0,0), (1,1,0), (0.5,0,2) and (1,0,2). Each setting renders one `.RGBA16`
sRGB reference at `extendedDynamicRangeAmount` 0 (what a 16-bit recipe applies) and one `.RGBAf`
extended linear sRGB float at `extendedDynamicRangeAmount` 1 and local tone map 0 (what the float
recipe applies), each reduced to a 32×20 lattice of 5×5 means. `encoded` is M1's: the per-pixel
sRGB encoding of the float sample clamped to 0…1, averaged.

The bounds are fixed here, before the first run, and are never widened.

T1  At (0.5,0,2) and at (1,0,2), each: the largest float lattice channel is >= 1.5 and >= 20
    points have a channel > 1.0 (the tone curve keeps values above white).
T2  For (0.5,0,0) and for (1,0,0), each against (0,0,0): over the channels whose reference mean
    moves by more than 0.01 (code/65535), at points whose every reference code is in 256…65279 in
    both renders, the float's `encoded` mean moves with the same sign for >= 90 % of those
    channels, and >= 20 points hold such a channel.
T3  (1,1,0) against (1,0,0), T2's statement (Shadows acts at float as at 16 bits).

Reported, not judged: |encoded − code/65535| median and 95th percentile at (1,0,0) and (1,1,0)
for points whose every reference code is below M1's shoulder S = 65535 × sRGB-encode(0.25).

Exit status 0 with `M2 PASS`; 1 with `STOP: M2 <statements>` and the table on any miss. Standard
library only. Core Image needs macOS graphics-service access.
"""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import sys

HERE = Path(__file__).resolve().parent
TARGET = 'arm64-apple-macos15.0'
SHOULDER_CODE = 65535 * (1.055 * 0.25 ** (1 / 2.4) - 0.055)


def key(entry):
    return (entry['tone_curve'], entry['shadows'], entry['exposure'])


def judge_t1(floats, setting):
    points = floats[setting]['linear']
    largest = max(max(point) for point in points)
    above = sum(1 for point in points if max(point) > 1.0)
    return {'setting': setting, 'largest': largest, 'points_above_one': above,
            'passed': largest >= 1.5 and above >= 20}


def judge_move(references, floats, moved, base):
    channels = 0
    agree = 0
    points = 0
    for index, (after, before) in enumerate(zip(references[moved]['codes'], references[base]['codes'])):
        if not all(256 <= code <= 65279 for code in after + before):
            continue
        held = False
        for channel in range(3):
            delta = (after[channel] - before[channel]) / 65535
            if abs(delta) <= 0.01:
                continue
            held = True
            channels += 1
            float_delta = floats[moved]['encoded'][index][channel] - floats[base]['encoded'][index][channel]
            if float_delta * delta > 0:
                agree += 1
        if held:
            points += 1
    share = agree / channels if channels else 0.0
    return {'moved': moved, 'base': base, 'channels': channels, 'agree': agree, 'share': share,
            'points': points, 'passed': channels > 0 and share >= 0.90 and points >= 20}


def report_agreement(references, floats, setting):
    differences = []
    for index, codes in enumerate(references[setting]['codes']):
        if all(code < SHOULDER_CODE for code in codes):
            differences.extend(abs(floats[setting]['encoded'][index][channel] - codes[channel] / 65535)
                               for channel in range(3))
    if not differences:
        return {'setting': setting, 'differences': 0, 'median': float('nan'), 'p95': float('nan')}
    ordered = sorted(differences)
    return {'setting': setting, 'differences': len(ordered), 'median': statistics.median(ordered),
            'p95': ordered[min(len(ordered) - 1, int(0.95 * len(ordered)))]}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--camera', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    arguments = parser.parse_args()
    camera = arguments.camera.expanduser()
    if not camera.is_file():
        print(f'STOP: the camera file {camera} is absent')
        return 1
    receipt = json.loads((HERE / 'camera-fixture.json').read_text())
    data = camera.read_bytes()
    sha256 = hashlib.sha256(data).hexdigest()
    if sha256 != receipt['sha256'] or len(data) != receipt['encoded_bytes']:
        print(f'STOP: {camera} is not the pinned camera original (sha256 {sha256})')
        return 1
    output = arguments.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = output / 'float_tone'
    with (output / 'build.log').open('w') as log:
        built = subprocess.run(['xcrun', 'swiftc', '-O', '-target', TARGET, str(HERE / 'float_tone.swift'),
                                '-o', str(binary)], stdin=subprocess.DEVNULL, stdout=log,
                               stderr=subprocess.STDOUT, timeout=900)
    if built.returncode != 0:
        print(f'STOP: float_tone.swift did not build; see {output / "build.log"}')
        return 1
    with (output / 'run.log').open('w') as log:
        ran = subprocess.run([str(binary), str(camera), str(output)], stdin=subprocess.DEVNULL, stdout=log,
                             stderr=subprocess.STDOUT, timeout=1800)
    if ran.returncode != 0:
        print(f'STOP: the measurement did not run; see {output / "run.log"}')
        return 1
    report = json.loads((output / 'lattice.json').read_text())
    references = {key(entry): entry for entry in report['references']}
    floats = {key(entry): entry for entry in report['floats']}
    print(f'camera {camera.name} sha256 {sha256}')
    print(f'os {report["os"]}; decoder {report["decoder"]}; {report["width"]}x{report["height"]}; '
          f'lattice {report["columns"]}x{report["rows"]} of {report["window"]}x{report["window"]} means')
    print(f'isLocalToneMapSupported {report["is_local_tone_map_supported"]}; '
          f'file localToneMapAmount {report["file_local_tone_map"]}')
    print('setting (b, s, e) | float opaque, largest, smallest full frame | read back EDR, boost, shadows')
    for setting, entry in floats.items():
        print(f'{setting} | {entry["opaque"]} {entry["largest"]:.4f} {entry["smallest"]:.4f} | '
              f'{entry["edr_read_back"]:g} {entry["boost_read_back"]:g} {entry["shadows_read_back"]:g}')
    t1 = [judge_t1(floats, setting) for setting in ((0.5, 0.0, 2.0), (1.0, 0.0, 2.0))]
    t2 = [judge_move(references, floats, moved, (0.0, 0.0, 0.0)) for moved in ((0.5, 0.0, 0.0), (1.0, 0.0, 0.0))]
    t3 = [judge_move(references, floats, (1.0, 1.0, 0.0), (1.0, 0.0, 0.0))]
    reported = [report_agreement(references, floats, setting) for setting in ((1.0, 0.0, 0.0), (1.0, 1.0, 0.0))]
    for result in t1:
        print(f'T1 {result["setting"]}: largest {result["largest"]:.4f}, points above 1.0 '
              f'{result["points_above_one"]} => {"pass" if result["passed"] else "MISS"}')
    for name, results in (('T2', t2), ('T3', t3)):
        for result in results:
            print(f'{name} {result["moved"]} vs {result["base"]}: {result["agree"]}/{result["channels"]} channels '
                  f'same sign ({result["share"]:.3f}), {result["points"]} points '
                  f'=> {"pass" if result["passed"] else "MISS"}')
    for result in reported:
        print(f'reported {result["setting"]}: below the shoulder |encoded - code| median {result["median"]:.5f}, '
              f'95th percentile {result["p95"]:.5f} over {result["differences"]} channels')
    verdicts = {'T1': all(result['passed'] for result in t1), 'T2': all(result['passed'] for result in t2),
                'T3': all(result['passed'] for result in t3)}
    opaque = all(entry['opaque'] for entry in floats.values())
    summary = {'camera': str(camera), 'sha256': sha256, 'os': report['os'], 'decoder': report['decoder'],
               'is_local_tone_map_supported': report['is_local_tone_map_supported'],
               't1': t1, 't2': t2, 't3': t3, 'reported': reported, 'verdicts': verdicts, 'opaque': opaque}
    (output / 'report.json').write_text(json.dumps(summary, indent=2, sort_keys=True, default=str))
    missed = [name for name, passed in verdicts.items() if not passed]
    if not opaque:
        missed.append('opaque')
    if missed:
        print(f'STOP: M2 {" ".join(missed)}')
        return 1
    print('M2 PASS (T1 PASS, T2 PASS, T3 PASS)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
