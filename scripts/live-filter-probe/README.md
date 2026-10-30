# live-filter-probe

Shared Python modules that other drives import. The Phase 9A live-filter probe that lived
here (its Swift, `mcp.py`, `verify.py`, the oracle and `scripts/verify_phase9a.py`) went
with live filters on 2026-10-10 (`designs/convert-to-smart-object-status.md`).

| File | What it holds |
|---|---|
| `support.py` | The isolated app transport (`host-source-probe/support.py`) with this probe's private stores. |
| `native.py` | An independent length-framed DRDC source reader; never calls the product. |
| `fixtures.py` | The independent native-depth fixture writers. |
