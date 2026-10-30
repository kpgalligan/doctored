#!/usr/bin/env python3
"""Create independent native-depth fixtures; retain pinned real originals."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
WIDTH, HEIGHT = 48, 32


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(width, height, depth, pixels, profile):
    pack = (lambda p: struct.pack('>4H', *p)) if depth == 16 else bytes
    rows = [b'\0' + b''.join(pack(pixel) for pixel in pixels[y * width:(y + 1) * width])
            for y in range(height)]
    return (b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
        + chunk(b'iCCP', b'Fixture RGB\0\0' + zlib.compress(profile))
        + chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))


def pixels(depth, width=WIDTH, height=HEIGHT):
    maximum = (1 << depth) - 1
    alphas = [0, 1, 2, 17, maximum // 3, maximum // 2, maximum - 1, maximum]
    result = []
    for y in range(height):
        for x in range(width):
            i = y * width + x
            rgba = [(30001 + 1009 * x + 421 * y) & maximum,
                    (12347 + 701 * i) & maximum, (40003 + 157 * i) & maximum,
                    alphas[(x + 3 * y) % len(alphas)]]
            if x < 5:
                rgba[3] = 0  # Latent colors must bypass exactly and never bleed.
            elif x < 10:
                rgba[3] = 1 + (x + y) % 2
            elif x >= width - 8:
                rgba[3] = maximum
            if y == height // 2 and x == width // 2:
                rgba = [maximum, 0, maximum, maximum]  # Isolated color impulse.
            if y == 1 and x == width // 2:
                rgba = [maximum, maximum // 7, 0, maximum]  # Outside-mask input still feeds convolution.
            result.append(tuple(rgba))
    return result


def mask_pixels(width=WIDTH, height=HEIGHT):
    result = []
    for y in range(height):
        for x in range(width):
            if x < 7 or y < 3:
                result.append(0)
            elif x < 12:
                result.append(1 + (x + y) % 2)
            elif x >= width - 8:
                result.append(65535)
            else:
                result.append((x * 1031 + y * 2393 + 32767) & 65535)
    return result


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def create(output, camera, photos):
    if not Path(camera).is_file():
        raise SystemExit(f'Camera original missing at {camera}; rerun with --download-camera')
    if shutil.which('ffmpeg') is None:
        raise SystemExit('ffmpeg is required to author h264-distinct.mov; install it (brew install ffmpeg) and rerun.')
    output.mkdir(parents=True, exist_ok=True)
    records = []
    for space, filename in [('srgb', 'sRGB Profile.icc'), ('p3', 'Display P3.icc')]:
        profile = (Path('/System/Library/ColorSync/Profiles') / filename).read_bytes()
        (output / f'{space}.icc').write_bytes(profile)
        for depth in (8, 16):
            values = pixels(depth)
            stem = f'source-{space}-{depth}'
            (output / f'{stem}.png').write_bytes(png(WIDTH, HEIGHT, depth, values, profile))
            code = '<4H' if depth == 16 else '4B'
            (output / f'{stem}.rgba').write_bytes(b''.join(struct.pack(code, *p) for p in values))
            records.append(dict(name=stem, width=WIDTH, height=HEIGHT, source_depth=depth,
                                profile=f'{space}.icc'))
            if space == 'p3':
                maximum = (1 << depth) - 1
                alternate = [tuple(maximum - c for c in p[:3]) + (p[3],) for p in values]
                (output / f'replacement-same{depth}.png').write_bytes(png(WIDTH, HEIGHT, depth, alternate, profile))
                wider = pixels(depth, WIDTH + 2, HEIGHT + 2)
                (output / f'replacement-size{depth}.png').write_bytes(png(WIDTH + 2, HEIGHT + 2, depth, wider, profile))
    raw = struct.pack(f'<{WIDTH * HEIGHT}H', *mask_pixels())
    (output / 'filter-mask.rzlm').write_bytes(b'RZLM' + struct.pack('<III', 1, WIDTH, HEIGHT) + raw)
    (output / 'filter-mask.u16').write_bytes(raw)
    side = 129
    impulse = [(1023, 50000, 11111, 0)] * (side * side)
    center = (side // 2) * side + side // 2
    impulse[center] = (43421, 33667, 62851, 65535)
    (output / 'near-full-impulse16.png').write_bytes(png(side, side, 16, impulse, (output / 'p3.icc').read_bytes()))
    impulse_mask = [0] * (side * side); impulse_mask[center] = 65534
    (output / 'near-full-impulse-mask.rzlm').write_bytes(b'RZLM' + struct.pack('<III', 1, side, side)
        + struct.pack(f'<{side * side}H', *impulse_mask))
    receipt = json.loads((ROOT / 'scripts/raw-probe/camera-fixture.json').read_text())
    assert digest(camera) == receipt['sha256'], 'Camera original does not match its independent receipt'
    shutil.copy2(camera, output / 'camera.NEF')
    shutil.copy2(ROOT / 'scripts/raw-probe/camera-fixture.json', output / 'camera-provenance.json')
    movie_spec = importlib.util.spec_from_file_location('phase9a_movie_fixtures', ROOT / 'scripts/host-source-probe/fixtures.py')
    movie_module = importlib.util.module_from_spec(movie_spec); movie_spec.loader.exec_module(movie_module)

    def command(argv):
        result = subprocess.run([str(v) for v in argv], capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise SystemExit(f'ffmpeg exited {result.returncode}: {result.stderr[-2000:]}')
        return result.stdout

    with tempfile.TemporaryDirectory() as scratch:
        authored = movie_module.write_h264_distinct(Path(scratch), command)
        shutil.copy2(Path(scratch) / 'h264-distinct.mov', output / 'h264-distinct.mov')
    (output / 'movie-provenance.json').write_text(json.dumps({
        'source': 'scripts/host-source-probe/fixtures.py write_h264_distinct',
        'argv': authored['argv'], 'ffmpeg': authored['ffmpeg'],
        'sha256': digest(output / 'h264-distinct.mov')}, indent=2) + '\n')
    spec = importlib.util.spec_from_file_location('phase9a_photo_receipts', ROOT / 'scripts/phase6_quality_fixtures.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    photo = next(item for item in module.PHOTOS if item['name'] == 'portrait')
    original = photos / photo['file']
    assert digest(original) == photo['sha256']
    shutil.copy2(original, output / 'photograph.png')
    (output / 'photograph-provenance.json').write_text(json.dumps(photo, indent=2) + '\n')
    (output / 'cases.json').write_text(json.dumps(records, indent=2) + '\n')
    planner_spec = importlib.util.spec_from_file_location('phase9a_planner_fixtures', Path(__file__).with_name('planner_fixture.py'))
    planner = importlib.util.module_from_spec(planner_spec); planner_spec.loader.exec_module(planner)
    (output / 'planner-provenance.json').write_text(json.dumps(planner.create(output), indent=2) + '\n')
    manifest = {p.name: digest(p) for p in sorted(output.iterdir()) if p.is_file() and p.name != 'manifest.json'}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    return manifest


if __name__ == '__main__':
    camera_spec = importlib.util.spec_from_file_location('phase9a_camera', ROOT / 'scripts/raw-probe/camera.py')
    camera_module = importlib.util.module_from_spec(camera_spec); camera_spec.loader.exec_module(camera_module)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path.home() / 'devel/doctored-fixtures/phase9a')
    parser.add_argument('--camera', type=Path, default=camera_module.DEFAULT_CAMERA)
    parser.add_argument('--download-camera', action='store_true',
                        help='Fetch and verify the pinned camera original into --camera first (never overwrites)')
    parser.add_argument('--photos', type=Path, default=Path.home() / 'devel/doctored-fixtures/phase6/photos')
    args = parser.parse_args()
    if args.download_camera:
        camera_module.fetch_camera(args.camera)
    print('Prepared pinned Phase9A fixtures:', len(create(args.output, args.camera, args.photos)))
