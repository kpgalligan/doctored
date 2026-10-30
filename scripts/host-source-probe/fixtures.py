#!/usr/bin/env python3
"""Generate bounded, independent Phase 8F source and rational-frame fixtures."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import zlib

WIDTH, HEIGHT = 384, 96
PROFILE_ROOT = Path('/System/Library/ColorSync/Profiles')
PROFILE_NAMES = {'srgb': 'sRGB Profile.icc', 'p3': 'Display P3.icc'}
CORPUS = Path.home() / 'devel/doctored-fixtures/phase8f'
FFMPEG = ['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y']
COLOR = ['-color_primaries', 'bt709', '-color_trc', 'bt709', '-colorspace', 'bt709']


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def corpus_paths(corpus=CORPUS):
    corpus = Path(corpus)
    return {'fixtures': corpus / 'fixtures', 'reference': corpus / 'reference-output',
            'yuv_reference': corpus / 'yuv-reference', 'reports': corpus / 'reports'}


def claim_corpus(corpus, replace):
    """Refuse to overwrite an existing corpus unless --replace clears it first."""
    paths = corpus_paths(corpus)
    if not replace:
        occupied = [str(path) for path in paths.values() if path.is_dir() and any(path.iterdir())]
        if occupied:
            raise SystemExit(f'8F corpus already present at {corpus} ({", ".join(occupied)}); '
                             'rerun with --replace to re-author it')
        return
    for path in paths.values():
        shutil.rmtree(path, ignore_errors=True)


def tool_version(tool, arguments):
    executable = shutil.which(tool)
    assert executable, f'Required independent fixture tool is missing: {tool}'
    output = subprocess.run([executable, *arguments], capture_output=True, text=True, timeout=10)
    return {'executable': executable, 'version': (output.stdout + output.stderr).splitlines()}


def samples(width=WIDTH, height=HEIGHT, frame=0, opaque=False):
    """Authored samples are independent of every product/decoder code path."""
    for y in range(height):
        for x in range(width):
            yield ((x * 157 + y * 71 + frame * 11213) % 65536,
                   (x * 113 + y * 251 + 37 + frame * 19237) % 65536,
                   (x * 89 + y * 191 + 997 + frame * 25301) % 65536,
                   65535 if opaque else (x * 171 + y * 43 + frame * 7001) % 65536)


def png(path, *, depth=16, profile='srgb', width=WIDTH, height=HEIGHT, frame=0, opaque=False):
    profile_data = (PROFILE_ROOT / PROFILE_NAMES[profile]).read_bytes()
    rows = bytearray()
    values = iter(samples(width, height, frame, opaque))
    for _ in range(height):
        rows.append(0)
        for _ in range(width):
            value = next(values)
            rows.extend(struct.pack('>4H', *value) if depth == 16 else bytes(v >> 8 for v in value))

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
        + chunk(b'iCCP', b'ICC\0\0' + zlib.compress(profile_data))
        + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))
    return {'width': width, 'height': height, 'depth': depth, 'profile': profile,
            'profile_sha256': hashlib.sha256(profile_data).hexdigest(), 'opaque': opaque, 'frame': frame}


def write_h264_distinct(folder, command):
    """Author the four byte frames and the distinct-frame H.264 movie into `folder`."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    ffmpeg = tool_version('ffmpeg', ['-version'])
    authored = {}
    for frame in range(4):
        name = f'frame8-{frame:02d}.png'
        authored[name] = png(folder / name, depth=8, frame=frame, opaque=True)
    argv = [*FFMPEG, '-framerate', '24000/1001', '-i', folder / 'frame8-%02d.png',
            '-frames:v', '4', '-vf', 'format=yuv420p', '-c:v', 'libx264', '-bf', '2', '-g', '12',
            '-threads', '1', *COLOR, '-movflags', '+write_colr', '-video_track_timescale', '24000',
            folder / 'h264-distinct.mov']
    command(argv)
    return {'authored': authored, 'argv': [str(value) for value in argv], 'ffmpeg': ffmpeg}


def generate(folder):
    folder.mkdir(parents=True, exist_ok=True)
    provenance = {'authored': {}, 'commands': [], 'tools': {}}

    def command(arguments):
        result = subprocess.run([str(value) for value in arguments], capture_output=True, text=True, timeout=60)
        provenance['commands'].append({'argv': [str(value) for value in arguments], 'exit_code': result.returncode,
                                       'stderr': result.stderr[-12000:]})
        assert result.returncode == 0, (arguments, result.stderr)
        return result.stdout

    for tool, arguments in [('ffmpeg', ['-version']), ('ffprobe', ['-version']),
                            ('cjxl', ['--version']), ('magick', ['-version']),
                            ('opj_dump', ['-h']), ('brew', ['--version'])]:
        provenance['tools'][tool] = tool_version(tool, arguments)
    provenance['brew'] = command(['brew', 'list', '--versions', 'libheif', 'x265', 'openjpeg',
                                  'jpeg-xl', 'ffmpeg', 'imagemagick'])
    provenance['generator'] = {'path': 'scripts/host-source-probe/fixtures.py', 'sha256': digest(__file__)}
    for name, profile in PROFILE_NAMES.items():
        (folder / f'{name}.icc').write_bytes((PROFILE_ROOT / profile).read_bytes())

    # The former feasibility stills, re-authored from the committed recipe:
    # lossless P3 JXL, HEIC10/HEIC8 and the (now supported, 11D) JPEG 2000.
    provenance['authored']['rgba16-p3.png'] = png(folder / 'rgba16-p3.png', depth=16, profile='p3')
    command(['cjxl', folder / 'rgba16-p3.png', folder / 'rgba16-p3.jxl',
             '--distance=0', '--effort=1', '--num_threads=1'])
    provenance['authored']['heic10-source.png'] = png(folder / 'heic10-source.png', depth=16, opaque=True)
    command(['magick', folder / 'heic10-source.png', '-depth', '10', '-quality', '100', folder / 'rgb10.heic'])
    provenance['authored']['heic8-source.png'] = png(folder / 'heic8-source.png', depth=8, opaque=True)
    command(['magick', folder / 'heic8-source.png', '-quality', '100', folder / 'rgb8.heic'])
    provenance['authored']['jp2-source.png'] = png(folder / 'jp2-source.png', depth=16)
    command(['magick', folder / 'jp2-source.png', '-depth', '16', folder / 'rgba16-srgb.jp2'])
    provenance['opj_dump'] = command(['opj_dump', '-i', folder / 'rgba16-srgb.jp2'])

    for frame in range(4):
        name = f'frame16-{frame:02d}.png'
        provenance['authored'][name] = png(folder / name, depth=16, frame=frame, opaque=False)
    # PPM has no competing ICC metadata: these exact code values are explicitly
    # interpreted as Rec.2100 PQ/HLG, including above-SDR brightness, by libjxl.
    hdr_codes = b''.join(struct.pack('>3H', *value[:3]) for value in samples())
    (folder / 'hdr-codes.ppm').write_bytes(f'P6\n{WIDTH} {HEIGHT}\n65535\n'.encode() + hdr_codes)
    for transfer in ['PQ', 'HLG']:
        command(['cjxl', folder / 'hdr-codes.ppm', folder / f'hdr-{transfer.lower()}.jxl',
                 '--distance=0', '--effort=1', '--num_threads=1', '-x', f'color_space=Rec2100{transfer}'])
    ffmpeg = FFMPEG
    color = COLOR
    prores = ['-c:v', 'prores_ks', '-profile:v', '4', '-alpha_bits', '16', '-threads', '1', *color,
              '-movflags', '+write_colr', '-video_track_timescale', '24000']
    vf = 'sidedata=mode=delete:type=ICC_PROFILE,scale=out_color_matrix=bt709:out_range=tv,format=yuva444p10le'
    # The single ProRes 4444 calibration movie (three frames at 3 fps) and its
    # independent FFmpeg native YUV12 decode.
    command([*ffmpeg, '-framerate', '3', '-i', folder / 'frame16-%02d.png',
             '-frames:v', '3', '-vf', vf, *prores, folder / 'prores709.mov'])
    command([*ffmpeg, '-i', folder / 'prores709.mov', '-pix_fmt', 'yuva444p12le',
             '-f', 'rawvideo', folder / 'prores709.ffmpeg-yuva12le'])
    command([*ffmpeg, '-framerate', '24000/1001', '-i', folder / 'frame16-%02d.png',
             '-frames:v', '4', '-vf', vf, *prores, folder / 'prores-distinct.mov'])
    provenance['authored'].update(write_h264_distinct(folder, command)['authored'])
    command([*ffmpeg, '-framerate', '24000/1001', '-i', folder / 'frame16-%02d.png',
             '-frames:v', '4', '-vf', 'format=yuv420p10le', '-c:v', 'libx264', '-profile:v', 'high10',
             '-threads', '1', *color, '-movflags', '+write_colr', folder / 'h264-high10.mov'])
    command([*ffmpeg, '-framerate', '24000/1001', '-i', folder / 'frame8-%02d.png',
             '-frames:v', '4', '-vf', 'format=yuv420p,setsar=2', '-c:v', 'libx264',
             '-threads', '1', *color, '-movflags', '+write_colr', folder / 'h264-nonsquare.mov'])
    command([*ffmpeg, '-display_rotation', '90', '-i', folder / 'prores-distinct.mov', '-c', 'copy',
             folder / 'prores-rotated.mov'])
    command([*ffmpeg, '-i', folder / 'prores-distinct.mov', '-c', 'copy',
             '-output_ts_offset', '0.125', folder / 'prores-offset.mov'])
    command([*ffmpeg, '-framerate', '1/121', '-i', folder / 'frame16-%02d.png',
             '-frames:v', '1', '-vf', vf, *prores, folder / 'over-duration.mov'])
    # Explicit HDR signalling is a negative policy fixture, not an SDR reference.
    command([*ffmpeg, '-i', folder / 'h264-distinct.mov', '-c', 'copy',
             '-bsf:v', 'h264_metadata=colour_primaries=9:transfer_characteristics=16:matrix_coefficients=9',
             '-color_primaries', 'bt2020', '-color_trc', 'smpte2084', '-colorspace', 'bt2020nc',
             '-movflags', '+write_colr', folder / 'hdr-pq.mov'])

    pair = folder / 'pair'
    pair.mkdir(exist_ok=True)
    provenance['authored']['pair/key-source.png'] = png(pair / 'key-source.png', width=768, height=192, opaque=True)
    command(['magick', pair / 'key-source.png', '-depth', '10', '-quality', '100', pair / 'Live.heic'])
    shutil.copy2(folder / 'h264-distinct.mov', pair / 'Live.mov')
    jpeg_pair = folder / 'pair-jpeg'
    jpeg_pair.mkdir(exist_ok=True)
    command([*ffmpeg, '-i', folder / 'frame8-00.png', '-frames:v', '1', '-q:v', '2', jpeg_pair / 'Live.jpg'])
    shutil.copy2(folder / 'h264-distinct.mov', jpeg_pair / 'Live.mov')

    for name in ['prores709.mov', 'prores-distinct.mov', 'prores-rotated.mov', 'prores-offset.mov',
                 'h264-distinct.mov', 'h264-high10.mov', 'h264-nonsquare.mov',
                 'over-duration.mov', 'hdr-pq.mov']:
        output = command(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams',
                          '-show_frames', '-show_format', '-of', 'json', folder / name])
        (folder / f'{name}.ffprobe.json').write_text(output)
    for name in ['h264-distinct.mov', 'h264-high10.mov', 'hdr-pq.mov']:
        output = command(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams',
                          '-show_packets', '-show_data', '-of', 'json', folder / name])
        (folder / f'{name}.packet-data.json').write_text(output)
    command([*ffmpeg, '-i', folder / 'prores-distinct.mov', '-pix_fmt', 'yuva444p12le',
             '-f', 'rawvideo', folder / 'prores-distinct.ffmpeg-yuva12le'])
    reference_jobs = [{'path': str(folder / name), 'kind': 'image', 'index': 0}
                      for name in ['rgba16-p3.jxl', 'rgb10.heic', 'rgb8.heic', 'pair/Live.heic', 'pair-jpeg/Live.jpg',
                                   'rgba16-srgb.jp2', 'hdr-pq.jxl', 'hdr-hlg.jxl']]
    reference_jobs += [{'path': str(folder / name), 'kind': 'video', 'format': 'b64a'}
                       for name in ['prores709.mov', 'prores-distinct.mov', 'prores-rotated.mov', 'prores-offset.mov']]
    reference_jobs += [{'path': str(folder / name), 'kind': 'video', 'format': 'bgra'}
                       for name in ['h264-distinct.mov', 'pair/Live.mov', 'pair-jpeg/Live.mov']]
    (folder / 'reference-jobs.json').write_text(json.dumps(reference_jobs, indent=2) + '\n')
    yuv_jobs = [{'path': str(folder / name), 'kind': 'video', 'format': 'y416'}
                for name in ['prores709.mov', 'prores-distinct.mov']]
    (folder / 'yuv-reference-jobs.json').write_text(json.dumps(yuv_jobs, indent=2) + '\n')
    provenance['files'] = {str(path.relative_to(folder)): {'bytes': path.stat().st_size, 'sha256': digest(path)}
                           for path in sorted(folder.rglob('*')) if path.is_file() and path.name != 'manifest.json'}
    (folder / 'manifest.json').write_text(json.dumps(provenance, indent=2) + '\n')
    return provenance


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--corpus', type=Path, default=CORPUS)
    parser.add_argument('--replace', action='store_true')
    arguments = parser.parse_args()
    corpus = arguments.corpus.resolve()
    claim_corpus(corpus, arguments.replace)
    result = generate(corpus_paths(corpus)['fixtures'])
    print(f'Generated {len(result["files"])} bounded fixture/reference artifacts.')
