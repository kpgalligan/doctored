"""The existing isolated app transport with Phase 9A private stores."""
import importlib.util
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('phase9a_host_support', ROOT / 'scripts/host-source-probe/support.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
HostApp = module.HostApp
call_for = module.call_for
wait_for = module.wait_for
sys.path.insert(0, str(Path(__file__).resolve().parent))
