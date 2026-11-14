# Phase 13C — Incremental I/O and scratch (status)

Status: OPEN. Items 2 and 3 of [#11](https://github.com/kpgalligan/doctored/issues/11): a save and an open
whose peak memory is bounded, spilling to scratch where a table cannot stream, and recovery and `.drsnapshot`
payloads stored as shared, deduplicated, compressed tiles. This doc is the design; no 13C-1 to 13C-6 code has
landed. The roadmap's 13C line links here.

## Shipped

- **Stale temporaries** (093f57db). Every core save first sweeps its destination folder of
  `<name>.dr-tmp-<pid>-<seq>` files whose process is gone (`core/src/stale_temp.rs`); the app's staged files carry
  their pid (`app/Sources/StagedFile.swift`) and are swept by the same rule. An interrupted or disk-full save keeps
  the previous file and leaves no staged name (`scripts/verify_save_interrupt.py`).
- **Cancel in Save and Open** (b979f919). The native, flat and PSD saves and opens take a single-use control; reads
  and writes poll every 64 KiB (`core/src/cancel_io.rs`); a cancel answers `operation cancelled` and keeps the
  previous file.

## What each path holds at peak now (read 2026-11-14)

| Path | Peak beside the document | Streams? |
| --- | --- | --- |
| Byte native save (`drdc::save_native_controlled`) | the whole encoded file (`encode_native` returns a `Vec`), then `cancel_io::write_file` | no |
| Byte native open (`DrDocument::open_controlled`) | the whole file (`cancel_io::read_file`), then `drdc_read::parse_native` decodes from it | no |
| Typed native save (`precision_native_write`) | `write_scratch_bytes`: a 1 MiB `BufWriter` and one plane's IDAT window (`native_stream::FileSink`) | yes |
| Typed native open (`precision_native_plan`, `native_stream::Reader`) | positional `pread`s; payloads decoded band by band into tiles (`precision_png_rows`) | yes |
| Flat export: JPEG, BMP, TIFF | the encoder's own rows | yes |
| Flat export: PNG, WebP, GIF, JPEG XL | the whole encoded output before the first write | no |
| Flat and PSD open | the whole file's bytes, then the decoded image | no |
| Recovery generation, `.drsnapshot` | one full native image per state: the editing state and every named snapshot (up to 32), each generation | per its writer |

## Decisions

### D1. The tile pool

A recovery folder and a `.drsnapshot` package each hold ONE pool: content-addressed tile payloads in append-only
pack files (`pack-<n>`, at most 256 MiB each) and an index (`pool.index`: key → pack, offset, stored length, codec).
A generation or a snapshot state is no longer an `image.rz`: it is the small non-pixel records (the native format's
tables, slots and descriptions, as the typed writer emits them today) plus, for every tiled plane, its geometry and
its list of tile keys.

- **Key.** SHA-256 over the sample type tag, the tile's width and height, and its sample bytes in the tile's
  little-endian layout. Why: the tile store's one equality (`TilePlane::same_bits`) is bitwise, and a hash of the
  bytes is that equality on disk; the type and size in the key keep a u8 tile from colliding with a u16 one of the
  same bytes; a 256-bit digest needs no read-back to rule out a collision. The core has no hash crate (the source
  fingerprint's SHA-256 is the host's CryptoKit, and handing every tile across the FFI to hash it would copy it),
  so 13C-1 writes the core's own FIPS 180-4 SHA-256 in a kernel module, checked against the standard's
  known-answer vectors. Adding a hash crate instead would be a new dependency, which is the owner's call.
- **Identity memo.** Per session, a map from a tile's `Arc` identity (its sample pointer, held with the `Arc` so the
  address cannot be reused) to its key. Why: an untouched tile is the same `Arc` across undo states, snapshots and
  generations (13B's sharing latch), so it is hashed once per session, not once per write.
- **Deduplication** falls out of the key: two layers, a layer and its snapshot, or a RAW master's tiles and a
  rendered layer with the same bytes store one payload.
- **Compression.** Each payload deflates through `miniz_oxide` (already a dependency) at level 1 and is stored raw
  when deflate does not shrink it. A u16 or f32 tile is byte-shuffled (high bytes, then low) before deflate, which
  is what makes high-depth noise compress; the codec byte in the index names raw, deflate or shuffle-deflate.

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
  generation exactly as a damaged `image.rz` does now; a package refuses with the damaged state's name.
- The recovery lease (`RecoveryLease`) and the publication gate are unchanged: one writer per folder.

### D3. Bounded save and open

- **Byte native save** streams as the typed one does: the byte writer (`drdc`) gets a `NativeSink` writer that
  emits each layer's PNG plane tile row by tile row, so the peak is one tile row's planes, not the file.
- **Byte native open** reads through `native_stream::Reader` (positional reads, spans) and decodes each plane band
  by band into tiles, the typed reader's model.
- **Flat exports that build their output in memory** (PNG, WebP, GIF, JPEG XL) write their encoder's output to a
  SCRATCH file in the staged file's folder instead of a `Vec`, then copy it into the staged file. The scratch is
  named `<name>.dr-tmp-<pid>-<seq>` so the 093f57db sweep removes it after a crash; a disk-full scratch write fails
  the save with the previous file kept, as a disk-full staged write does now.
- **Flat and PSD opens** keep reading the whole file: the decoders (`image`, the PSD parser) take a slice. A file
  larger than the working limit already refuses at admission; streaming them is out of 13C (see Open items).
- **The `.doctored` format does not change.** Its pixel payloads are already PNG-compressed per plane, and the
  streaming writer and reader above need no new word. A tile-pool or compressed-payload `.doctored` would be a new
  conditional DRDC word (the next free is 115) with every prior word still read; 13C does not need one.

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
2. **13C-2 — Byte native streaming.** D3's byte writer and reader. Check: `make t F=native_save_bytes` (the bytes
   are unchanged) plus a counted test that the writer's peak reservation is one tile row's planes, not the file.
3. **13C-3 — Recovery on the pool.** `RecoveryStore` writes a generation as a skeleton per state plus pool
   appends, collects after the rename, and reads the older `image.rz` generations. Check: the recovery drive, plus a
   new drive case where a second checkpoint after a one-layer edit appends only that layer's changed tiles
   (counted from the pool's index, not timed).
4. **13C-4 — Packages on the pool** (after D4's answer). Check: the snapshots drive; an older-version package still
   opens.
5. **13C-5 — Scratch-spilled flat exports.** Check: `make t F=export_codec_cancel` and `scripts/verify_save_interrupt.py`
   stay green; a test that the PNG export's staged write never holds the whole encoded image (its reservation).
6. **13C-6 — Measurement** (needs a quiet machine and the 16 GB Mac, so it runs with 13D): #11's "a recovery
   generation that changes one layer writes about that layer's bytes" and the save/open peak figures, on the 24 MP
   corpus.

## Open items

- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 13C-1 to 13C-5 above, and D4's owner answer.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — 13C-6's figures (quiet machine, 16 GB Mac).
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — Flat and PSD opens still read the whole file
  (D3); the decoders that still cancel only between stages (ledger row P7-32).
