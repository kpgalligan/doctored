# Phase 13C — Incremental I/O and scratch (status)

Status: OPEN. Items 2 and 3 of [#11](https://github.com/kpgalligan/doctored/issues/11): a save and an open
whose peak memory is bounded, spilling to scratch where a table cannot stream, and recovery and `.drsnapshot`
payloads stored as shared, deduplicated, compressed tiles. 13C-1 (the pool core), 13C-2a (the streamed byte
save), 13C-2b (the streamed byte open), 13C-3 (recovery on the pool) and 13C-5 (the streamed 8-bit PNG) have
shipped, and mask, channel and 8-bit source planes are pool tiles; 13C-4 and 13C-6 are open. The roadmap's 13C
line links here.

## Shipped

- **Stale temporaries** (093f57db). Every core save first sweeps its destination folder of
  `<name>.dr-tmp-<pid>-<seq>` files whose process is gone (`core/src/stale_temp.rs`); the app's staged files carry
  their pid (`app/Sources/StagedFile.swift`) and are swept by the same rule. An interrupted or disk-full save keeps
  the previous file and leaves no staged name (`scripts/verify_save_interrupt.py`).
- **Cancel in Save and Open** (b979f919). The native, flat and PSD saves and opens take a single-use control; reads
  and writes poll every 64 KiB (`core/src/cancel_io.rs`); a cancel answers `operation cancelled` and keeps the
  previous file.
- **13C-1, the pool core** (#11). `sha256.rs`, `tile_pool_codec.rs` and `tile_pool.rs` (kernels),
  `pool_skeleton.rs`, `doc_tile_pool.rs`, `ffi_tile_pool.rs` (`dr_tile_pool_open`/`free`/`info`/`rebuild`/`collect`,
  `dr_doc_tile_pool_write`, `dr_precision_doc_tile_pool_write`, `dr_tile_pool_window_bytes`). The native writers
  offer each tiled raster through `NativeSink::raster` and announce the other large payloads through
  `NativeSink::begin_blob`; a file or `Vec` sink ignores both, so save bytes are unchanged. Check: `make t
  F=tile_pool` (FIPS vectors; byte, u16, float and CMYK rebuilds equal to `save_native`'s files; dedup, edit and
  codec counts; damage, torn index and collection), each claim also seen red under a mutant.
- **13C-2a, the byte native writer to a sink** (#11). `DrDocument::write_native` writes in order to a
  `NativeSink` (no `encode_native` `Vec`, no `ByteMark` walk: the byte pool write runs it into the skeleton sink);
  each layer, channel and 8-bit source PNG is streamed row by row by `png_rows8` (a private copy of png 0.18.1's
  adaptive filter over fdeflate, `PngEncoder`'s bytes); `NativeSink` gains `patch`, `truncate` and `rewrites`,
  and a sink that cannot rewrite (the skeleton) gets each PNG measured first, never staged.
  `write_scratch_bytes = (encoded_bound − streamed_bound) + held + 5·(c·w + 1) + 1 MiB + window + bookkeeping +
  overhead` (`drdc_save_plan`; gate M kept as the former writer's record). Check: `native_rows8_bytes` (each PNG
  record equals `PngEncoder`'s; red under the tie, order and stored-fallback mutants), `native_save_plan8_45mp`
  (a save adds about 1.3 MB at gate M's sizes, 1.59 GB before; the recovery write stays under its admitted
  figure, red under a staging mutant), `native_save_bytes` untouched.
- **13C-2b, the byte native reader over the stream** (#11). `DrDocument::open_controlled` sniffs a native file
  (`native_magic::file_is_native`) and streams it: `parse_native` drives `native_stream::Reader` over
  `NativeFile::open_byte`, the layer PNG, raw mask and channel PNG are spans, and `png_rows8_read` (a u8 twin of
  `precision_png_rows`) decodes the writer's form (8 bits, colour type 6 or 0, not interlaced, IHDR/IDAT/IEND
  only), the masks and the 8-bit sources row by row into tiles. Any other stored PNG, and a version 2–9 mask over
  `MAX_PIXELS` (which the tile store refuses), decodes whole as before. `drdc_reopen_plan`: held term `encoded_bound
  − streamed_bound + NATIVE_STREAM_WINDOW_BYTES`, T the band figure; gate R′ kept as the former reader's record and
  `DECODE_OVERHEAD_BYTES` unchanged until 13C-6. Check: `native_save_plan8_45mp`'s open cases (S2 adds 184.3 MB for
  a document holding 181.8 MB, 545.4 MB before; S1 within three planes and a band and within `decode_bytes`; red
  on the buffered open and under a whole-decode mutant; a version-2 mask over `MAX_PIXELS` still opens),
  `native_open_stream8` (round trip, fallback forms, truncation, corruption), `save_open_cancel`'s native floor
  one pass.
- **13C-5, the streamed 8-bit PNG** (#11). Flat saves and exports write the 8-bit PNG through `flat_png` over
  `png_rows8`, the profile, EXIF and splice chunks taken from `PngEncoder`'s encode of one pixel; `encode_into`'s
  PNG arm is unreached. Check: `flat_png_rows` (plain and with metadata, chunk for chunk; red under a reordered
  and a dropped chunk slice), the 45 MP flat case (about 1.4 MB added, 493 MB before),
  `scripts/verify_save_interrupt.py` with an 8-bit PNG case.
- **Mask, channel and 8-bit source planes as pool tiles** (#11). Skeleton version 2 (version 1 still read):
  sites 5–8, a byte mask's rows, a byte channel's PNG record, a typed plane's samples and an 8-bit source's PNG
  record, each rebuilt by the writer's own function; sites 5–8 in a version 1 skeleton are damage. Check:
  `tile_pool` (a 300×300 byte or typed mask or channel dab appends the 4 tiles it meets and no chunk; a version 1
  skeleton still rebuilds; red on the previous writer and under a version-2-only reader) and `verify_phase5.py
  --suite recovery`'s `pool_mask_checkpoints` (red against the previous Debug app: one chunk, no tile).
- **13C-3, recovery on the pool** (#11). A generation's states and session snapshots are
  `image.drskel` skeletons beside `<recovery>/pool` (`RecoveryPool.swift`, `EditingStateArchive.write/read`);
  the manifest is version 2 and version 1 generations (`image.rz`) still read; the pool is collected after the
  cleanup over every skeleton the kept generations hold. A record's `RecoveryPoolBox` keeps the handle (and the
  memo) between checkpoints, drops it on any outcome but a published manifest, and `clear` replaces the box.
  `write_recovery` reports `pool` and takes `native_copy` (the checkpoint rebuilt as a native file, how a drive
  reads a leased checkpoint: `scripts/recovery_checkpoint.py`). Check: `verify_phase5.py --suite recovery`'s
  `pool_checkpoints` (a 300×300 edit appends at most the 4 tiles it meets; a damaged payload only the newest
  generation names falls back to the previous one; an `image.rz` generation recovers).

## What each path holds at peak now (read 2026-11-15)

| Path | Peak beside the document | Streams? |
| --- | --- | --- |
| Byte native save (`drdc::save_native_controlled`) | the small fields between planes (a slot's payload and a table built before the body go from their own buffers), one plane's encoder rows (`png_rows8`) and a 1 MiB `FileSink` | yes |
| Byte native open (`DrDocument::open_controlled`) | positional `pread`s (`native_stream::Reader` over `NativeFile::open_byte`); each plane decoded band by band into tiles (`png_rows8_read`); another writer's PNG whole | yes |
| Typed native save (`precision_native_write`) | `write_scratch_bytes`: a 1 MiB `BufWriter` and one plane's IDAT window (`native_stream::FileSink`) | yes |
| Typed native open (`precision_native_plan`, `native_stream::Reader`) | positional `pread`s; payloads decoded band by band into tiles (`precision_png_rows`) | yes |
| Flat export: JPEG, BMP, TIFF, 8-bit PNG | the encoder's own rows (a PNG export's result is its `Vec`) | yes |
| Flat export: 16-bit PNG, WebP, GIF, JPEG XL | the whole encoded output before the first write | no |
| Flat and PSD open | the whole file's bytes, then the decoded image | no |
| Recovery generation | a skeleton per state; the pool appends only tiles it lacks, one tile and its codec buffers at a time (`TILE_POOL_WINDOW_BYTES`); masks, channels and 8-bit sources are tiles too | yes |
| `.drsnapshot` | one full native image per state: the editing state and every named snapshot (up to 32) | per its writer |

## Decisions

### D1. The tile pool

A recovery folder (and, after D4, a `.drsnapshot` package) holds ONE pool: content-addressed payloads in
append-only pack files (`pack-<n>`, at most 256 MiB each) and an index (`pool.index`: key → pack, offset, stored
length, codec). A state is a SKELETON (`pool_skeleton.rs`): the native file with each tiled plane (a layer, a
master, a mask or channel plane, an 8-bit source) replaced by its geometry and tile keys, and each other large
payload (a RAW payload, a contiguous plane, 64 KiB or more) replaced by the keys of its 1 MiB chunks; everything
else is literal. Masks, channels and 8-bit sources became tile segments in skeleton version 2 (2026-11-15), so a
mask or channel dab appends the tiles it meets; version 1 skeletons still read. The rebuild writes the native file
back through the writers' own encoders, byte for byte, and the existing readers open it: a recovery reopen
re-encodes and decodes each raster once, after a crash, never per edit.

- **Key.** SHA-256 over the sample type tag, the tile's width and height, and its sample bytes in the tile's
  little-endian layout. Why: the tile store's one equality (`TilePlane::same_bits`) is bitwise, and a hash of the
  bytes is that equality on disk; the type and size in the key keep a u8 tile from colliding with a u16 one of the
  same bytes; a 256-bit digest needs no read-back to rule out a collision. The core has no hash crate (the source
  fingerprint's SHA-256 is the host's CryptoKit, and handing every tile across the FFI to hash it would copy it),
  so 13C-1 writes the core's own FIPS 180-4 SHA-256 in a kernel module, checked against the standard's
  known-answer vectors. Adding a hash crate instead would be a new dependency, which is the owner's call.
- **Identity memo.** Per pool handle, a map from a tile's address to `(Weak<Tile>, key)`. Why: an untouched tile
  is the same `Arc` across undo states, snapshots and generations (13B's sharing latch), so it is hashed once per
  handle, not once per write. The `Weak` pins the allocation (its header, not its samples), so the address cannot be
  reused while the entry exists; dead entries are pruned at each write. A memo hit still asks the pool, so a tile a
  collection removed is appended again.
- **Deduplication** falls out of the key: two layers, a layer and its snapshot, or a RAW master's tiles and a
  rendered layer with the same bytes store one payload.
- **Compression.** Each payload deflates through `psd_zip` (`miniz_oxide`, the crate's one zlib seam) at level 1.
  A u16 or f32 payload is deflated both plain and byte-shuffled and the smaller is kept, since the spike measured
  shuffle winning on noisy high-depth data and losing on clean data; raw is kept when neither shrinks it. The codec
  byte in the index names raw, deflate or shuffle-deflate.

### D2. Publication and collection

The two-generation rule (`app/Sources/RecoveryStore.swift`) stays: a generation is complete before the manifest
names it, and at most two committed generations are kept.

- A generation's new tiles are appended to the open pack and the index, both fsynced, BEFORE the generation's
  record is written and before the manifest rename; a pack is never rewritten in place.
- After the manifest rename, the pool is collected: a tile no surviving generation (and, in a package, no state)
  references is dead; a pack whose live bytes fall under half its size is copied forward (its live tiles appended
  to the open pack, fsynced) and then removed. Collection never runs before the rename, so a crash leaves at most
  unreferenced bytes, never a dangling key.
- A key whose payload is missing, short or fails its hash is DAMAGE: the reader falls back to the previous
  generation exactly as a damaged `image.rz` does now; a package refuses with the damaged state's name. The handle
  forgets a damaged payload, so the record's next checkpoint (the recovered document keeps the handle that read it)
  stores that content again and its new index entry wins.
- An append writes payloads first and keeps their entries pending; the pool's flush barrier-flushes the packs, then
  appends and flushes the entries, before the skeleton's rename. Open truncates a torn index tail and rewrites the
  index without an entry whose pack is missing or too short.
- The recovery lease (`RecoveryLease`) and the publication gate are unchanged: one writer per folder.

### D3. Bounded save and open

- **Byte native save** streams as the typed one does: the byte writer (`drdc`) gets a `NativeSink` writer that
  emits each layer's PNG plane tile row by tile row, so the peak is one tile row's planes, not the file.
- **Byte native open** reads through `native_stream::Reader` (positional reads, spans) and decodes each plane band
  by band into tiles, the typed reader's model.
- **Flat exports that build their output in memory** (PNG, WebP, GIF, JPEG XL): the encoders already write to the
  staged file; what they hold whole is their own output (13C-5 below has the corrected plan: PNG through 13C-2a's
  row encoder, the others left as they are).
- **Flat and PSD opens** keep reading the whole file: the decoders (`image`, the PSD parser) take a slice. A file
  larger than the working limit already refuses at admission; streaming them is out of 13C (see Open items).
- **The `.doctored` format does not change.** Its pixel payloads are already PNG-compressed per plane, and the
  streaming writer and reader above need no new word. A tile-pool or compressed-payload `.doctored` would be a new
  conditional DRDC word (the next free is 116) with every prior word still read; 13C does not need one.

### D4. Held for the owner: the `.drsnapshot` package format

A `.drsnapshot` is a file a person keeps and may move between machines, so changing its inside changes a format
they would notice: a package written by a 13C build would not open in an older build.

- Options: (a) packages move to the pool too (one pool per package, deduplicated across its states, compressed);
  (b) packages keep `image.rz` per state and only recovery uses the pool.
- Recommendation: (a), with the package's `version` raised so an older build refuses it by name ("This snapshot was
  saved by a newer Doctored") rather than misreading it, and the reader keeping the current package version. The
  saving is the same as recovery's, and a package of 32 states of one document is where it is largest.
- 13C-3 builds recovery only; 13C-4 waits on this answer.

## Sub-slices, in build order

Each slice's check is a core test through the FFI (red without the change) unless it says otherwise; the drives are
`scripts/verify_phase5.py --suite recovery` and `--suite snapshots` against an owned Debug instance.

1. **13C-1 — Pool core.** A new kernel module for keys (SHA-256), the shuffle-deflate codec and the pack/index
   format; FFI `dr_tile_pool_*` to append a document's tiles (returning its skeleton) and to rebuild a document from
   a skeleton and a pool. Check: the SHA-256 known-answer vectors; a byte and a typed document round-trip bit for
   bit; a second write of the same document appends nothing; a damaged payload refuses by name.
1. 13C-1 shipped (above).
2. 13C-2a shipped (above).
3. 13C-2b shipped (above).
4. 13C-3 shipped (above).
5. **13C-4 — Packages on the pool** (after D4's answer). Check: the snapshots drive; an older-version package still
   opens.
6. 13C-5 shipped (above). WebP, GIF and JPEG XL have no streaming form, and a file-backed export result is a
   separate item to weigh against its saving (the spike's photo: 22.5 MB encoded beside 48.8 MB of flattened
   pixels).
7. **13C-6 — Measurement** (needs a quiet machine and the 16 GB Mac, so it runs with 13D): #11's "a recovery
   generation that changes one layer writes about that layer's bytes" and the save/open peak figures, on the 24 MP
   corpus.

## Open items

- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 13C-4 above, and D4's owner answer.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 13C-6's figures (quiet machine, 16 GB Mac),
  including the pool write's own peak and SHA-256 cost: recovery admits the native save's scratch plus
  `TILE_POOL_WINDOW_BYTES`, an upper bound a measurement may lower.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — Flat and PSD opens still read the whole file
  (D3); the decoders that still cancel only between stages (ledger row P7-32).
