# Contact-sheet admission drive

`ContactSheet.compose` admits `ContactSheetAdmission.composeScratch` (F +
a·sheet px + b·largest cell px) beside every distinct cell's own bytes before
the compose runs as a cancellable job, and the MCP `contact_sheet` /
`contact_sheet_from_sources` replies report it as `working_memory_plan_bytes`.
The measurement record (method, every per-run E, plan and P÷E per case) sits in
the `///` block beside those constants. This drive measures it and pins it, on
a built Release app.

```sh
make app
python3 -B scripts/contact-sheet-admission/measure.py \
  --app build/Build/Products/Release/Doctored.app [--output <folder outside the repo>] \
  [--runs 3] [--cases one-S12,sheet-98]
```

Every case runs in its own fresh owned app at `DR_WORKING_LIMIT_BYTES` 8 GiB
with `MallocLargeCache=0`, beside a baseline twin (the same sources, then
`resource_usage`), `--runs` times. Sources are MCP `new_document`s, white,
8-bit: S12 4000x3000, S48 8000x6000, S96 12000x8000, P48 8000x6000 Display P3,
L48 (S48 plus a `new_layer`). E is `ri_interval_max_phys_footprint` − pre
around `contact_sheet_from_sources {documents}`, read once the footprint has
been still for 2 s, so the delivered document's window and projection are
inside the interval. The cases: one cell each of S12, S48, S96, P48 and L48
(default layout); `sheet-57` (S12 x196, 14 columns, 512x512 cells),
`sheet-57-export` (the same, exported as PNG into a private folder) and
`sheet-98` (S12 x196, 14 columns, 680x680 cells).

- **M**: for every case, `working_memory_plan_bytes` ≥ ⌈1.25 × max E⌉.
- **R**: at 1 GiB, `sheet-98` from S12 is refused with "…on its own…" and the
  `list_documents` count is unchanged.

It imports `owned`, `call` and `footprint` from
`scripts/host-still-admission/verify.py`. Memory, not wall-clock: it may run
beside another lane's build. Run it outside the sandbox: it launches owned app
instances (`HostApp`: `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, a free
port, private `DR_*_DIR` folders) and stops only those. It writes
`receipt.json` to the output folder (a fresh temporary folder when `--output`
is omitted) and exits 0 only when both gates pass.
