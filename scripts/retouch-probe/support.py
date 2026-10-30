"""The existing isolated app transport, for the Phase 9D retouch probe.

`HostApp` redirects every preset library into its own scratch folder, so this
probe can never read or write the user's real RAW or filter preset libraries.
"""
import importlib.util
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_host = _load('phase9d_host_support', 'scripts/host-source-probe/support.py')
HostApp = _host.HostApp
call_for = _host.call_for
wait_for = _host.wait_for

# Loaded BY PATH, never as a bare `import fixtures`: six probe directories each
# define a `fixtures` module, and whichever one is already on sys.path wins —
# which silently bound precision-probe's 32x12 geometry into the 9B probe and
# made every recorded geometry guard refuse.
fixtures = _load('phase9d_fixtures', 'scripts/retouch-probe/fixtures.py')

_native = _load('phase9d_native', 'scripts/live-filter-probe/native.py')
read_native = _native.read_native
png_samples = _native.png_samples
digest = _native.digest

sys.path.insert(0, str(Path(__file__).resolve().parent))
