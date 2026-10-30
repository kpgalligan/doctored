# Phase 8G — camera controls and local RAW development

## Completion

**Phase 8G is complete within the qualified scope below, completing Phase 8.**
Implementation followed the separate 8F commit
`9b3f65cf255cc024c89ba03fce0dddd32a3dc902`. Historical 8C–8F measurements remain
unchanged in their existing status files. Phase 9 is next and has not started.

Final qualification: all 56 actual-source host/runtime groups pass,
including both-depth Develop controls and genuine 6 MP navigation/feather/close.
The independent camera oracle compares 577,536,000 source components (maximum
one u16 code, fixed bound three); eight complete native masters retain all
192,512,000 components exactly. Analytical masks differ by at most one code,
ordered exact-input blend error is at most 0.000007749367 against
`2e-6 + 0.5/65535`, and profile-aware vignette differs by at most one code against
the fixed three-code bound. Final host/UI qualification includes the compact typed RAW stack-summary fix,
memory-pressure/refusal checks and real 2× native gesture geometry. All 701 used input
hashes/file sets and 350 compiled inputs remain exact; the native strict-command
and migration budget/cancellation findings are corrected. The final Rust suite passes 1,147 tests with no failures and two existing ignored
tests. Final Release and authoritative Swift typecheck pass. External MCP passes
38 groups / 760 tool calls, with 145,956,864 exact persisted source components;
all 728 input hashes/file sets remain exact and all eight owned app processes exit.

- Native core: immutable source-coordinate u16 masks, ordered local composition,
  profile-aware vignette, attachment sharing, admission/cancellation and tests.
- Host workflow: recipe 3, exact old-recipe compatibility, sequential alternate
  RAW development/cache, preset/sync and migration preservation, MCP/Actions.
- Develop UI: qualified camera controls, a retained local session, native overlay
  and source-coordinate gestures, one prepared Apply and complete Cancel cleanup.
- Final verification: independent small-array oracles and full-size Nikon D70s
  reference developments, both document depths, native/snapshot/recovery/Undo,
  actual UI and external MCP, then complete repository checks and artifact audit.

## Frozen evaluation and storage contract

The source pipeline is original camera bytes → pinned global camera/vendor lens
development for the base and each enabled local variant → selected sRGB or Display
P3 integer u16 output → ordered native coverage blending → calibrated manual
vignette → existing retained layer affine/profile/fallback → layer mask/retouch.
Variants retain the same decoder, orientation, actual dimensions and output ICC.
Local blending keeps a float accumulator through the vignette and quantizes once
at final u16 source storage. It uses the established encoded SDR policy; this introduces no linear
or HDR document mode. Zero coverage, disabled/identity locals and neutral vignette
must bypass exactly.

At most four ordered locals use top-left, oriented, full-source pixel coordinates
before layer geometry. Coverage is native u16; radial/linear primitives, brush
add/subtract, inversion, density and feather never derive authoritative coverage
from a byte preview. Each mask is at most 64 MP. Original RAW plus masks retain
the existing 256 MiB per-resource and 512 MiB aggregate binary-resource limits.
Versioned `RZLM` mask blobs use the 8F native 21/22 pool under `raw_local_masks`;
mask pixels never enter the capped JSON recipe. Unchanged immutable masks/originals
share storage across revisions. Every recognized mask header/length/reference is
validated before native pixel decoding.

Recipe 3 adds camera-relative baseline compensation, supported moiré reduction,
manual vignette and ordered local exposure/WB intent plus captured effective
variants. Recipes 1/2 and strict binary64 persistence retain their existing
meaning. Global transfer preserves target-local masks and camera-specific defaults.
Same-geometry migration preserves masks exactly. Phase 12F-4 subsequently added
reviewed EXIF/crop/pad and filtered affine remapping of genuine u16 local masks;
unregistered extent changes still refuse without an explicit inverse map.

## Gates fixed before implementation

1. Exact old recipe 1/2 white-balance, profile, no-op, native and Undo behavior.
2. Independent analytical source-coordinate coverage; fractional coordinates,
   more than 256 levels, one-code differences, density/feather/inversion and exact
   persisted samples. Geometry uses declared pixel-center field sampling.
3. Ordered blend agreement with a float64 oracle within `2e-6 + 0.5/65535`, exact
   outside-mask/disabled/neutral branches and exact opaque camera alpha.
4. Independent sRGB/P3 vignette and calibrated exposure within **3 u16 codes**,
   exact neutral/center bypass, and correction of independently synthesized falloff.
5. Independent complete SDK base/local camera develops within **3 u16 codes**;
   separately exact captured-variant composition checks. Local negative exposure
   must reveal original-derived detail unavailable by darkening a clipped base
   rendering; this makes no claim about recovering sensor-saturated information.
6. Full-size genuine Nikon D70s both-depth local/retouch/redevelop, original deletion,
   native/snapshot/recovery, exact masks/ICC/original bytes and typed PNG output.
7. Same-geometry reviewed migration, exact accepted result/planes, Undo and Actions;
   stale/expired candidates and changed geometry refuse without publication.
8. Actual fit/100%/pan/Retina/affine UI gestures and overlay; all local edits through
   MCP/Actions, strict malformed requests, no-op history and atomic sync refusal.
9. Admission, preallocation caps, shared-resource accounting, stage cancellation
   and post-close cleanup. CI decode remains noncancellable within one measured
   decoder stage; Rust coverage/composition/vignette cooperate with cancellation.
10. Target-15 archive, formatting, all-target Clippy, full Rust tests, authoritative
    Swift typecheck, Release app and independent/host/UI/MCP qualification against
    exact final source/header/Cargo/build/helper/artifact hashes.

## Historical camera reconnaissance and qualified matrix

The independent SDK reconnaissance uses macOS 26.5.1 (25F80), CIRAW decoder `8`:
Nikon D70s NEF, 3008×2000, baseline −0.55000001192092896 EV. Moiré is supported;
vendor lens correction and local-tone-map controls are unavailable for this file.
Baseline +0.5 EV and moiré 0→1 both measurably affect genuine source data; moiré 0
is exact. A separate decoder `8.dng` synthetic CFA verifies oriented geometry
(384×512 output from a 512×384 native size). These reconnaissance findings preceded implementation. The final product
qualification below covers this declared matrix; it makes no all-camera/future-OS claim.

Broader camera/decoder and source-heavy scale qualification remains 13D; typed
capture-packet/PPI and auxiliary-matte work remains 11D/9G. General lens databases,
distortion/TCA and additional photographic operators retain their named follow-on
tracks. This slice must complete the bounded local RAW scope above before closing.

## Qualification

The final host report is `/tmp/rz-phase8g-host-final.json`. Its 56 groups comprise
16 model/integrity groups, 18 actual Develop UI groups, 16 full-camera local
workflow groups, four genuine four-local migration/admission/cancellation groups,
and two full-camera navigation/close groups. Both fallback depths are included.
The navigation test found screens at 1× and 2×, used the real 2× screen for native
crop/pan/radial gestures, and reports `actual_2x_hardware_qualified:true`; no
simulated result substitutes for that measurement. Global/local controls and
full-size native overlay captures were visually inspected.

| Gate | Result |
| --- | --- |
| Independent native coverage/blend/vignette tests | 13 pass; fixed one-code mask, `2e-6 + 0.5/65535` blend and three-code vignette bounds |
| Native resource/FFI/admission/refusal tests | All 11 pass in the final suite, including strict empty/invert fields and unknown-version cached preservation; the exact archive also passes the independent C ABI check |
| Standalone camera SDK versus complete product variants/calibration | 577,536,000 compared components; maximum **1** u16 code, fixed bound **3** |
| Eight complete native master payloads | **192,512,000 exact components**; original RAW, all native planes and every source ICC match captured inputs |
| Four full-size native mask oracles | Maximum errors `[0, 0, 0, 1]` codes; fixed bound **1** |
| Exact captured-input ordered blend | Maximum **0.000007749367**; fixed bound `2e-6 + 0.5/65535` |
| Independent source-ICC vignette | Maximum **1** u16 code; fixed bound **3**; no intermediate blend quantization |
| Outside-mask/alpha | **67,962** exact outside pixels per camera case; camera alpha remains exactly opaque |
| Original-derived highlight detail | **661,948** identically clipped base pixels in full radial coverage; recovered variants contain **601,441 sRGB / 601,054 P3** distinct RGB triples |
| Source/artifact provenance | 701 used input hashes/file sets and 350 compiled inputs exact; owned host process exited |
| Formatting / all-target Clippy | Pass; final Clippy log `/tmp/rz-phase8g-clippy-final.log` |
| Full Rust regression | **1,147 passed, zero failed, two existing ignored**, across 113 result groups; all 332 source/build inputs and the archive remain unchanged |
| Final Release / authoritative Swift typecheck | Pass, exit 0; all 563 app/core/build inputs remain unchanged |
| Final external MCP | **38 groups / 760 tool calls pass**, 19 groups per depth; 145,956,864 exact persisted source components, 728 input hashes/file sets exact, all eight owned processes exited |

The small native oracle separately corrects independently synthesized illumination
falloff. The full-camera blend/vignette oracle uses the exact captured product
variants, keeping the independent SDK's three-code allowance out of the tighter
kernel bound. The profile oracle reads actual source ICC transfer functions;
Apple's named-space reference ICC file need not have the same serialized bytes as
the equivalent app built-in profile. Native ICC bytes are checked exactly against
every captured product input.

The genuine four-local migration case admits a **775,248,375-byte** peak plan,
refuses denied admission without publication, cancels through the same parent
while native scratch is active, and verifies scratch release. It also checks
owner-close cancellation between camera stages, exact reviewed acceptance, all
four mask fingerprints and one Undo. The longest of the 14 standalone camera
reference renders was **3.790 seconds** on this host; this is an observation,
not a live-cancellation or arbitrary-camera latency guarantee.

Reproduction sources are in [`scripts/raw-local-probe`](../scripts/raw-local-probe/README.md)
and [`verify_phase8g.py`](../scripts/verify_phase8g.py). The exact CC0 camera
provenance is in [`camera-fixture.json`](../scripts/raw-probe/camera-fixture.json).
Standalone references and their receipt are `/tmp/rz-phase8g-reference-output`
and `/tmp/rz-phase8g-reference-provenance.json`; the earlier capability/effect
reconnaissance is `/tmp/rz-phase8g-capabilities.swift` / `.json`. Final Rust evidence is `/tmp/rz-phase8g-core-summary.json`,
`/tmp/rz-phase8g-core-provenance.json` and `/tmp/rz-phase8g-core-tests.log`. The independent
root audit `/tmp/rz-phase8g-root-host-audit.json` rechecks the final source/file
sets, all native ICCs and fixed camera gates. The qualified host executable SHA256
is `5e123da4f489846026a4316b0ce7feed0acdeacbefd21b3abd5287ee4219e88e`;
its target-15 core archive is
`35f1770304394b9ee735bc70ebd2ccfc271d12d9057bc4c329741b5701436d87`. The final Release executable SHA256 is
`883c78692f0dd465ef44911419b478dfed5ea7135a014a90b228e97a6a45f5a8`,
recorded in `/tmp/rz-phase8g-build-third-artifacts.json`. The combined final audit
`/tmp/rz-phase8g-final-audit.json` verifies current core/host/MCP/build input sets,
all source/reference/artifact hashes and numeric bounds;
`/tmp/rz-phase8g-root-cleanup-audit.json` confirms all nine final owned processes
are absent. Final external evidence is
`/tmp/rz-phase8g-mcp-final.json` and `/tmp/rz-phase8g-mcp-final3.log`; it includes
42,203 distinct native radial coverage levels and 31 strict refusal cases per depth.

## Qualified boundaries and later owners

- **Camera/lens matrix — 13D, with 11D/12F:** Nikon D70s decoder 8 and the oriented
  synthetic DNG decoder 8.dng are the declared corpus. Nikon baseline +0.5 EV
  changes 18,035,580 RGB components; moiré 0→1 changes 17,450,843, while moiré 0
  is exact. Vendor lens correction and local tone mapping are unavailable on this
  camera; unsupported controls are not exposed. More camera/decoder controls and
  lens/calibration datasets require separate evidence. Lens-database distortion,
  chromatic-aberration geometry and local-plane remapping have explicit 12F work;
  typed capture packets/PPI remain 11D and auxiliary mattes remain 9G.
- **Geometry/masks — 12F and 9F/9G/9H:** local planes live in oriented source pixel
  coordinates before layer affine. Same-geometry migration preserves them;
  changed extent/orientation uses 12F-4's reviewed source-coordinate map and
  independently bounded native u16 remap. Hard radial fields use point-center sampling,
  not area integration. These source-local tools do not close document selection,
  scalar-plane painting, canvas brush coverage or P7-07's backdrop/selection gate.
- **Scale/cancellation — 13C/13D:** at most four locals, five unique cached camera
  developments / 256 MiB, 64 MP per plane, 256 pending operations, 4,096 aggregate
  brush dabs, 256 million evaluated brush bounding-box pixels per native operation,
  and 256 KiB operation JSON. Original plus mask resources keep the
  256 MiB individual / 512 MiB native aggregate caps; working admission can refuse
  smaller cases. The 64 MP cap is not a four-local 64 MP qualification. Individual
  platform capture/render calls remain noncancellable; native stages and stage
  boundaries cooperate. Larger camera/source/history combinations and tighter
  codec cancellation require their own corpus and measured bounds.
- **Color/interchange — 11C–11F:** the source policy is normalized encoded SDR
  sRGB/P3. Wider calibrated workflows, linear/HDR/float policies and additional
  preserving metadata/codecs retain their existing owners. The failed expanded
  8F ProRes cross-decoder comparison remains explicitly unqualified in 11D/13D at
  its original threshold; 8G does not change that result.
- **Preset repair — 12B: DONE.** `list_raw_presets` reports each bad file as a
  `broken` row (`{file, code, reason, repairable}`) instead of refusing the
  library, `delete_raw_preset` takes a `file` as well as a `name`,
  `repair_raw_preset` rewrites a repairable file as canonical schema 2 naming
  every dropped control, and every preset is written at schema 2 while v1 is
  still read. Manual file removal is no longer the repair. Presets and global
  sync never transfer another camera's hidden recipe/decoder or a target's
  local coverage.

Six Phase 7 assignments remain verified and 30 remain open. Phase 8 is complete
within its declared qualified scope. Phase 9 is next and has not started.
