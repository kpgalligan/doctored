#!/usr/bin/env python3
"""Makes the assistant helper's SwiftPM module caches again when its scratch path was made elsewhere.

`build/agent-swift` is the scratch path of both entry points that run SwiftPM on `agent/`: `make
agent-test` (debug) and `scripts/build_agent_helper.sh` (`make agent` and the app build's Build
Agent Helper phase; release by default). Each configuration keeps a Clang module cache,
`build/agent-swift/<triple>/<configuration>/ModuleCache`, which records its own absolute path, so a
`build/` copied from another checkout fails every compile with "PCH was compiled with module cache
path …" and "missing required module 'SwiftShims'". Both entry points run this script before
SwiftPM. One marker, `build/agent-swift/module-cache-made-at`, covers both configurations, since a
copy moves both; when it does not name this checkout's scratch path, every `ModuleCache` under it
is removed and SwiftPM makes it again. The rule and its lock are `probe_build.remake_if_moved`'s.

Run: `python3 scripts/agent_module_cache.py`, from any directory. It prints one line when it
removed the caches and nothing otherwise. Standard library and `probe_build` only; Python 3.9, the
`/usr/bin/python3` of the helper script's scrubbed `PATH`.
"""

from __future__ import annotations

from pathlib import Path

import probe_build

ROOT = Path(__file__).resolve().parents[1]
SCRATCH = ROOT / "build/agent-swift"
MARKER = SCRATCH / "module-cache-made-at"
# Beside the scratch path, not in it: SwiftPM owns what is inside.
LOCK = SCRATCH.with_name(SCRATCH.name + ".module-cache.lock")
# `<triple>/<configuration>/ModuleCache`; the `debug` and `release` links at the root are one level short.
CACHES = "*-apple-macosx*/*/ModuleCache"


def main() -> int:
    made_at = probe_build.remake_if_moved(MARKER, SCRATCH, SCRATCH.glob(CACHES), LOCK)
    if made_at is not None:
        where = f"was made at {made_at}" if made_at else "has no record of where it was made"
        print(f"agent_module_cache: build/agent-swift {where}; its module caches are made again", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
