# Phase 9D — native spot-healing synthesis qualification

The native Spot Healing Brush runs the existing component planner, PatchMatch
pyramid/search/vote and membrane solver with genuine u16 source/destination
storage. RGB stays in native-code f32 through the pyramid, synthesis and solve;
only final destination publication quantizes. No byte image supplies search,
source colour or solved RGB. The shared byte path retains its rounding policy.
Binary topology still uses the established contour implementation; that is
distinct from RGB and from the original u16 write coverage.

Source sampling requires alpha at least 32768, matching the byte path's half-alpha
rule. Destination alpha remains exact, including alpha 1; fully transparent
destination storage and pixels outside the stroke's half-coverage contour remain
unchanged. Source-profile conversion/composition happens in the retouch dispatcher
before this pipeline receives its captured canvas plate.

## Verification

`core/tests/precision_inpaint.rs` exercises the public `rz_precision_doc_edit`
FFI. Its seven groups pass, including the optional photographic group when run
with `RZ_RETOUCH_PHOTOGRAPHS=/private/tmp/rz-phase9c-photographs`. The committed
`samples/portrait-exif6.jpg` is a synthetic coloured texture, so this qualification
does not call that fixture a photograph.

- The 32×32 periodic fixture with an 8×8 hole at (12,12) reconstructs within one
  native code for stripe contrasts 1, 91 and 20000. Adjacent native codes remain
  distinct; repeated seed 12345 is exact; native reopen preserves the result.
- Independent source-layer and Display P3 checks recover the expected constant
  destination colour while preserving alpha 1 and the unchanged source layer.
- Native coverage 32767 stays unchanged, 32768 is inside the spot contour,
  alpha-zero hidden RGB stays exact, and alpha 1 remains usable destination
  storage. Source alpha below 32768 refuses when no complete source patch exists.
- Mask/bounds mismatch, insufficient memory, precancellation and empty coverage
  are atomic. The original document never changes on refusal or cancellation.
- A 512×512 fixture with a 270×270 stroke at (120,120) waits until native stage 3
  and nonzero progress have been observed, then cancels. The recorded run returned
  in **42.916 microseconds**, below the fixed two-second gate, and reported
  **zero retained scratch reservation** after the worker returned. The source
  remained exact. The timer starts after native work, not before dispatch.

The pre-existing 37 content-aware correctness, five content-aware cost and four
typed membrane-reference tests also pass.

The final byte-compatibility probe links the same C fixture driver against the
preserved pre-change archive and the final archive. **Twenty cases** (four seeds
over interior, canvas-edge, soft-coverage, partial-alpha and larger-pyramid
fixtures) produce **2,826,240 directly compared, byte-identical RGBA bytes**,
SHA-256 `7498cf8fab3c4e51e1fcc05c13fc0d384ea1fe15ddc90b87e83de86f07912b5c`.
The paired synthesis run took 1.623936 versus 1.581636 CPU seconds (ratio 0.974);
this is a compatibility/cost check on that corpus, not a general speed claim.
The initial generic byte SSD loop had measured a 46.5% slowdown. Restoring the
integer row kernel removed it while the typed row kernel retains f64 arithmetic.
The byte window scratch allowance increases from 128 to 136 bytes per pixel
because the shared nearest-neighbour distance array now stores f64 values.

The temporary reproduction inputs are
`/private/tmp/rasterize-9d-byte-synthesis.c` (SHA-256
`f792f5fbfa032bbd5440b2f7ce0dbad3c8d422d56f5fd3513e63eca9eeeb8645`),
`/private/tmp/rasterize-9d-before-synthesis.a` (SHA-256
`747117b618196bfe31deef952fc013d214daa77a16079a97c178f7db93198fe7`), and
the final `core/target/release/librasterize_core.a` (SHA-256
`0e523b28d7d2c7a3625025ca584dc1be903a07881842b753bab3aa480d486a58`).
The direct comparison was rerun against this final archive after the source-API
retained-source output and compound-progress fixes; all 2,826,240 bytes remain identical. The
earlier paired timing used archive `5af18a52ca43cb80209c8b9225d56d4192eef6bb2d40a6bfcdff700ddf0a611d`
with the same synthesis code. No new timing claim is inferred from the final
correctness rerun.

## Photographic check

These are the already licensed, checksum-pinned Phase 6/9C photographs, copied
into private scratch. The test verifies their SHA-256 before decoding. Neither
fixture nor the original photographic precision is represented as a native
16-bit camera capture: source RGB is the decoded byte value ×257 plus an explicitly
generated 0…30-native-code carrier (channel offsets `i%31`, `7i%31`, `11i%31`,
clamped to 65535). This checks photographic structure alongside sub-byte data.

| Fixture | SHA-256 | Image | Corruption location |
| --- | --- | --- | --- |
| NASA astronaut | `88431cd9653ccd539741b555fb0a46b61558b301d4110412b5bc28b5e3ea6cb5` | 512×512 | radius-four disc centred at (197,122), bounds (193,118,9,9) |
| Chelsea cat / fur | `596aa1e7cb875eb79f437e310381d26b338a81c2da23439704a73c4651e8c4bb` | 451×300 | radius-four disc centred at (220,68), bounds (216,64,9,9) |

Each disc's 49 pixels is replaced with native RGB (60001,3001,50003), retaining
alpha. Both fills use seed 12345, automatic ring, current-layer sampling and full
strength. Before running, the gates were set to **mean absolute RGB error below
15 byte-code equivalents**, **remaining error below 25% of the corruption's
error**, and **every pixel outside coverage exact**. Error is measured against
the uncorrupted photograph plus its identical native carrier, not against another
fill or a generated expected result.

| Fixture | Mean absolute error after repair | Corruption error remaining | Outside coverage |
| --- | --- | --- | --- |
| Astronaut | **10.8037 /255** | **13.489%** | Exact |
| Fur | **9.1293 /255** | **8.358%** | Exact |

This qualifies small photographic spots on these two sources. It does not claim
an arbitrary large removal is visually indistinguishable from the original.

## Preserved failure and boundaries

An initial test demanded exact periodic reconstruction for a **24×24 hole at
(52,52) in a 128×128 period-four texture**. It failed: coarse reduction removes
texture phase, and approximate matching can reconstruct the opposite phase in
the interior. Maximum reconstruction error was the stripe contrast itself:
**1, 91 and 20000 native codes**, respectively. These results remain printed and
documented; that exact-reconstruction gate is not reported as passed. The larger
case now checks only the analytic source range, exact outside coverage and
survival of adjacent native values. Those checks establish precision and
confinement, while the separate photographic gates establish the qualified
small-spot quality. They do not establish correct phase for larger periodic fills.

The shared planner retains its existing limits: one million hole pixels, four
million dilated pixels, 80 million cumulative window pixels, and the existing
single-window solve cap. Native memory admission can refuse earlier: its
conservative forecast includes 320 bytes per largest window pixel, component
storage and the caller's captured plates/output. These are computational ceilings,
not photographic quality guarantees. The variance floor is one squared native
code, so sub-byte texture participates in matching; it does not inherit a
257-native-code noise floor.

Source exclusion editing, final candidate comparison and Content-Aware Fill
output choices remain Phase 9E. General scalar targets and authoritative native
selection creation remain Phase 9G.
