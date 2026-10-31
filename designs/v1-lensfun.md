# v1 Lensfun lens profiles (ADR)

- **Status:** Accepted — closed; shipped 2026-10-12. Filter ▸ Lens Correction applies a bundled Lensfun profile (Auto, None or a chosen lens); the Develop RAW window's Enable Profile Corrections applies one when the decoder carries no lens profile; MCP mirrors both (`apply_filter` lens_correction `profile`, `list_lens_profiles`).
- **Code:** `core/src/lens_model.rs` (the models, `LensProfile`), `core/src/lens_db.rs` (data, matching, interpolation), `core/src/sampling_lens.rs` (the stage), `core/src/ffi_lens_profile.rs` (`dr_lens_exif_identity`, `dr_lens_match`), `app/Sources/LensProfile.swift`, `LensCorrectionSheet.swift`, `AgentServer+LensProfiles.swift`, the RAW recipe files. Tests: `core/tests/filter_lens_profiles.rs`. Drive: `scripts/verify_v1_lensfun.py`.

## Licence and attribution

- Lensfun's library (`libs/`) is LGPL-3.0 and its database (`data/db/*.xml`) CC BY-SA 3.0 Unported (README and `docs/manual-main.txt`, "Software license", tag v0.3.4, commit `101c745e…`). We bundle only the database and link and copy no library code. The model formulas are written from `lensfun.h`'s model comments and the order of corrections from the manual's "How it is really done"; no documentation text is copied.
- Terms met: the licence URI ships with every copy (the JSON's `database` object, `Credits.html` in the About panel, `Acknowledgements.txt` in the bundle); the JSON stays plain text with no added terms; the conversion is an Adaptation and says it is CC BY-SA 3.0; the credit is "the Lensfun project and its contributors (original database: Tom Niemann's PTLens database)", the title "Lensfun lens database", its URI, and "converted to JSON by Doctored". Bundling in the app is a Collection, which does not bring the app under the licence.

## The data

- `core/data/lensfun/lensfun-db.json` (1.49 MB), written by `scripts/fetch_lensfun.py` from the pinned tarball (`v0.3.4`, sha256 `dafb39c0…b112`) and `include_bytes!`'d by `lens_db`, parsed once. The build stays offline; the script is the only network step and runs by hand.
- Compact form: mounts (name → compatible mounts), cameras (maker, model, variant, mount, crop) and lenses (id, maker, model, mounts, crop, aspect (default 1.5), centre, focal range (the calibration focals when the XML gives none) and calibration rows as number arrays per model: `poly3`, `poly5`, `ptlens`, `tca_linear`, `tca_poly3`, `pa`). Translations, comments, crop and real-focal-length rows are dropped. One entry a line, keys sorted, so a tag bump diffs entry by entry.
- A row outside the profile parser's bounds is dropped (33 at v0.3.4), and so is a lens left with no row (11). Lens ids are the model string, plus ` [crop c]` when a model repeats, then ` #n`; the script asserts they are unique.
- **Updating:** set the new tag, commit, URL and sha256 in `scripts/fetch_lensfun.py`, run it, review the data diff and the printed counts, and run `make t F=filter_lens_profiles` (every bundled lens resolves at each calibrated focal length). `python3 -I scripts/fetch_lensfun.py --check` re-fetches and compares the committed file byte for byte. Lensfun master's database version 2 is a different schema and is out of scope.

## The models (decisions in force)

- Lensfun stores maps from the corrected radius to the source radius, which is the direction the filter's inverse map runs: poly3 `r_d = r_u(1 − k1 + k1 r_u²)`, poly5 `r_u(1 + k1 r_u² + k2 r_u⁴)`, ptlens `r_u(a r_u³ + b r_u² + c r_u + 1 − a − b − c)`; TCA scales red and blue on the distorted radius (`r·k`, `r(b r² + c r + v)`).
- Normalisation (PanoTools/Hugin, whose coefficients Lensfun stores): `r_img = |q − c| / (S/2)`, S the plane's shorter side, `c = (W/2, H/2) + center·S/2`; calibration units `r = r_img · (Cc/Ci) · √(1+Ac²)/√(1+Ai²)` (Ci the image's crop, Cc the calibration's, Ac the lens aspect, Ai the plane's long/short), because a crop factor is 43.27 mm over the diagonal. `pa` reads `r/√(1+Ac²)`, so the calibration frame's corner is 1.
- Vignetting divides out the measured falloff: `stops = −log2(max(V, 1/16))`, at most four stops.
- A point where the distortion's slope `d(r·factor)/dr ≤ 0` is outside the picture (transparent), as the manual fold.
- Composition: per channel `source = T_c(D(manual_c(p)))` — the sliders first (output side), then the profile's distortion, then its TCA. A profile with TCA makes three passes. The profile's vignette is read at the full green source point and its stops are added to the slider's, one gain. A neutral profile (every model the identity) is the exact bypass, and without `profile` the filter is bit-identical to before.
- The gain's basis: a float layer multiplies linear RGB by `2^stops`; a 16-bit plane whose profile has vignetting decodes through the image profile's matrix/TRC model, scales, and encodes with `PrecisionToneEncoder` (its tables are in the plan); the 8-bit byte path keeps the sRGB-linear gain, the byte filter family's convention. **Limit:** an 8-bit Adobe RGB or ProPhoto document's vignetting is corrected in sRGB light.

## Matching

- Names are trimmed, spaces collapsed, lowercased, `N.0` written `N`, and split into Lensfun's documented words (runs of digits, punctuation, other characters); single punctuation characters and the word `f` are dropped; words compare as sets.
- Camera: case-insensitive exact maker and model, the first entry without a variant. Crop: the camera's, else `focal_length_35mm / focal_length`, else no profile ("crop factor unknown").
- A lens is a candidate when the EXIF lens words are a subset of its maker and model words, or its model words less its maker's are a subset of the EXIF words; the EXIF focal is within its range ± 0.5 mm; and, with a known camera, a mount is the camera's or one its mount lists as compatible (one level). A fixed-lens camera (lower-case mount) with exactly one lens on that mount matches with no lens name.
- Rank: Jaccard of the word sets, then `|ln(Cc/Ci)|`, then database order. Auto takes the first.
- Frame check (Auto only; a chosen lens is the user's call): with the target plane's size and the EXIF 0xA002/0xA003 both known, Auto gives no profile when the long/short aspects differ by more than 1%. A uniformly scaled frame passes. **Limit:** a crop whose writer rewrote 0xA002/0xA003 to the crop (as `glass.jpg`'s was) is not detectable and is normalised as a full frame.

## Interpolation (our stated rules)

- Distortion and TCA: linear in focal between the bracketing rows of one model, else the nearer row; the nearest end outside the range. A zoom with no EXIF focal has no profile; a prime uses its own focal.
- `pa`: at each bracketing focal, inverse-distance weighting (power 2) over `(1/N, 1/d)` with an exact hit taking its row, then linear in focal. No f-number: no vignetting (the answer's reason says so). No distance: 1000 m.

## Develop RAW

- Order: the decoder's own lens profile when `CIRAWFilter` has one (the maker's per-body data, inside the demosaic) wins; otherwise the Lensfun profile; never both.
- A new capture with no decoder profile resolves Auto from `CIRAWFilter.properties` (no plane size: a development is the camera frame) and keeps `lensProfile` = {id, lens, profile JSON} with Profile Corrections on and recipe version 5. A recipe without it encodes exactly as before (no DRDC bump); version 5 is accepted wherever 4 is, and an older build keeps the saved rendering.
- The stage is `lens_correction` with the saved profile through the selected edit (16-bit and float), right after the decoder render: on `RetainedRawDecoder.develop`'s development and on `RawLocalEvaluation`'s base and each camera variant, before the locals and the manual vignette. Canonical local coordinates, range reads and generated masks are therefore the corrected frame, as with a decoder profile. The camera-only variant strips it and its toggle; the decoder read-back keeps the Lensfun toggle; a Lensfun-corrected layer is never reused as the camera-only base; decoder migration carries the saved profile (a new decoder profile replaces it).
- The corrected frame of a pincushion lens has transparent edges, because auto-scale stays out of scope.
- Presets and Sync Settings never carry it. The manual pick for RAW is Filter ▸ Lens Correction.
- Admission: the stage needs no new charge. Check-only OL (open, 16/8) and VL (import preview) variants (`scripts/raw-decode-probe`, Release, 3 runs + twin, `MallocLargeCache=0`) hold the committed decode charge at ≥ 1.25× max E: camera.NEF OL16 1.318, OL8 1.304, VL16 1.260; D850 OL16/OL8 2.158, VL16 1.883 (`measure.py --camera camera.NEF --real D850=… --only OL16-NEF OL8-NEF VL16-NEF OL16-D850 OL8-D850 VL16-D850`).
- **Recording:** Lens Correction records `"auto"` or the lens id at every depth, which replay resolves against the target document. At 16/32 bits the frozen slider sheet runs its edit from that step, and `SelectedAdjustment.edit` resolves the string for the document (`RasterDocument.lensResolvedParameters`); `apply_filter` still takes a profile object as it is.
