#!/usr/bin/env python3
"""Compare two independently generated GIMP corpora without rewriting artifacts.

GIMP stamps creation times in its builtin ICC headers. Only that authoring-time
field may differ here. Imported/native profile preservation still requires exact
original bytes in the production verifier.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from psd_qualification_decode import decode

spec = importlib.util.spec_from_file_location('external_author_native', ROOT / 'scripts/live-filter-probe/native.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


def profile_equal(a, b):
    assert len(a) == len(b)
    assert a[:24] + a[36:] == b[:24] + b[36:], 'ICC data outside creation time changed'


def png_chunks(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    chunks, offset = [], 8
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        assert zlib.crc32(kind + payload) == struct.unpack_from('>I', data, offset + 8 + size)[0]
        chunks.append((kind, payload))
        offset += size + 12
    assert offset == len(data)
    return chunks


def compare(first, second):
    for root in (first, second):
        for manifest_path in root.glob('*/manifest.json'):
            manifest = json.loads(manifest_path.read_text())
            for name, sha in manifest['files'].items():
                assert hashlib.sha256((manifest_path.parent / name).read_bytes()).hexdigest() == sha
    files = [{str(p.relative_to(root)): p for p in root.rglob('*') if p.is_file()} for root in (first, second)]
    assert files[0].keys() == files[1].keys()
    differences = []
    for relative, path in sorted(files[0].items()):
        a, b = path.read_bytes(), files[1][relative].read_bytes()
        if a == b:
            continue
        differences.append(relative)
        if path.suffix == '.icc':
            profile_equal(a, b)
        elif path.suffix == '.psd':
            x, y = decode(path), decode(files[1][relative])
            profile_a, profile_b = x.pop('profile'), y.pop('profile')
            profile_equal(profile_a, profile_b)
            assert a.count(profile_a) == b.count(profile_b) == 1
            assert a.replace(profile_a, bytes(len(profile_a)), 1) == b.replace(profile_b, bytes(len(profile_b)), 1)
            x['resources'].pop(1039)
            y['resources'].pop(1039)
            assert x == y, relative
        elif path.suffix == '.png':
            x, y = native.png_samples(a), native.png_samples(b)
            assert (x['width'], x['height'], x['depth'], x['samples']) == (y['width'], y['height'], y['depth'], y['samples']), relative
            first_chunks, second_chunks = png_chunks(a), png_chunks(b)
            assert len(first_chunks) == len(second_chunks)
            for (kind, first_payload), (other_kind, second_payload) in zip(first_chunks, second_chunks):
                assert kind == other_kind
                if kind == b'iCCP':
                    first_name, first_profile = first_payload.split(b'\0', 1)
                    second_name, second_profile = second_payload.split(b'\0', 1)
                    assert first_name == second_name and first_profile[0] == second_profile[0] == 0
                    profile_equal(zlib.decompress(first_profile[1:]), zlib.decompress(second_profile[1:]))
                else:
                    assert first_payload == second_payload, (relative, kind)
        elif path.name == 'manifest.json':
            x, y = json.loads(a), json.loads(b)
            x.pop('files')
            y.pop('files')
            for stage in ('author', 'reopened'):
                x[stage].pop('profile_sha256')
                y[stage].pop('profile_sha256')
            assert x == y, relative
        else:
            raise AssertionError(relative)
    return dict(files=len(files[0]), differing_artifact_hashes=differences,
                raw_structure_metadata_and_render_samples_exact=True,
                profile_color_payload_exact=True,
                allowed_authoring_difference='GIMP builtin ICC creation time bytes24..35',
                first_hashes={name: hashlib.sha256(p.read_bytes()).hexdigest() for name, p in files[0].items()},
                second_hashes={name: hashlib.sha256(p.read_bytes()).hexdigest() for name, p in files[1].items()})


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('first', type=Path)
    parser.add_argument('second', type=Path)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    result = compare(args.first, args.second)
    args.receipt.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if not k.endswith('_hashes')}))
