#!/usr/bin/env python3
"""Fetch and verify the pinned genuine-camera NEF against its committed receipt."""
import hashlib
import json
from pathlib import Path
import sys
import urllib.parse
import urllib.request

sys.dont_write_bytecode = True
RECEIPT = Path(__file__).with_name('camera-fixture.json')
DEFAULT_CAMERA = Path.home() / 'devel/doctored-fixtures/camera/camera.NEF'


def fetch_camera(dest: Path = DEFAULT_CAMERA) -> dict:
    """Verify `dest`, downloading the receipt's `raw_url` first when it is absent; never overwrites."""
    dest = Path(dest)
    receipt = json.loads(RECEIPT.read_text())
    url = receipt['raw_url']
    downloaded = not dest.exists()
    if downloaded:
        parts = urllib.parse.urlsplit(url)
        quoted = urllib.parse.urlunsplit(parts._replace(path=urllib.parse.quote(parts.path, safe='/')))
        request = urllib.request.Request(quoted, headers={'User-Agent': 'doctored-raw-probe/1'})
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read()
    else:
        data = dest.read_bytes()
    sha256 = hashlib.sha256(data).hexdigest()
    if sha256 != receipt['sha256'] or len(data) != receipt['encoded_bytes']:
        raise ValueError(f'Camera original at {dest} has sha256 {sha256} ({len(data)} bytes); '
                         f'the receipt pins {receipt["sha256"]} ({receipt["encoded_bytes"]} bytes)')
    if downloaded:
        dest.parent.mkdir(parents=True, exist_ok=True)
        with dest.open('xb') as stream:
            stream.write(data)
    return {'path': str(dest), 'url': url, 'sha256': sha256, 'bytes': len(data), 'downloaded': downloaded}


if __name__ == '__main__':
    print(json.dumps(fetch_camera(Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_CAMERA), indent=2))
