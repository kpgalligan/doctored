# host-source-probe

Shared Python modules that other drives import. The Phase 8F host-source probe that lived
here (its Swift, `mcp.py`, `verify.py`, the numeric oracles and `scripts/verify_phase8f.py`)
went with retained host sources on 2026-10-10 (`designs/convert-to-smart-object-status.md`):
a host still or movie frame now opens and places as an ordinary raster layer.

| File | What it holds |
|---|---|
| `support.py` | Owned app processes and explicit document calls (`HostApp`, with every `DR_*_DIR` redirected). |
| `native.py` | An independent reader of the native source and resource tables; no product loader calls. |
| `fixtures.py` | The bounded, independent source and rational-frame fixture generator. |
