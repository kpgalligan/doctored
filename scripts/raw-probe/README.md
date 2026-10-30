# Retained RAW qualification

Run `python3 -B scripts/verify_phase8c.py --camera /path/to/camera.NEF --keep`
after the final Release app and Rust archive build. `--host-only` runs the
synthetic DNG host workflow; `--mcp-only --camera …` runs the genuine camera
workflow against the built app. `--library` selects an alternate coordinated
Rust archive. Core Image and AppKit require macOS graphics service access; an
all-zero sandbox rendering is a failure, never a reference image.

`camera-fixture.json` identifies one CC0 Nikon D70s 12-bit compressed CFA
photograph from raw.pixls.us, including its exact URL, size, SHA256 and license.
Download that one file separately into temporary storage and pass its path.
The runner requires the exact original and develops its native 3008×2000
pixels. The photograph is not committed. Synthetic DNG data qualifies host
state and error handling; it does not substitute for genuine camera data.

The host compiles actual app sources against a private immutable copy of the
release archive. It opens real documents and Develop controls, cancels with the
actual button, checks native NSDocument Save and saved identity, one-step
Undo/Redo, sparse recipe updates, Actions recording/replay, named snapshots and
private recovery. It also chooses the byte fallback in the actual initial
Develop UI and verifies ordinary PNG16/TIFF16 files named with a `.dng`
extension preserve every source code. Original files are removed before native
redevelopment. Exposure-only changes must retain exact saved white balance;
returning exposure to its prior value must restore every rendered code.

`reference.swift` compiles independently of app/core code and renders the real
camera at native resolution directly through CIRAWFilter into UInt16 storage.
The initial and changed white-balance/exposure references have a fixed maximum
error of three u16 codes against every saved developed master sample. The
reference also requires opaque alpha, more than 4,096 distinct RGB codes and
millions of components that cannot come from 8-bit promotion. This qualifies
the storage/development bridge against the platform decoder, not the decoder's
photographic color accuracy against a calibrated camera target.

`native.py` independently reads the DRDC v18/v19 source trailer, exact original
RAW payload, descriptor, native PNG masters and ordinary layer PNG fallbacks.
The v18/v19 trailer is read directly or inside the DRDC 45/46 + `LIDS` identity
wrapper, which `identity_body` peels, together with an optional source
profile-refusal word 56/57 and its `RSPR` table (`source_profile_refusals`),
warp word 53/54 and its `WARP` table (`warp_entries`) and byte style-knockout
word 49 (`knockout_word`).
It decodes PNG8/16 and the qualified TIFF16 strips using Python's standard
library. The camera workflow checks both byte and typed fallbacks, retained
retouch pixels (including exact genuine UInt16 values in opaque cloned areas),
all exported pixels against an independent source-over oracle,
fresh-process portability and explicit redevelopment after deleting the
original path. Changing only the saved decoder implementation descriptor must
leave exported rendering identical while redevelopment refuses.

All app processes, ports, Actions libraries and recovery directories belong to
the runner; credentials are dummy. Reports include source hashes, file-set
checks, library/app hashes and process IDs. Source changes during qualification
fail the run. This is one 6 MP camera workflow plus a small synthetic state
workflow; it makes no larger workload, other-camera, HDR or cancellation claim.
