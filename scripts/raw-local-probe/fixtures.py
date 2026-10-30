#!/usr/bin/env python3
"""Create bounded Phase 8G fixtures and frozen standalone-SDK job descriptions."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from phase3_fixtures import synthetic_dng


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--camera', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    expected = json.loads((ROOT / 'raw-probe/camera-fixture.json').read_text())
    assert hashlib.sha256(args.camera.read_bytes()).hexdigest() == expected['sha256']
    args.output.mkdir(parents=True, exist_ok=True)
    camera = args.output / 'camera.NEF'
    shutil.copy2(args.camera, camera)
    synthetic_dng(args.output / 'source.dng', width=512, height=384)
    jobs = []
    for space in ['srgb', 'display_p3']:
        for name, settings in [
            ('initial', {}), ('enhanced', {'exposure': 0.35, 'baseline_compensation': 0.5, 'moire': 1}),
            ('clipped', {'exposure': 4}), ('recovered', {'exposure': 0}),
            ('warm', {'exposure': 1, 'temperature': 7000, 'tint': 17}),
            ('cool', {'exposure': 2, 'temperature': 5000, 'tint': -15}), ('neutral', {'exposure': 3})]:
            jobs.append(dict(name=name+'-'+space, path=str(camera.resolve()), space=space, **settings))
    (args.output / 'reference-jobs.json').write_text(json.dumps(jobs, indent=2)+'\n')
    manifest = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in args.output.iterdir() if p.name != 'manifest.json'}
    (args.output / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True)+'\n')


if __name__ == '__main__':
    main()
