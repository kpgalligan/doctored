#!/usr/bin/env python3
"""Author the v1.x HDR photo fixtures and their oracle.

    python3 scripts/hdr-photos/author.py --output ~/devel/doctored-fixtures/hdr-photos
    python3 scripts/hdr-photos/author.py inspect <path> [--patches COLS ROWS]

Every fixture is an AUTHORED scene of flat patches (64 px and up), never a photo:
the system frameworks write the HEIC gain maps and PQ/HLG HEICs
(`author.swift`, ImageIO's `kCGImageDestinationEncodeToISOGainmap`, Core Image's
`.hdrImage`, `kCIImageRepresentationHDRGainMapImage` and
`writeHEIF10Representation`); this script writes the PNG cICP files byte by
byte, and the command-line encoders write the AVIF and JPEG XL ones
(`avifgainmaputil combine`, `avifenc --cicp`, `cjxl`). Each fixture is
self-checked as it is written (dimensions; which gain-map auxiliaries ImageIO
finds; the PQ/HLG decode space) and a failed check exits nonzero.

The two scenes, linear light (1.0 = SDR white for a gain map, 203 cd/m2 for
PQ/HLG), 4 patches across and 2 down:

- GAIN: greys 0.05 0.18 0.5 1.0 2.0 3.0 and two in-range colours, for the Core
  Image and libavif gain maps (both write a colour gain map).
- GAIN_GREY: the same greys and two more (0.75, 1.5), for ImageIO's ISO encoder,
  which writes a MONOCHROME gain map over an SDR base whose tone curve moves
  chroma, so no coloured patch comes back from it within the drive's bound
  (measured: (0.6, 0.3, 0.1) opens as (0.588, 0.327, 0.113)).
- SIGNAL: greys 0.05 0.18 0.5 1.0 2.0 4.0 and two colours above white.

`oracle.json` holds, per fixture, the patch centres, the authored linear values
in the document's own primaries, and for PQ/HLG the authored codes and the
matrix from the document's primaries back to the file's. `provenance.json`
holds the tools' versions, the commands and every file's SHA-256. The `inspect`
subcommand runs `author.swift inspect` (read-only, numbers only).

Admission fixtures (12 and 48 MP gain-map and PQ HEICs, the same scenes
stretched) are written beside them for the drive's `--measure`.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import platform
import struct
import subprocess
import sys
import zlib

HERE = Path(__file__).resolve().parent

GAIN = [[0.05] * 3, [0.18] * 3, [0.5] * 3, [1.0] * 3, [2.0] * 3, [3.0] * 3,
        [0.6, 0.3, 0.1], [0.1, 0.4, 0.7]]
GAIN_GREY = [[0.05] * 3, [0.18] * 3, [0.5] * 3, [1.0] * 3, [2.0] * 3, [3.0] * 3,
             [0.75] * 3, [1.5] * 3]
SIGNAL = [[0.05] * 3, [0.18] * 3, [0.5] * 3, [1.0] * 3, [2.0] * 3, [4.0] * 3,
          [2.5, 1.0, 0.3], [0.2, 0.6, 3.0]]
WIDTH, HEIGHT = 256, 128
COLUMNS, ROWS = 4, 2
ADMISSION = {12: (4032, 3024), 48: (8064, 6048)}

REC709 = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.3127, 0.3290]]
BT2020 = [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046], [0.3127, 0.3290]]


# ---------------------------------------------------------------- colour math

def solve(m, v):
    """m · x = v for a 3x3 m (Cramer's rule)."""
    def det(a):
        return (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
                - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
                + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
    d = det(m)
    out = []
    for k in range(3):
        a = [row[:] for row in m]
        for i in range(3):
            a[i][k] = v[i]
        out.append(det(a) / d)
    return out


def rgb_to_xyz(primaries):
    """Row-major linear RGB -> XYZ of `primaries` (the white at Y = 1)."""
    xyz = [[x / y, 1.0, (1 - x - y) / y] for x, y in primaries]
    columns = [[xyz[k][i] for k in range(3)] for i in range(3)]
    scale = solve(columns, xyz[3])
    return [[xyz[k][i] * scale[k] for k in range(3)] for i in range(3)]


def inverse(m):
    columns = [solve(m, [1.0 if i == k else 0.0 for i in range(3)]) for k in range(3)]
    return [[columns[k][i] for k in range(3)] for i in range(3)]


def multiply(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def conversion(source, target):
    """Linear RGB in `source` primaries to `target` primaries (same D65 white)."""
    return multiply(inverse(rgb_to_xyz(target)), rgb_to_xyz(source))


def pq_signal(nits):
    y = max(0.0, min(1.0, nits / 10000.0))
    p = y ** (2610 / 16384)
    return ((3424 / 4096 + 2413 / 4096 * 32 * p) / (1 + 2392 / 4096 * 32 * p)) ** (2523 / 4096 * 128)


def hlg_signal(linear, luma):
    """BT.2100 HLG of one display-linear triple (1.0 = 203 cd/m2) at Lw 1000, gamma 1.2."""
    display = [max(0.0, c) * 203.0 for c in linear]
    yd = sum(l * d for l, d in zip(luma, display))
    if yd <= 0:
        return [0.0, 0.0, 0.0]
    ys = (yd / 1000.0) ** (1 / 1.2)
    scene = [d / (1000.0 * ys ** 0.2) for d in display]
    a = 0.17883277
    b = 1 - 4 * a
    c = 0.5 - a * math.log(4 * a)
    return [math.sqrt(3 * e) if e <= 1 / 12 else a * math.log(12 * e - b) + c for e in scene]


def srgb_code(v):
    v = max(0.0, min(1.0, v))
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


# --------------------------------------------------------------------- writers

def patch_of(patches, x, y, width, height):
    return patches[min(ROWS - 1, y * ROWS // height) * COLUMNS + min(COLUMNS - 1, x * COLUMNS // width)]


def centres(width, height):
    return [[int((c + 0.5) * width / COLUMNS), int((r + 0.5) * height / ROWS)]
            for r in range(ROWS) for c in range(COLUMNS)]


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)


def write_png(path, depth, pixel, cicp=None, width=WIDTH, height=HEIGHT):
    """An RGB PNG; `pixel(x, y)` gives the three codes."""
    rows = []
    for y in range(height):
        row = bytearray(b'\x00')
        for x in range(width):
            codes = pixel(x, y)
            row += bytes(codes) if depth == 8 else b''.join(struct.pack('>H', c) for c in codes)
        rows.append(bytes(row))
    out = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 2, 0, 0, 0))
    if cicp:
        out += chunk(b'cICP', bytes(cicp))
    out += chunk(b'IDAT', zlib.compress(b''.join(rows), 9)) + chunk(b'IEND', b'')
    Path(path).write_bytes(out)


def run(command, log):
    log.append([str(part) for part in command])
    result = subprocess.run([str(part) for part in command], capture_output=True, text=True, timeout=1800)
    if result.returncode != 0:
        sys.exit(f'{command[0]} failed ({result.returncode}): {result.stderr.strip()[:600]}')
    return result.stdout


def build(output, log):
    binary = output / 'author'
    run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', HERE / 'author.swift', '-o', binary], log)
    return binary


def inspect(binary, path, log, patches=True):
    command = [binary, 'inspect', path] + (['--patches', COLUMNS, ROWS] if patches else [])
    return json.loads(run(command, log))


def scene(path, patches, space, headroom, width=WIDTH, height=HEIGHT):
    Path(path).write_text(json.dumps(dict(width=width, height=height, space=space, headroom=headroom,
                                          patches=patches)))
    return path


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def version(command):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=60)
        return (result.stdout or result.stderr).strip().splitlines()[0]
    except (OSError, IndexError, subprocess.TimeoutExpired):
        return None


def author(output):
    output.mkdir(parents=True, exist_ok=True)
    work = output / 'work'
    work.mkdir(exist_ok=True)
    log = []
    binary = build(work, log)
    oracle = {'columns': COLUMNS, 'rows': ROWS, 'fixtures': {}}
    luma709 = rgb_to_xyz(REC709)[1]
    to2020 = conversion(REC709, BT2020)
    identity = [[1.0 if i == k else 0.0 for k in range(3)] for i in range(3)]

    def record(name, kind, profile, values, **extra):
        oracle['fixtures'][name] = dict(kind=kind, profile=profile, width=WIDTH, height=HEIGHT,
                                        centres=centres(WIDTH, HEIGHT), linear=values, **extra)

    # Gain maps (HEIC): ImageIO ISO (Display P3 base, as its encoder writes),
    # Core Image ISO and Core Image Apple (sRGB base).
    p3_scene = scene(work / 'gain-p3.json', GAIN_GREY, 'display_p3', 4.5)
    srgb_scene = scene(work / 'gain-srgb.json', GAIN, 'srgb', 4.0)
    run([binary, 'gainmap-iso', p3_scene, output / 'gainmap-iso.heic'], log)
    record('gainmap-iso.heic', 'gain_map', 'display_p3', GAIN_GREY, writer='imageio-iso')
    run([binary, 'gainmap-ci', srgb_scene, output / 'gainmap-ci.heic'], log)
    record('gainmap-ci.heic', 'gain_map', 'srgb', GAIN, writer='coreimage-hdrimage')
    run([binary, 'gainmap-apple', srgb_scene, output / 'gainmap-ci.heic', output / 'gainmap-apple.heic'], log)
    record('gainmap-apple.heic', 'gain_map', 'srgb', GAIN, writer='coreimage-hdrgainmapimage')

    # Gain map (AVIF): libavif's combine of an SDR base and a PQ alternate.
    write_png(work / 'gain-base.png', 8,
              lambda x, y: [round(255 * srgb_code(c)) for c in patch_of(GAIN, x, y, WIDTH, HEIGHT)])
    write_png(work / 'gain-alternate.png', 16,
              lambda x, y: [round(65535 * pq_signal(203 * c)) for c in patch_of(GAIN, x, y, WIDTH, HEIGHT)])
    run(['avifgainmaputil', 'combine', work / 'gain-base.png', work / 'gain-alternate.png',
         output / 'gainmap-iso.avif', '--cicp-base', '1/13/6', '--cicp-alternate', '1/16/6',
         '--qgain-map', '100', '-q', '100'], log)
    record('gainmap-iso.avif', 'gain_map', 'srgb', GAIN, writer='avifgainmaputil-combine')

    # PQ HEIC: Core Image converts the BT.709 scene to BT.2020 and codes it.
    # HLG HEIC: BT.2100 codes computed here, written through Core Image's
    # HEIF10 writer in the same HLG space (`author.swift heic-codes`), because
    # Core Image's HLG model of a LINEAR scene does not follow BT.2100's
    # luminance-based OOTF for a saturated colour (measured: its (2.5, 1.0,
    # 0.3) decodes 4.5 % dark under BT.2100 while every grey agrees), so its
    # codes for a linear scene are not a BT.2100 oracle.
    signal_scene = scene(work / 'signal-srgb.json', SIGNAL, 'srgb', 4.0)
    luma2020 = rgb_to_xyz(BT2020)[1]
    wide = [apply(to2020, v) for v in SIGNAL]
    run([binary, 'heif10', signal_scene, 'pq', output / 'pq.heic'], log)
    record('pq.heic', 'pq', 'srgb', SIGNAL, writer='coreimage-heif10', code_bits=10,
           codes=[[round(1023 * pq_signal(203 * c)) for c in v] for v in wide], to_file=to2020,
           file_luma=luma2020)

    def hlg2020(x, y):
        return [round(65535 * s) for s in hlg_signal(apply(to2020, patch_of(SIGNAL, x, y, WIDTH, HEIGHT)),
                                                     luma2020)]

    write_png(work / 'hlg-codes2020.png', 16, hlg2020)
    run([binary, 'heic-codes', work / 'hlg-codes2020.png', 'hlg', output / 'hlg.heic'], log)
    record('hlg.heic', 'hlg', 'srgb', SIGNAL, writer='coreimage-heif10-codes', code_bits=10,
           codes=[[round(1023 * c / 65535) for c in hlg2020(x, y)] for x, y in centres(WIDTH, HEIGHT)],
           to_file=to2020, file_luma=luma2020)

    # PQ / HLG in BT.709: 16-bit codes written here, then AVIF (avifenc) and
    # JPEG XL (cjxl) from the same codes.
    for transfer, tc in (('pq', 16), ('hlg', 18)):
        def code16(x, y, transfer=transfer):
            v = patch_of(SIGNAL, x, y, WIDTH, HEIGHT)
            signal = [pq_signal(203 * c) for c in v] if transfer == 'pq' else hlg_signal(v, luma709)
            return [round(65535 * s) for s in signal]

        def code10(x, y, transfer=transfer):
            return [round(round(1023 * c / 65535) * 65535 / 1023) for c in code16(x, y)]

        write_png(output / f'{transfer}-cicp.png', 16, code16, cicp=[1, tc, 0, 1])
        record(f'{transfer}-cicp.png', transfer, 'srgb', SIGNAL, writer='author.py', code_bits=16,
               codes=[code16(x, y) for x, y in centres(WIDTH, HEIGHT)], to_file=identity, file_luma=luma709)
        write_png(work / f'{transfer}-codes10.png', 16, code10)
        run(['avifenc', '-s', '4', '-q', '100', '-y', '444', '-d', '10', '--cicp', f'1/{tc}/6',
             work / f'{transfer}-codes10.png', output / f'{transfer}.avif'], log)
        record(f'{transfer}.avif', transfer, 'srgb', SIGNAL, writer='avifenc', code_bits=10,
               codes=[[round(1023 * c / 65535) for c in code16(x, y)] for x, y in centres(WIDTH, HEIGHT)],
               to_file=identity, file_luma=luma709)
    run(['cjxl', output / 'pq-cicp.png', output / 'pq.jxl', '-d', '0'], log)
    record('pq.jxl', 'pq', 'srgb', SIGNAL, writer='cjxl', code_bits=16,
           codes=oracle['fixtures']['pq-cicp.png']['codes'], to_file=identity, file_luma=luma709)

    # Self-checks the writers cannot make themselves.
    checks = {}
    for name, entry in oracle['fixtures'].items():
        seen = inspect(binary, output / name, log, patches=False)
        aux = seen['auxiliary']
        if entry['kind'] == 'gain_map':
            want_iso = name != 'gainmap-apple.heic'
            ok = (aux['iso'] is not None) == want_iso and (aux['apple'] is not None) == (not want_iso)
        else:
            ok = aux['iso'] is None and aux['apple'] is None and seen.get('hdr_decode_space') is not None
        checks[name] = dict(ok=ok, iso=aux['iso'] is not None, apple=aux['apple'] is not None,
                            hdr_decode_space=seen.get('hdr_decode_space'),
                            width=seen['width'], height=seen['height'])
        if not ok or (seen['width'], seen['height']) != (WIDTH, HEIGHT):
            sys.exit(f'self-check failed for {name}: {checks[name]}')

    # Admission fixtures: the same scenes stretched to 12 and 48 MP.
    admission = {}
    for size, (width, height) in ADMISSION.items():
        big_gain = scene(work / f'gain-p3-{size}.json', GAIN_GREY, 'display_p3', 4.5, width, height)
        big_signal = scene(work / f'signal-srgb-{size}.json', SIGNAL, 'srgb', 4.0, width, height)
        gain_path = output / f'admission-gainmap-{size}.heic'
        pq_path = output / f'admission-pq-{size}.heic'
        run([binary, 'gainmap-iso', big_gain, gain_path], log)
        run([binary, 'heif10', big_signal, 'pq', pq_path], log)
        admission[f'gainmap-{size}'] = dict(path=gain_path.name, width=width, height=height, kind='gain_map')
        admission[f'pq-{size}'] = dict(path=pq_path.name, width=width, height=height, kind='pq')
    oracle['admission'] = admission

    (output / 'oracle.json').write_text(json.dumps(oracle, indent=2, sort_keys=True) + '\n')
    provenance = dict(
        macos=platform.mac_ver()[0], machine=platform.machine(),
        swiftc=version(['xcrun', 'swiftc', '--version']), avifenc=version(['avifenc', '--version']),
        avifgainmaputil=version(['avifgainmaputil', 'help']), cjxl=version(['cjxl', '--version']),
        author_swift_sha256=digest(HERE / 'author.swift'), author_py_sha256=digest(Path(__file__)),
        commands=log, self_checks=checks,
        files={path.name: dict(sha256=digest(path), bytes=path.stat().st_size)
               for path in sorted(output.iterdir()) if path.is_file() and path.suffix != '.json'})
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2, sort_keys=True) + '\n')
    print(json.dumps(dict(ok=True, fixtures=sorted(oracle['fixtures']), admission=sorted(admission))))


def main():
    if len(sys.argv) > 1 and sys.argv[1] == 'inspect':
        parser = argparse.ArgumentParser(prog='author.py inspect')
        parser.add_argument('command')
        parser.add_argument('path', type=Path)
        parser.add_argument('--patches', nargs=2, type=int)
        parser.add_argument('--work', type=Path, default=Path('/tmp') / 'rz-hdr-photos-author')
        args = parser.parse_args()
        args.work.mkdir(parents=True, exist_ok=True)
        binary = build(args.work, [])
        command = [binary, 'inspect', args.path] + (['--patches', *args.patches] if args.patches else [])
        sys.stdout.write(run(command, []))
        return
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    author(args.output.expanduser())


if __name__ == '__main__':
    main()
