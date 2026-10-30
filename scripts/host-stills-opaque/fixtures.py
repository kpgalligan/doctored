#!/usr/bin/env python3
"""Write the committed opaque host-still fixtures and their `provenance.json`.

Every picture is authored here, byte by byte, as a 64x48 PNG with an sRGB
chunk, then encoded by a named third-party encoder (`avifenc`, `heif-enc`,
`cjxl`, `opj_compress`, `ffmpeg`) or, for the gain-map HEIC, by the
independent ImageIO/Core Image oracle beside this file. No personal photo is
used and nothing is derived from Doctored. Each output is checked opaque
through the oracle's ImageIO layout report, except the alpha control.

    python3 scripts/host-stills-opaque/fixtures.py --output core/tests/fixtures/host_stills_opaque
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

HERE = Path(__file__).resolve().parent
WIDTH, HEIGHT = 64, 48
COUNT = WIDTH * HEIGHT          # 3072 = 2^10 * 3
SCHEMA = 1
OPAQUE_ALPHA = {'none', 'noneSkipLast', 'noneSkipFirst'}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def chunk(kind, body):
    return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))


def write_png(path, samples, colour, depth):
    """`samples` is a flat list in the file's own order; `colour` 2 or 6."""
    per_pixel = {2: 3, 6: 4}[colour]
    assert len(samples) == COUNT * per_pixel
    pack = '>H' if depth == 16 else '>B'
    rows = b''
    for y in range(HEIGHT):
        start = y * WIDTH * per_pixel
        rows += b'\0' + b''.join(struct.pack(pack, value)
                                 for value in samples[start:start + WIDTH * per_pixel])
    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n'
                           + chunk(b'IHDR', struct.pack('>IIBBBBB', WIDTH, HEIGHT, depth, colour, 0, 0, 0))
                           + chunk(b'sRGB', b'\0')
                           + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))


def source8():
    """Every 8-bit code on every channel: i*k + c mod 256 with k odd is a
    bijection over each run of 256 pixels."""
    out = []
    for i in range(COUNT):
        out += [i % 256, (i * 7 + 3) % 256, (i * 37 + 11) % 256]
    return out


def source16():
    """3072 distinct values per channel including 0 and 65535: a permutation
    j = (i*k + c) mod 3072 with k coprime to 3072, spread over the full range."""
    def spread(j):
        return (j * 65535 + 1535) // 3071
    out = []
    for i in range(COUNT):
        out += [spread((i * 5) % COUNT), spread((i * 7 + 1000) % COUNT), spread((i * 11 + 2000) % COUNT)]
    return out


def source8_alpha():
    rgb = source8()
    out = []
    for i in range(COUNT):
        out += rgb[i * 3:i * 3 + 3] + [(i * 13 + 40) % 256]
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    folder = arguments.output.resolve()
    folder.mkdir(parents=True, exist_ok=True)

    tools = {}
    for name, version in [('avifenc', ['--version']), ('avifdec', ['--version']),
                          ('heif-enc', ['--version']), ('cjxl', ['--version']),
                          ('opj_compress', ['-h']), ('ffmpeg', ['-version']),
                          ('magick', ['-version'])]:
        executable = shutil.which(name)
        assert executable, f'required fixture tool is missing: {name}'
        result = subprocess.run([executable, *version], capture_output=True, text=True, timeout=30)
        lines = [line for line in (result.stdout + result.stderr).splitlines() if line.strip()]
        tools[name] = dict(executable=executable, version=lines[:3])

    scratch = Path(tempfile.mkdtemp(prefix='rz-host-stills-oracle-'))
    oracle = scratch / 'oracle'
    build = subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
                            str(HERE / 'oracle.swift'), '-o', str(oracle)],
                           capture_output=True, text=True, timeout=600)
    assert build.returncode == 0, build.stderr

    files = {}

    def command(name, argv, source):
        result = subprocess.run([str(part) for part in argv], capture_output=True, text=True, timeout=120)
        assert result.returncode == 0, (argv, result.stderr)
        path = folder / name
        assert path.is_file(), (name, result.stderr)
        # Paths are recorded relative to the fixture folder (the oracle by
        # name), so the provenance does not depend on where it was generated.
        files[name] = dict(argv=[part.name if isinstance(part, Path) else str(part) for part in argv],
                           source=source)

    def authored(name, samples, colour, depth):
        write_png(folder / name, samples, colour, depth)
        files[name] = dict(argv=None, source='authored by fixtures.py')

    authored('source8.png', source8(), 2, 8)
    authored('source16.png', source16(), 2, 16)
    authored('source8-alpha.png', source8_alpha(), 6, 8)
    s8, s16, sa = folder / 'source8.png', folder / 'source16.png', folder / 'source8-alpha.png'

    command('avif8.avif', ['avifenc', '-l', s8, folder / 'avif8.avif'], 'source8.png')
    command('avif10.avif', ['avifenc', '-l', '-d', '10', s16, folder / 'avif10.avif'], 'source16.png')
    command('avif12.avif', ['avifenc', '-l', '-d', '12', s16, folder / 'avif12.avif'], 'source16.png')
    command('avif8-alpha.avif', ['avifenc', '-l', sa, folder / 'avif8-alpha.avif'], 'source8-alpha.png')
    command('heic8.heic', ['heif-enc', '--lossless', '-b', '8', s8, '-o', folder / 'heic8.heic'], 'source8.png')
    command('heic10.heic', ['heif-enc', '--lossless', '-b', '10', s16, '-o', folder / 'heic10.heic'],
            'source16.png')
    # NOT lossless: `--lossless` forces 4:4:4, and ImageIO cannot decode 12-bit
    # 4:4:4 HEVC at all (its CGImage provider has no data and a CG draw of it
    # is all zeros). 12-bit 4:2:0 (Main 12) decodes; the 12-bit row is
    # recorded, not an exactness gate, so a lossy 12-bit file serves it.
    command('heic12.heic', ['heif-enc', '-q', '100', '-b', '12', s16, '-o', folder / 'heic12.heic'],
            'source16.png')
    # The 12-bit 4:4:4 file ImageIO cannot decode: the open
    # refuses it by name, so it is committed as that refusal's fixture.
    command('heic12-444.heic', ['heif-enc', '--lossless', '-b', '12', s16, '-o', folder / 'heic12-444.heic'],
            'source16.png')
    command('jxl8.jxl', ['cjxl', '-d', '0', s8, folder / 'jxl8.jxl'], 'source8.png')
    command('jp2-8.jp2', ['opj_compress', '-i', s8, '-o', folder / 'jp2-8.jp2', '-r', '1'], 'source8.png')
    # The Live pair: the command shape of scripts/host-source-probe/fixtures.py.
    ffmpeg = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y']
    color = ['-color_primaries', 'bt709', '-color_trc', 'bt709', '-colorspace', 'bt709']
    command('live.jpg', [*ffmpeg, '-i', s8, '-frames:v', '1', '-q:v', '2', folder / 'live.jpg'], 'source8.png')
    command('live.mov', [*ffmpeg, '-loop', '1', '-framerate', '24000/1001', '-i', s8, '-frames:v', '4',
                         # setparams: ffmpeg otherwise carries the PNG's sRGB
                         # chunk into the stream as transfer iec61966-2-1, which
                         # the SDR movie policy refuses; the probe's PNGs had none.
                         '-vf', 'setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709,format=yuv420p',
                         '-c:v', 'libx264', '-bf', '2', '-g', '12', '-threads', '1',
                         *color, '-movflags', '+write_colr', '-video_track_timescale', '24000',
                         folder / 'live.mov'], 'source8.png')
    command('gainmap.heic', [oracle, 'gainmap', folder / 'gainmap.heic'],
            'synthetic HDR scene authored in oracle.swift (Core Image HDR HEIF writer)')

    # Self-checks through ImageIO: container depth and opacity per image.
    for name, entry in files.items():
        if name.endswith('.mov'):
            continue
        result = subprocess.run([str(oracle), 'layout', str(folder / name)],
                                capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, (name, result.stderr)
        report = json.loads(result.stdout)
        entry['encoded_depth'] = report['depth']
        entry['imageio_layout'] = report
        request = report['sdr_request'] if name == 'gainmap.heic' else report['hdr_request']
        opaque = request['alpha_info'] in OPAQUE_ALPHA
        if name in ('source8-alpha.png', 'avif8-alpha.avif'):
            assert not opaque, f'{name} is the alpha control and must carry alpha'
        else:
            assert opaque, f'{name} decodes with alpha: {request}'
    expected_depth = {'avif8.avif': 8, 'avif10.avif': 10, 'avif12.avif': 12, 'heic8.heic': 8,
                      'heic10.heic': 10, 'heic12.heic': 12, 'heic12-444.heic': 12, 'gainmap.heic': 8}
    for name, depth in expected_depth.items():
        assert files[name]['encoded_depth'] == depth, (name, files[name]['encoded_depth'])
    # Only the Live pair may share a basename (LivePhoto.locate pairs by name).
    stems = {}
    for name in files:
        stems.setdefault(Path(name).stem, []).append(name)
    assert all(len(names) == 1 or sorted(names) == ['live.jpg', 'live.mov'] for names in stems.values()), stems

    for name, entry in files.items():
        path = folder / name
        entry['sha256'] = digest(path)
        entry['bytes'] = path.stat().st_size
    provenance = dict(
        schema=SCHEMA,
        generator=dict(path='scripts/host-stills-opaque/fixtures.py', sha256=digest(Path(__file__))),
        oracle=dict(path='scripts/host-stills-opaque/oracle.swift', sha256=digest(HERE / 'oracle.swift')),
        macos=dict(version=platform.mac_ver()[0],
                   build=subprocess.run(['sw_vers', '-buildVersion'], capture_output=True,
                                        text=True).stdout.strip()),
        tools=tools,
        files=dict(sorted(files.items())))
    (folder / 'provenance.json').write_text(json.dumps(provenance, indent=2, sort_keys=True) + '\n')
    shutil.rmtree(scratch, ignore_errors=True)
    print(f'Wrote {len(files)} fixtures and provenance.json to {folder}')


if __name__ == '__main__':
    sys.dont_write_bytecode = True
    main()
