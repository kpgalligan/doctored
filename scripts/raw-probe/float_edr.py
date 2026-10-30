#!/usr/bin/env python3
"""Measurement M1: the `extendedDynamicRangeAmount` of the 32-bit float (linear) RAW development.

    python3 -I -B scripts/raw-probe/float_edr.py --camera <camera.NEF> --output <new folder>

Builds `float_edr.swift` (Core Image only, no app or core source), which renders the pinned camera
file at exposure 0, +1 and +2: one `.RGBA16` sRGB reference per exposure (what a 16-bit
`tone_curve: 0` recipe applies) and one `.RGBAf` extended linear sRGB candidate per exposure at
amount 0, 1 and 2 (what the float recipe applies). Each render is reduced to a 32×20 lattice of
5×5 means.

A candidate passes when all five hold. The bounds are fixed here and are never widened.

V1  At +2 EV the largest float channel of the lattice is >= 1.5 and >= 20 points exceed 1.0.
V2  For points whose largest float channel at 0 EV is in 0.02…0.6 and whose 0 EV reference codes
    are all < 60000: the per-channel ratios f(+1)/f(0) have median in 1.98…2.02 with >= 95 % in
    1.9…2.1, and f(+2)/f(0) median in 3.92…4.08 with >= 95 % in 3.6…4.4. A channel whose 0 EV
    value is not positive has no ratio and counts as outside both ranges.

The 16-bit development compresses highlights with a shoulder (sub-linear above about 0.3 linear)
and a scene-linear float development does not, so agreement with it is asked only below the
shoulder once exposure is raised (controller ruling 1; the first run asked it everywhere and no
amount passed). The shoulder code is S = 65535 × sRGB-encode(0.25) = 35198.77: a point is BELOW
the shoulder when every one of its reference codes is under S, and AT OR ABOVE it otherwise.
`d` is sRGB-encode(clamp01(f)) − code/65535 per channel, the encoding taken per pixel and then
averaged, as the reference's codes are.

V3a At 0 EV, for points with every reference code in 256…65279: |d| has median <= 0.002 and
    >= 95 % <= 0.01.
V3b At +1 EV and at +2 EV, each against the reference of its own exposure, for points with every
    reference code in 256…65279 that are below the shoulder: |d| has median <= 0.002 and
    >= 95 % <= 0.01. Points at or above the shoulder are reported (count, largest signed d) and
    not judged here.
V3c At 0, +1 and +2 EV, for every point at or above the shoulder: the point's LARGEST channel
    of the float development (clipped to 0…1 and encoded) is not below the largest channel of
    the reference by more than 0.01, that is, max(encoded) − max(code)/65535 >= −0.01. No code
    range limit. The per-channel d of those points is reported (the count below −0.01 and the
    smallest) and not judged: the 16-bit development moves an over-range colour toward white,
    which raises its lower channels, so no linear render is per-channel "never darker" there
    (controller ruling 2, written before the third run; the second run judged it per channel).

The first passing amount, in the order 0, 1, 2, is `RawOutputEncoding.linearFloat.extendedDynamicRange`.
Exit status 0 with `CHOSEN extendedDynamicRangeAmount: <n>`; 1 with `STOP` and the table when none
passes. Standard library only. Core Image needs macOS graphics-service access.
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
AMOUNTS = (0.0, 1.0, 2.0)
EXPOSURES = (0.0, 1.0, 2.0)
SHOULDER_CODE = 65535 * (1.055 * 0.25 ** (1 / 2.4) - 0.055)


def within(values, low, high):
    return sum(1 for value in values if low <= value <= high) / len(values) if values else 0.0


def judge(report, amount):
    references = {entry['exposure']: entry for entry in report['references']}
    floats = {entry['exposure']: entry for entry in report['candidates'] if entry['amount'] == amount}
    result = {'amount': amount, 'opaque': all(entry['opaque'] for entry in floats.values()),
              'local_tone_map': [floats[exposure]['local_tone_map'] for exposure in EXPOSURES],
              'edr_read_back': [floats[exposure]['edr_read_back'] for exposure in EXPOSURES],
              'largest_full_frame': [floats[exposure]['largest'] for exposure in EXPOSURES],
              'smallest_full_frame': [floats[exposure]['smallest'] for exposure in EXPOSURES]}

    plus_two = floats[2.0]['linear']
    largest = max(max(point) for point in plus_two)
    above = sum(1 for point in plus_two if max(point) > 1.0)
    result['v1'] = {'largest': largest, 'points_above_one': above, 'passed': largest >= 1.5 and above >= 20}

    base = floats[0.0]['linear']
    codes = references[0.0]['codes']
    chosen = [index for index, point in enumerate(base)
              if 0.02 <= max(point) <= 0.6 and all(code < 60000 for code in codes[index])]
    v2 = {'points': len(chosen)}
    passed = bool(chosen)
    for exposure, median_range, share_range in ((1.0, (1.98, 2.02), (1.9, 2.1)), (2.0, (3.92, 4.08), (3.6, 4.4))):
        ratios = []
        for index in chosen:
            for channel in range(3):
                low = base[index][channel]
                ratios.append(floats[exposure]['linear'][index][channel] / low if low > 0 else float('inf'))
        median = statistics.median(ratios) if ratios else float('nan')
        share = within(ratios, *share_range)
        ok = bool(ratios) and median_range[0] <= median <= median_range[1] and share >= 0.95
        v2[f'plus_{int(exposure)}'] = {'median': median, 'share': share, 'ratios': len(ratios), 'passed': ok}
        passed = passed and ok
    v2['passed'] = passed
    result['v2'] = v2

    v3 = {}
    agreement = {}
    brighter = True
    for exposure in EXPOSURES:
        differences = []
        above = []
        largest_channel = []
        for index, point in enumerate(references[exposure]['codes']):
            signed = [floats[exposure]['encoded'][index][channel] - point[channel] / 65535 for channel in range(3)]
            below = all(code < SHOULDER_CODE for code in point)
            if not below:
                above.extend(signed)
                largest_channel.append(max(floats[exposure]['encoded'][index][:3]) - max(point[:3]) / 65535)
            if all(256 <= code <= 65279 for code in point) and (below or exposure == 0.0):
                differences.extend(abs(value) for value in signed)
        median = statistics.median(differences) if differences else float('nan')
        share = within(differences, 0.0, 0.01)
        ok = bool(differences) and median <= 0.002 and share >= 0.95
        not_darker = all(value >= -0.01 for value in largest_channel)
        v3[f'plus_{int(exposure)}'] = {
            'median': median, 'share': share, 'differences': len(differences),
            'largest': max(differences) if differences else float('nan'), 'passed': ok,
            'above_shoulder_points': len(largest_channel),
            'above_shoulder_largest': max(above) if above else float('nan'),
            'above_shoulder_smallest': min(above) if above else float('nan'),
            'above_shoulder_channels_below': sum(1 for value in above if value < -0.01),
            'above_shoulder_largest_channel_smallest': min(largest_channel) if largest_channel else float('nan'),
            'not_darker': not_darker}
        agreement[exposure] = ok
        brighter = brighter and not_darker
    v3['a_passed'] = agreement[0.0]
    v3['b_passed'] = agreement[1.0] and agreement[2.0]
    v3['c_passed'] = brighter
    v3['passed'] = v3['a_passed'] and v3['b_passed'] and v3['c_passed']
    result['v3'] = v3
    result['passed'] = result['opaque'] and result['v1']['passed'] and v2['passed'] and v3['passed']
    return result


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
    binary = output / 'float_edr'
    with (output / 'build.log').open('w') as log:
        built = subprocess.run(['xcrun', 'swiftc', '-O', '-target', TARGET, str(HERE / 'float_edr.swift'),
                                '-o', str(binary)], stdin=subprocess.DEVNULL, stdout=log,
                               stderr=subprocess.STDOUT, timeout=900)
    if built.returncode != 0:
        print(f'STOP: float_edr.swift did not build; see {output / "build.log"}')
        return 1
    with (output / 'run.log').open('w') as log:
        ran = subprocess.run([str(binary), str(camera), str(output)], stdin=subprocess.DEVNULL, stdout=log,
                             stderr=subprocess.STDOUT, timeout=1800)
    if ran.returncode != 0:
        print(f'STOP: the measurement did not run; see {output / "run.log"}')
        return 1
    report = json.loads((output / 'lattice.json').read_text())
    print(f'camera {camera.name} sha256 {sha256}')
    print(f'os {report["os"]}; decoder {report["decoder"]}; {report["width"]}x{report["height"]}; '
          f'lattice {report["columns"]}x{report["rows"]} of {report["window"]}x{report["window"]} means')
    print(f'isLocalToneMapSupported {report["is_local_tone_map_supported"]}; '
          f'file localToneMapAmount {report["file_local_tone_map"]}')
    results = [judge(report, amount) for amount in AMOUNTS]
    print(f'shoulder code {SHOULDER_CODE:.2f} (0.25 linear)')
    print('amount | V1 largest  >1.0 | V2 pts  +1 median share  +2 median share | '
          'V3a 0 EV, V3b +1, +2 median/share | at or above the shoulder at 0, +1, +2: '
          'points, V3c smallest largest-channel d (per channel, not judged: below -0.01, smallest..largest d) '
          '| verdict')
    for result in results:
        v1, v2, v3 = result['v1'], result['v2'], result['v3']
        keys = ('plus_0', 'plus_1', 'plus_2')
        agreement = ', '.join(f'{v3[key]["median"]:.5f}/{v3[key]["share"]:.3f}' for key in keys)
        shoulder = ', '.join(f'{v3[key]["above_shoulder_points"]} '
                             f'{v3[key]["above_shoulder_largest_channel_smallest"]:+.4f} '
                             f'({v3[key]["above_shoulder_channels_below"]}, '
                             f'{v3[key]["above_shoulder_smallest"]:+.4f}..'
                             f'{v3[key]["above_shoulder_largest"]:+.4f})' for key in keys)
        print(f'{result["amount"]:6.0f} | {v1["largest"]:10.4f} {v1["points_above_one"]:5d} | '
              f'{v2["points"]:6d}  {v2["plus_1"]["median"]:9.4f} {v2["plus_1"]["share"]:5.3f}  '
              f'{v2["plus_2"]["median"]:9.4f} {v2["plus_2"]["share"]:5.3f} | {agreement} | {shoulder} | '
              f'V1 {"pass" if v1["passed"] else "MISS"} V2 {"pass" if v2["passed"] else "MISS"} '
              f'V3a {"pass" if v3["a_passed"] else "MISS"} V3b {"pass" if v3["b_passed"] else "MISS"} '
              f'V3c {"pass" if v3["c_passed"] else "MISS"} opaque {result["opaque"]} '
              f'=> {"PASS" if result["passed"] else "MISS"}')
    chosen = next((result['amount'] for result in results if result['passed']), None)
    summary = {'camera': str(camera), 'sha256': sha256, 'os': report['os'], 'decoder': report['decoder'],
               'is_local_tone_map_supported': report['is_local_tone_map_supported'],
               'file_local_tone_map': report['file_local_tone_map'], 'results': results, 'chosen': chosen}
    (output / 'report.json').write_text(json.dumps(summary, indent=2, sort_keys=True))
    if chosen is None:
        print('STOP: no extendedDynamicRangeAmount candidate passes V1, V2, V3a, V3b and V3c')
        return 1
    print(f'CHOSEN extendedDynamicRangeAmount: {chosen:g}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
