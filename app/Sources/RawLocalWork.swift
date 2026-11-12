import AppKit
import CryptoKit

// Capture cumulative bounded operations against the original source; worker-safe request/evaluate
// uses one shared peak plan and sequential original-derived camera stages; local work
// (`RawLocalWork.request`: the Develop preview, `edit_raw_development`, enhanced `redevelop_raw`)
// takes `measuredScratch`, whose measurement record sits beside its constants, or the replace
// stage's forecast `replaceScratch` when that is larger, while decoder migration keeps the
// `scratch` envelope.

enum RawLocalWork {
    /// The media type a RAW local mask resource is written with
    /// (`raw_local_attachment.rs`' `RAW_LOCAL_MEDIA_TYPE`).
    static let maskMediaType = "com.kgalligan.doctored.raw-local-mask"
    /// The media type written before the Rasterize-to-Doctored rename; read, never written.
    static let legacyMaskMediaType = "com.rasterize.raw-local-mask"

    static let maximumOperations = 256
    static let maximumDabs = 4096
    static let maximumOperationBytes = 256 * 1024

    static func state(_ source: RasterDocument, layer: Int) throws -> RawLocalState {
        let payload = try PrecisionEditPayload.admitted(bytes: 2 * 1024 * 1024)
        defer { withExtendedLifetime(payload) {} }
        guard let info = try source.retainedSourceInfo(layer) else {
            throw DoctoredCoreError(message: "Choose a retained RAW source layer.")
        }
        let descriptor = try RawSourceDescriptor.read(info)
        let width = Int(try PrecisionDocumentDescriptor.integer(info, "width"))
        let height = Int(try PrecisionDocumentDescriptor.integer(info, "height"))
        let nodes = descriptor.recipe.enhancements?.locals ?? []
        var masks: [RasterRawLocalMask] = []
        if nodes.isEmpty {
            guard source.sourceAttachmentKind(layer) == .none else {
                throw DoctoredCoreError(message: "This RAW has an unknown or inconsistent attached resource policy. Keep its saved rendering.")
            }
        } else {
            guard source.sourceAttachmentKind(layer) == .rawLocalMasks,
                  let attachment = try source.sourceAttachmentInfo(layer),
                  let state = attachment["state"] as? [String: Any],
                  let resources = attachment["resources"] as? [[String: Any]],
                  Set(state.keys) == ["schema_version", "coordinates", "width", "height", "nodes"],
                  state["schema_version"] as? Int == 1,
                  state["coordinates"] as? String == "oriented-source-pixel-centers-v1",
                  state["width"] as? Int == width, state["height"] as? Int == height,
                  state["nodes"] as? [String] == nodes.map(\.id), resources.count == nodes.count else {
                throw DoctoredCoreError(message: "The saved RAW mask resources disagree with their recipe or source coordinates.")
            }
            for (index, node) in nodes.enumerated() {
                guard resources[index]["id"] as? String == node.id,
                      let media = resources[index]["media_type"] as? String,
                      media == maskMediaType || media == legacyMaskMediaType else {
                    throw DoctoredCoreError(message: "A saved RAW local mask reference is invalid.")
                }
                let mask = try RasterRawLocalMask.fromSource(source, layer: layer, resource: index)
                let metadata = try mask.info()
                guard metadata["width"] as? Int == width, metadata["height"] as? Int == height else {
                    throw DoctoredCoreError(message: "The saved local mask dimensions differ from the developed RAW source.")
                }
                let hash = try mask.sha256()
                guard resources[index]["sha256"] as? String == hash else {
                    throw DoctoredCoreError(message: "The retained RAW mask fingerprint does not match its saved resource. Its rendering remains intact.")
                }
                masks.append(mask)
            }
        }
        return RawLocalState(descriptor: descriptor, width: width, height: height, nodes: nodes, masks: masks)
    }

    /// One peak plan shared by ordinary local work and reviewed decoder migration.
    /// Decoder stages are sequential; retained variants use normal owner accounting.
    static func scratch(width: Int, height: Int, encoded: UInt64,
                        float: Bool = false) throws -> UInt64 {
        let decode = try RawSourceWork.scratch(width: width, height: height, encoded: encoded,
                                               float: float)
        // A float development's masters are 16 B/px where a 16-bit one's are 8.
        return try PrecisionDocumentDescriptor.sum(decode,
            UInt64(width) * UInt64(height) * (float ? 112 : 56) + 4 * 1024 * 1024)
    }

    /// Measurement record (refit on real camera files;
    /// `scripts/raw-decode-probe` `--phase2`). `restoreScratch` =
    /// `restoreFixed + restorePerPixel·px + 3·encoded` is Layer ▸ Redevelop
    /// RAW…'s restore admit (it decodes nothing; the window then opens).
    /// `measuredScratch` = max(the decode charge `RawSourceWork.scratch`,
    /// `localFixed + (localPerPixel + k·localPerLocalPixel +
    /// e·localPerEffectsPixel)·px + 3·encoded`) is local development's: the
    /// Develop preview (`RawLocalDevelopSession`, which adds its 64 MiB display
    /// term), MCP `edit_raw_development` and copy-local, and enhanced
    /// `redevelop_raw`, all through `request`, which charges
    /// max(`measuredScratch`, `replaceScratch`). k = the captured camera
    /// locals + creates + camera updates (≤ 4); e = authored lens geometry,
    /// grain or a vignette; the tone terms are below the constants.
    /// Decoder migration keeps `scratch`, the unmeasured envelope.
    ///
    /// Method: `RawSourceWork`'s: -O WMO harness over the app
    /// sources, release core, one case per process, `MallocLargeCache=0`,
    /// `DR_WORKING_LIMIT_BYTES` 32 GiB, 3 runs plus a baseline twin (every
    /// twin reads E 0), setup steps in autorelease pools, 1 s settle, E =
    /// `ri_interval_max_phys_footprint` − pre with the results alive;
    /// calibration 1 GiB drop returns 0 B, 256 MiB reads 268,566,720.
    /// Setup opens with `openRetainedRaw`, or (cold K cases) a saved `.rz`
    /// reopened with no decode before the measured stage. Settings are
    /// exposure 0.5 plus the neutral manual lens `pendingSettings` sends.
    /// RS: `restoreForRedevelop`, then `sourceMaster16` and the session, all
    /// alive (RS4 on a source carrying four locals). L: the session's preview
    /// request run to ready: L0 no locals; L1 one radial; L1B one empty local
    /// brushed with 2048 dabs on a 64×32 grid at the largest radius ≤ 180 the
    /// core admits (180 at D24 and D26, 179 at D850, 178 at A7R4: 253.6–256.0
    /// million evaluated px, cap 256,000,000); L1C exposure 0.2 on a source
    /// already carrying L1; L4 four radials; FX L1 + grain, vignette and
    /// authored lens. PB: `PreparedDocumentEdit` of the result (the publish
    /// stage).
    ///
    /// Fixtures: the synthetic DNGs D0, D3, D12, D24 (6000×4000,
    /// 48,001,004 B), D26 (6240×4160, 51,917,804 B), the Nikon D70s NEF
    /// (3008×2000, 5,193,213 B) and the seven raw.pixls.us
    /// camera files (px, bytes): Z6 24,337,152, 30,164,134; A73 24,000,000,
    /// 25,639,680; K1D 36,152,320, 46,069,312; A7R3 42,177,408, 43,002,368; R5
    /// 44,761,088, 73,484,232; D850 45,441,024, 55,196,715; A7R4 60,217,344,
    /// 61,983,744.
    ///
    /// Per case, max E over 3 runs (16-bit / 8-bit):
    ///   RS D0 589,824 / 589,824; D3 12,206,080 / 12,271,616; D12 48,136,216
    ///     / 48,136,216; D24 96,190,512 / 96,190,536; D26 104,005,704 /
    ///     104,038,472; NEF 10,387,456 / 10,534,912; Z6 60,538,928 /
    ///     60,506,160; A73 51,429,400 / 51,396,632; K1D 92,127,256 /
    ///     92,127,256; A7R3 86,147,096 / 86,147,096; R5 147,161,160 /
    ///     147,243,080; D850 110,559,328 / 110,592,048; A7R4 124,141,640 /
    ///     124,207,176
    ///   RS4 D3 12,304,384 / 12,255,232; D24 96,174,176 / 96,157,792; D26
    ///     104,038,496 / 104,022,112
    ///   RSK D0 14,483,504 / 18,923,664; D24 96,469,064 / 100,040,872; NEF
    ///     15,712,280 / 20,512,912; D850 110,903,368 / 115,130,536; A7R4
    ///     124,518,496 / 129,024,192
    ///   L0 D0 16,728,064 / 17,268,736; D3 234,340,424 / 222,380,080; D12
    ///     759,627,992 / 712,392,920; D24 1,377,173,960 / 1,286,816,152; D26
    ///     1,473,397,240 / 1,369,489,840; NEF 796,393,568 / 796,426,360; Z6
    ///     1,776,648,432 / 1,776,976,112; A73 1,599,897,864 / 1,599,537,416;
    ///     K1D 1,981,006,232 / 1,980,547,456; A7R3 2,112,913,744 /
    ///     2,113,585,488; R5 2,139,636,240 / 2,140,062,200; D850 2,047,214,048
    ///     / 2,046,935,496; A7R4 3,625,845,240 / 3,625,484,792
    ///   L1 D0 19,709,952 / 21,413,888; D3 288,522,360 / 288,473,184; D12
    ///     906,690,848 / 867,320,168; D24 1,618,281,168 / 1,527,792,192; D26
    ///     1,733,935,872 / 1,630,044,832; NEF 855,867,560 / 833,503,376; Z6
    ///     2,023,670,384 / 1,929,249,464; A73 1,840,726,448 / 1,745,715,824;
    ///     K1D 2,343,240,520 / 2,198,782,768; A7R3 2,539,848,448 /
    ///     2,370,044,576; R5 2,591,982,576 / 2,412,282,720; D850 2,506,048,400
    ///     / 2,324,775,728; A7R4 4,229,891,080 / 3,990,274,936
    ///   L0K D0 21,594,112 / 21,364,784; D24 1,379,205,744 / 1,380,270,728;
    ///     NEF 800,014,768 / 800,850,328; D850 2,050,786,192 / 2,051,982,152;
    ///     A7R4 3,630,875,824 / 3,631,826,072
    ///   L1K D0 24,166,424 / 24,330,240; D24 1,620,526,016 / 1,621,361,576;
    ///     NEF 859,062,656 / 859,947,296; D850 2,506,687,832 / 2,508,015,056;
    ///     A7R4 4,234,626,872 / 4,235,134,656
    ///   L1B D24 1,425,392,096 / 1,335,181,768; D26 1,525,596,712 /
    ///     1,421,689,288; D850 2,138,063,352 / 2,137,817,592; A7R4
    ///     3,746,628,184 / 3,745,923,696
    ///   L1C D24 1,377,616,376 / 1,467,400,720; D26 1,266,762,136 /
    ///     1,574,191,752; D850 2,065,711,728 / 2,062,402,160; A7R4
    ///     3,636,347,528 / 3,633,349,256
    ///   L4 D3 376,160,496 / 364,544,192; D24 2,341,701,096 / 2,173,846,872;
    ///     D26 2,515,584,608 / 2,411,202,072; NEF 1,034,617,288 /
    ///     1,016,349,080; Z6 2,770,634,168 / 2,676,360,512; A73 2,578,679,152
    ///     / 2,484,208,912; K1D 3,430,795,280 / 3,286,271,944; A7R3
    ///     3,823,552,648 / 3,654,682,616; R5 3,950,955,064 / 3,771,943,408;
    ///     D850 3,885,746,840 / 3,704,900,056; A7R4 refused by the core (below)
    ///   FX D3 275,611,864 / 263,766,208; D24 2,209,760,888 / 2,119,321,136;
    ///     D26 2,389,952,288 / 2,286,061,296; NEF 855,982,272 / 833,470,608;
    ///     Z6 2,222,196,464 / 2,128,103,152; A73 2,186,331,816 /
    ///     2,091,370,032; K1D 3,302,050,360 / 3,157,281,168; A7R3
    ///     3,843,197,760 / 3,674,884,832; R5 4,105,079,976 / 3,925,560,344;
    ///     D850 4,147,793,064 / 3,966,520,344; A7R4 5,486,088,152 /
    ///     5,245,718,344
    ///   LS (L1 with a Vision subject mask made from the base
    ///     development, `exposure_offset` 1) Z6 2,160,165,584 / 2,003,272,376
    ///   LP (the same with a people mask; Vision finds no person in
    ///     Z6, so the request ends after the Vision stage with no variant
    ///     and the session drops the create) Z6 1,776,713,968 / 1,776,697,584
    ///   PB (0 and 1 local alike) D24 576,307,608 / 480,248,144; D26
    ///     623,313,352 / 519,487,872
    /// L4 on the 61 MP A7R4 is not an admission case: the core kept a RAW's
    /// file and its local masks (2 B/px each) within one 512 MiB aggregate at
    /// any working limit, so four full-size masks stopped at about 59 MP (the
    /// aggregate is now 1 GiB: eight at that size). It is recorded as
    /// that ceiling and left out of the fit.
    ///
    /// Fit (deterministic; need = ⌈1.25 × max E⌉,
    /// rest = need − 3·enc, per-local and effects terms from cases ≥ 2 MP):
    /// (a, F) is the decode fit's own `fit()` over the k = 0 rows, so
    /// localPerPixel 73 is the 61 MP file's whole cost per pixel
    /// (L0K-8-A7R4) and localFixed 522 MiB the least fixed term beside it
    /// (L0K-8-NEF: the 6 MP NEF's preview peaks at 800 MB, which the old fit
    /// read as a 170 B/px slope and charged to every larger file);
    /// localPerLocalPixel 13 (L4-16-NEF), localPerEffectsPixel 16
    /// (FX-16-A7R4); restorePerPixel 2 (RSK8-NEF), restoreFixed 22 MiB
    /// (RSK8-D0). `measure.py --recheck-phase2` on the fit run charges all
    /// 172 fitted cases at ≥ 1.253× with no STOP: FX-16-A7R4 1.253 (margin
    /// 17,866,802), L1K-8-NEF 1.256 (5,378,191), L1K-16-NEF 1.258, L1-16-NEF
    /// 1.262, L4-16-NEF 1.271, FX-16-D850 1.289, RSK8-D0 1.302 (989,968).
    /// The decode max (518 MiB + 82 B/px) is above the k = 0 formula from
    /// 0.5 MP up, so it covers every k = 0 case but D0's and the local
    /// formula binds only with a local or an effect.
    /// Charges: restore D24 215,071,684, D850 279,540,865; local D24 k0
    /// 2,655,165,380, k1 2,755,359,684, k4 3,691,359,684 (4,224,003,012 /
    /// 4,488,003,012 / 5,280,003,012 before the real-file refit); D850 k0
    /// 4,434,916,481, k1 4,620,874,881, k4 6,393,074,817, FX 5,347,931,265
    /// (k4 was 9,889,969,281); A7R4 k0 5,666,935,808, k1 5,911,999,488, FX
    /// 6,875,476,992; NEF k0 1,052,054,007, k1 1,080,312,311, k4
    /// 1,314,936,311 (`scratch`, decoder migration's envelope, is
    /// 4,003,359,684 at D24, 6,983,808,129 at D850 and 1,393,144,311 for the
    /// NEF at every k). The publish projection charge,
    /// `projectionScratchEstimate()` with `ProjectionMargin`, is
    /// 1.380× its measured peak at 16 bits and 1.724× (m8 = 7) at 8.
    /// The migration envelope (`scratch` + 8 B/px) covers every L4 case at ≥
    /// 1.39×. Under the committed constants the L4 and FX cases of the NEF,
    /// D850 and A7R4 ran again (`--phase2 --only`): no stage refused inside
    /// the job, each max E within 0.4 % of the fit run's and under its charge.
    /// The generated-mask cases ran under the same constants, none
    /// moved (`--phase2 --real Z6=… --only LS-16-Z6 LS-8-Z6 LP-16-Z6
    /// LP-8-Z6`, k 1, charge 2,730,844,146, every twin 0, no STOP): the
    /// Vision stage sits inside the local charge at LS-16-Z6 1.264 (margin
    /// 30,637,166), LS-8-Z6 1.363, LP-16-Z6 1.537 and LP-8-Z6 1.537. Every
    /// LS run made a mask (`operations_after` 1); every LP run found nothing
    /// and left 0 operations, the session's first-run drop. 24 MP only; no
    /// larger frame was measured for the Vision stage. The same command
    /// with no build process running read LS-16-Z6 1.301 (margin
    /// 107,826,226) with the other three unchanged, so no constant moved.
    /// Develop Presence counts as an effect (`chargeShape`). PX (FX
    /// plus texture, clarity and dehaze 50; check-only, not fitted;
    /// `--phase2 --real D850=… --only PX-16-D850 PX-8-D850`, k 1 with
    /// effects, charge 5,347,931,265, twins 0, no STOP): max E PX-16-D850
    /// 4,148,251,840 (1.289, margin 162,616,465), PX-8-D850 3,967,290,392
    /// (1.348), within 0.1 % of FX's, so no constant moved. The decode
    /// term is now 528 MiB (`RawSourceWork`'s record), so the k = 0 charges
    /// and `scratch` below are 10,485,760 higher: D24 k0 2,665,651,140, D850
    /// k0 4,445,402,241, A7R4 k0 5,677,421,568, NEF k0 1,062,539,767;
    /// `scratch` 4,013,845,444 / 6,994,293,889 / 1,403,630,071; and the k = 0
    /// ceilings below read 72.7 (8-bit DNG), 71.6 and 74.3 (D850) MP.
    ///
    /// Menu (`--menu`, every stage admitted in order: restore, session,
    /// each preview, publish): at 8 GiB M0, M1, M11 and MC pass at 16 and 8
    /// bits for D24, D26 and all seven camera files, the 45 MP R5 and D850
    /// and the 61 MP A7R4 among them (D850's preview charge 4,687,983,745
    /// beside 2,240,753,748 retained at M11's second preview; A7R4's
    /// 5,979,108,352 beside 1,970,567,060, MC-16). Driven through MCP on the
    /// Debug app at 8 GiB, the D850 opened at 16 bits takes
    /// `edit_raw_development` with one radial create and with four, and a RAW
    /// layer scaled 150 % in a 32-bit float document takes one under the
    /// replace forecast.
    ///
    /// Ceilings (the preview charge + 64 MiB beside the opened document,
    /// M0-D24's 25.6 B/px retained at 16 bits, 21.6 at 8): at 8 GiB about
    /// 70.2 / 67.8 / 50.9 MP of uncompressed 16-bit DNG for 0 / 1 / 4 locals
    /// (8-bit 72.8 / 70.2 / 52.3); at the D850's 1.215 file bytes per pixel
    /// 71.7 / 69.2 / 51.7 (8-bit 74.4 / 71.7 / 53.1). So a 45 MP RAW fits with
    /// four locals; past that the core's own caps answer first (a local mask
    /// to 64 MP; the masks' resource cap, `maximumLocals`). The model
    /// mirrors `measuredScratch` only. The restore admit is never the ceiling
    /// (≥ 254 MP at 8 GiB).
    static let restoreFixed: UInt64 = 22 * 1024 * 1024
    static let restorePerPixel: UInt64 = 2
    static let localFixed: UInt64 = 522 * 1024 * 1024
    static let localPerPixel: UInt64 = 73
    static let localPerLocalPixel: UInt64 = 13
    static let localPerEffectsPixel: UInt64 = 16
    /// The local tone path. `localPerToneLocalPixel` is charged for every local (k_t =
    /// the captured locals + creates, ≤ 16), camera ones included; `localToneStagePixel` once
    /// when any local's tone reads a neighbourhood (`RawLocalTone.readsPlane`: Highlights,
    /// Shadows, Clarity, Texture, Sharpness or Noise Reduction; the core builds one guide, one
    /// Presence plane and one detail plane at a time). k, the camera count, takes
    /// every create and every update carrying a camera control (`chargeShape`).
    ///
    /// Record (`measure.py --phase2 --only LT…`, the method above; 3 runs + a twin per
    /// case, every twin E 0, no STOP): LT1 one pointwise tone radial (exposure 0.5, contrast 40,
    /// whites 20, blacks −20, saturation 20), LT16 sixteen of them on a 4×4 grid, LTS one
    /// radial with highlights, shadows, clarity and texture 50, at D24 and Z6, 16 / 8 bits,
    /// max E: LT1 D24 1,425,244,664 / 1,270,186,344, Z6 1,825,095,968 / 1,825,440,056; LT16
    /// D24 2,146,206,752 / 2,055,046,128, Z6 2,556,052,272 / 2,556,085,040; LTS D24
    /// 1,427,309,048 / 1,335,132,592, Z6 1,825,128,760 / 1,825,276,168. LTS reads LT1's E: the
    /// guide and the Presence plane peak below the decode stage's peak, so the stage adds
    /// nothing at 24 MP; LT16 adds the sixteen masks (about 32 B/px), which the camera term
    /// already charges for its creates (k = 4: 52 B/px). Fit (`fit_tone`, over the committed
    /// F, a and b): localPerToneLocalPixel 0 (LT16-8-Z6 binds), localToneStagePixel 0
    /// (LTS-8-Z6); every case is charged at ≥ 1.440× (LT16-16-Z6 1.440, margin 484,927,734;
    /// LT1/LTS-Z6 1.496; D24 ≥ 1.720). At 8 GiB beside the opened document LT16-Z6 asks
    /// 3,679,993,074 + 64 MiB + 360,045,420 retained (8-bit) and LTS-Z6 2,730,844,146 + 64 MiB
    /// + 360,045,420. 24 MP only; no larger frame was measured for the tone stage. Locals per
    /// frame: the core's 1 GiB resource cap holds 16 masks (16 + 2·px bytes each) beside the
    /// file up to about 32 MP and fewer above (D850 11, A7R4 8, K1D 14 with their files);
    /// `maximumLocals` refuses the excess in `request` before any admission.
    ///
    /// LTD (check-only, not fitted: LTS's tone plus Sharpness 60 and Noise
    /// Reduction 60, the detail stage over the Presence plane; `--phase2 --real Z6=… --only
    /// LTD-16-D24 LTD-8-D24 LTD-16-Z6 LTD-8-Z6`, 3 runs + a twin, every twin E 0, no STOP): max E
    /// D24 1,826,620,736 / 1,736,361,208, Z6 1,834,190,144 / 1,825,390,928 (16 / 8 bits); the
    /// committed charge covers each at ≥ 1.489× (LTD-16-Z6, margin 438,106,466; D24 ≥ 1.508), so
    /// no constant moved. At 8 GiB LTD-Z6 asks 2,730,844,146 + 64 MiB + 360,045,484 (8-bit).
    static let localPerToneLocalPixel: UInt64 = 0
    static let localToneStagePixel: UInt64 = 0

    /// The restore admit of Layer ▸ Redevelop RAW…: `restoreFixed +
    /// restorePerPixel·px + 3·encoded`. The restore decodes nothing.
    static func restoreScratch(width: Int, height: Int, encoded: UInt64) throws -> UInt64 {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "The RAW dimensions exceed the supported canvas size.")
        }
        return try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(restoreFixed, UInt64(width * height) * restorePerPixel),
            encoded * RawSourceWork.scratchPerEncodedByte)
    }

    /// Local development's measured charge: the decode charge, or `localFixed
    /// + (localPerPixel + locals·localPerLocalPixel + toneLocals·localPerToneLocalPixel
    /// + toneStage·localToneStagePixel + effects·localPerEffectsPixel)·px + 3·encoded`, whichever
    /// is larger, so `develop`'s own guard always holds. `locals` counts the camera locals (0…4),
    /// `toneLocals` every local (0…16).
    ///
    /// `float` is the 32-bit float (linear) development, the formula
    /// fixed before it was measured: the 16-bit charge + (8·(locals + 2) + 8 +
    /// (effects ? 8 : 0))·px. The base, the mixed result and each variant are
    /// f32 masters, 8 B/px more than their 16-bit twins; the render buffer is
    /// 8 more; an effect's working plane 8 more. It is never below the float
    /// decode charge (the 16-bit charge + 16·px), so `develop`'s guard holds.
    /// A tone local refuses at the float depth (`RawLocalTone.floatRefusal`),
    /// so the tone terms add nothing there beyond the 16-bit charge.
    /// Measurement record (`measure.py --float`, the decode record's method):
    /// charge (+ the session's 24 B/px of inspection copies) ÷ max E for L0,
    /// L1, L4 and FX (no grain) is 1.502/1.415/1.351/1.560 on the NEF,
    /// 2.707/1.716/1.543/1.471 on D24 and 2.582/2.184/1.740/1.381 on D850;
    /// FX-32 with grain (#4) 1.558/1.471/1.574 on the NEF, D24 and D850;
    /// every case ≥ 1.25, so `localPerPixel` was not raised. At 8 GiB, beside
    /// the opened float document (18.1 B/px retained), a Develop session fits
    /// about 53 MP with no local, 49 MP with one and 35.5 MP with four, so a
    /// 45 MP float development with four locals is refused there.
    static func measuredScratch(width: Int, height: Int, encoded: UInt64, locals: Int,
                                toneLocals: Int = 0, toneStage: Bool = false,
                                effects: Bool, float: Bool = false) throws -> UInt64 {
        guard (0...RawEnhancementRecipe.maximumCameraLocals).contains(locals),
              (0...RawEnhancementRecipe.maximumLocals).contains(toneLocals) else {
            throw DoctoredCoreError(message: "RAW local work supports at most sixteen locals, four of them camera locals.")
        }
        let decode = try RawSourceWork.scratch(width: width, height: height, encoded: encoded)
        let perPixel = localPerPixel + UInt64(locals) * localPerLocalPixel
            + UInt64(toneLocals) * localPerToneLocalPixel + (toneStage ? localToneStagePixel : 0)
            + (effects ? localPerEffectsPixel : 0)
        let local = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(localFixed, UInt64(width * height) * perPixel),
            encoded * RawSourceWork.scratchPerEncodedByte)
        guard float else { return max(decode, local) }
        let extra = UInt64(8 * (locals + 2) + 8 + (effects ? 8 : 0))
        return try PrecisionDocumentDescriptor.sum(max(decode, local), UInt64(width * height) * extra)
    }

    /// The replace stage's forecast: `scratch_bytes` of the source plan
    /// for `RawLocalEvaluation.replacing`'s request (replace, convert_fallback,
    /// the layer's link) with `source.rawLocalBaseSource(layer:)` standing in
    /// for the developed asset, + 32 MiB (the profile and attachment envelope
    /// of a development that chooses a different ICC), + 8·(locals + 2)·width·height
    /// (the base, the mixed result and one variant per local, each a 16-bit
    /// master the job has reserved by then; 16·(locals + 2) when `float`, the
    /// desired recipe's 32-bit float (linear) masters). `request` charges at
    /// least this, because its budget is the job's scratch limit and the
    /// core's replace reserves its exact plan against it: a charge below the
    /// plan would refuse inside the job.
    ///
    /// The stand-in is the layer's present master, at its own depth and size.
    /// In an 8- or 16-bit document that is the developed asset's depth too (a
    /// RAW layer there always keeps a 16-bit master). An encoding switch
    /// (`sdr16` ↔ `linear_float`) makes the stand-in the OTHER depth,
    /// and it happens only in a 32-bit float document, where the core's plan
    /// charges a master at max(master, fallback) depth with no promote,
    /// quantize or float-low term (`retained_source_render.rs`,
    /// `work_plan_within`): `scratch_bytes` is the same for a u16 and an f32
    /// asset of one size and profile, which
    /// `float_raw_source::a_float_documents_replace_plan_is_the_same_for_a_u16_and_an_f32_master`
    /// pins.
    static func replaceScratch(source: RasterDocument, layer: Int, width: Int, height: Int,
                               locals: Int, float: Bool = false) throws -> UInt64 {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              (0...4).contains(locals) else {
            throw DoctoredCoreError(message: "The RAW dimensions exceed the supported canvas size.")
        }
        let link = try source.retainedSourceInfo(layer)?["link"] as? [String: Any]
        let command = try PrecisionPixelCommand(["op": "replace", "layer": layer,
            "revision_id": UUID().uuidString, "depth_conversion": "convert_fallback",
            "link": link.map { $0 as Any } ?? NSNull()])
        let asset = try source.rawLocalBaseSource(layer: layer)
        let plan = try source.retainedSourcePlan(asset: asset, request: command.request)
        return try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(
                PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"), 32 * 1024 * 1024),
            UInt64(width * height) * (float ? 16 : 8) * UInt64(locals + 2))
    }

    /// The charge's shape for one request, counting over the operations, never under. Creates
    /// raise both counts; an update carrying a camera control (`exposure_offset`, `white_balance`,
    /// `temperature`, `tint`) may turn a local into a camera local, so it raises the camera
    /// count; remove and reorder never lower either. A captured local below recipe version 3 is
    /// always a camera local. `toneStage`: some captured or requested tone reads a neighbourhood.
    static func chargeShape(captured: RawLocalState, desired: RawDevelopmentRecipe,
                            operations: [[String: Any]])
        -> (locals: Int, toneLocals: Int, toneStage: Bool, effects: Bool) {
        let creates = operations.filter { $0["op"] as? String == "create" }.count
        let cameraKeys: Set<String> = ["exposure_offset", "white_balance", "temperature", "tint"]
        let cameraUpdates = operations.filter {
            $0["op"] as? String == "update" && !cameraKeys.isDisjoint(with: $0.keys)
        }.count
        let version = captured.descriptor.recipe.enhancements?.version ?? 1
        let capturedCameras = captured.nodes.filter { $0.developsCameraVariant(version: version) }.count
        let requested = operations.contains { operation in
            guard let object = operation["tone"] as? [String: Any],
                  let tone = try? RawLocalTone(json: object) else { return false }
            return tone.readsPlane
        }
        let toneStage = requested || captured.nodes.contains { $0.tone?.readsPlane ?? false }
        let effects = desired.sourceGeometry != nil || desired.enhancements?.grain != nil
            || desired.enhancements?.presence != nil || desired.lensfunApplies
            || (desired.enhancements?.vignette.edgeExposure ?? 0) != 0
        return (min(RawEnhancementRecipe.maximumCameraLocals, capturedCameras + creates + cameraUpdates),
                min(RawEnhancementRecipe.maximumLocals, captured.nodes.count + creates), toneStage, effects)
    }

    /// The tone path's share of a development the measured charge does not cover (decoder
    /// migration's `scratch` envelope): `localToneStagePixel·px` when some tone reads a
    /// neighbourhood, plus `localPerToneLocalPixel·px` per local with a tone.
    static func toneScratch(width: Int, height: Int, nodes: [RawLocalNode]) throws -> UInt64 {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "The RAW dimensions exceed the supported canvas size.")
        }
        let toned = nodes.compactMap(\.tone)
        guard !toned.isEmpty else { return 0 }
        let stage = toned.contains(where: \.readsPlane)
        let perPixel = UInt64(toned.count) * localPerToneLocalPixel + (stage ? localToneStagePixel : 0)
        return UInt64(width * height) * perPixel
    }

    /// How many locals fit beside the RAW under the core's 1 GiB combined resource cap: a mask
    /// is 16 + 2·px bytes, so `min(16, (2³⁰ − encoded) / (16 + 2·px))`, 0 when the file alone is
    /// over it. Mirrors `raw_local_limits::fitting_locals` in the core (the master copy).
    static func maximumLocals(width: Int, height: Int, encoded: UInt64) -> Int {
        let cap: UInt64 = 1 << 30
        guard width > 0, height > 0, encoded <= cap else { return 0 }
        let mask = UInt64(width) * UInt64(height) * 2 + 16
        return Int(min(UInt64(RawEnhancementRecipe.maximumLocals), (cap - encoded) / mask))
    }

    /// Main thread. Restores the retained camera file for the Develop window:
    /// the restore admit, the private snapshot, then the compatibility check.
    static func restoreForRedevelop(source: RasterDocument, layer: Int) throws
        -> (descriptor: RawSourceDescriptor, snapshot: RawSourceSnapshot, probe: RawProbe) {
        assert(Thread.isMainThread)
        let info = try LinkedImageWork.requireSource(source, layer: layer, editing: true)
        let descriptor = try RawSourceDescriptor.read(info)
        guard descriptor.compatible else {
            throw DoctoredCoreError(message: "The captured RAW decoder implementation is unavailable. Use Layer > Migrate RAW Decoder to compare the current decoder with the saved rendering.")
        }
        let encoded = try PrecisionDocumentDescriptor.integer(info["raw"] as? [String: Any] ?? [:], "encoded_bytes")
        let budget = try restoreScratch(width: Int(PrecisionDocumentDescriptor.integer(info, "width")),
            height: Int(PrecisionDocumentDescriptor.integer(info, "height")), encoded: encoded)
        _ = try RasterResources.shared.admit(scratch: budget)
        let control = try RasterJobControl()
        let snapshot = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            try RawSourceSnapshot.restore(source, layer: layer, descriptor: descriptor,
                                          control: control)
        }, title: "Opening Retained RAW", control: control)
        // Compatibility is checked before opening the controls. Their
        // preview and commit use the same private source and recipe.
        _ = try RetainedRawDecoder.filter(url: snapshot.url, descriptor: descriptor)
        guard let probe = RawImage.inspect(url: snapshot.url) else {
            throw DoctoredCoreError(message: "The captured RAW source cannot be decoded. Its saved rendering is intact.")
        }
        return (descriptor, snapshot, probe)
    }

    /// `provided`: the masks `coverage` creates name (Synchronize RAW Settings' Masking).
    static func request(source: RasterDocument, layer: Int,
                        settings: RawDevelopSettings = .init(), operations: [[String: Any]] = [],
                        provided: [RasterRawLocalMask] = [],
                        cache: RawLocalVariantCache? = nil) throws -> RawLocalRenderRequest {
        _ = try LinkedImageWork.requireSource(source, layer: layer, editing: true)
        let payload = try PrecisionEditPayload.admitted(bytes: 4 * 1024 * 1024)
        let captured = try state(source, layer: layer)
        let operations = try RawLocalOperations.capture(operations, provided: provided.count)
        try RawSourceWork.requireFloatDocument(settings, in: source)
        let desired = try captured.descriptor.recipe.updating(settings)
        try RawSourceWork.requireFloatDocument(desired, in: source)
        // The masters the job reserves are the REQUESTED encoding's, whatever
        // depth the layer's present master is.
        let float = desired.isLinearFloat
        // A requested local tone refuses at the float depth, before any admission.
        if float { try RawLocalTone.refuseAtFloat(nodes: captured.nodes, operations: operations) }
        let info = try source.retainedSourceInfo(layer) ?? [:]
        let encoded = try PrecisionDocumentDescriptor.integer(info["raw"] as? [String: Any] ?? [:], "encoded_bytes")
        // Locals past what the resource cap holds refuse before any admission or decode. A
        // remove frees its slot, so a replace (remove each, then create) is not over-counted.
        let creates = operations.filter { $0["op"] as? String == "create" }.count
        let removes = operations.filter { $0["op"] as? String == "remove" }.count
        let resulting = max(0, captured.nodes.count - removes) + creates
        let fitting = maximumLocals(width: captured.width, height: captured.height, encoded: encoded)
        if resulting > RawEnhancementRecipe.maximumLocals {
            throw DoctoredCoreError(message: "A RAW layer supports at most sixteen locals.")
        }
        if resulting > fitting {
            throw DoctoredCoreError(message: "This RAW's local masks would exceed the 1 GiB source resource limit: at most \(fitting) locals at this size.")
        }
        let shape = chargeShape(captured: captured, desired: desired, operations: operations)
        // The measured charge, or the replace stage's forecast when that is larger.
        let budget = try max(
            measuredScratch(width: captured.width, height: captured.height, encoded: encoded,
                locals: shape.locals, toneLocals: shape.toneLocals, toneStage: shape.toneStage,
                effects: shape.effects, float: float),
            replaceScratch(source: source, layer: layer, width: captured.width,
                height: captured.height, locals: shape.locals, float: float))
        let cache = cache ?? .shared
        do { _ = try RasterResources.shared.admit(scratch: budget) }
        catch { cache.clear(); _ = try RasterResources.shared.admit(scratch: budget) }
        return RawLocalRenderRequest(source: source, layer: layer, captured: captured,
            desired: desired, operations: operations, provided: provided, scratchEstimate: budget,
            payload: payload, cache: cache)
    }

    static func prepare(source: RasterDocument, layer: Int,
                        settings: RawDevelopSettings = .init(), operations: [[String: Any]] = [],
                        provided: [RasterRawLocalMask] = []) throws -> RasterDocument? {
        let request = try request(source: source, layer: layer, settings: settings, operations: operations,
                                  provided: provided)
        let control = try RasterJobControl()
        guard control.setScratchLimit(request.scratchEstimate) else {
            throw DoctoredCoreError(message: "Could not configure the RAW local work budget.")
        }
        return try BackgroundWork.wait(try WorkerTask(scratchEstimate: request.scratchEstimate) {
            try request.evaluate(control: control)
        }, title: "Developing Local RAW", control: control)
    }

    static func publish(prepared: RasterDocument, in document: ImageDocument, capture: ImageSourceCapture,
                        layer: Int, settings: RawDevelopSettings, operations: [[String: Any]]) throws {
        try capture.requireCurrent(document)
        guard prepared !== capture.source else { return }
        let operations = try RawLocalOperations.capture(operations)
        let before = try state(capture.source, layer: layer)
        let arguments: [String: Any] = ["layer": layer, "raw": settings.jsonObject(), "operations": operations,
            "before_recipe": try before.descriptor.json(), "before_masks": try maskFingerprints(capture.source, layer: layer)]
        try RawSourceWork.publish(prepared, in: document, capture: capture,
            title: "Develop RAW", record: .step("edit_raw_development", arguments, note: "Layer ▸ Redevelop RAW"))
    }

    static func maskFingerprints(_ source: RasterDocument, layer: Int) throws -> [String] {
        guard source.sourceAttachmentKind(layer) == .rawLocalMasks else { return [] }
        guard let attachment = try source.sourceAttachmentInfo(layer),
              let resources = attachment["resources"] as? [[String: Any]] else {
            throw DoctoredCoreError(message: "The RAW local resource manifest is unavailable.")
        }
        return try resources.map {
            guard let value = $0["sha256"] as? String else { throw DoctoredCoreError(message: "A local mask fingerprint is missing.") }
            return value
        }
    }

    static func attachment(nodes: [RawLocalNode], masks: [RasterRawLocalMask], width: Int, height: Int,
                           control: RasterJobControl) throws -> String? {
        guard !nodes.isEmpty else { return nil }
        var resources: [[String: Any]] = []
        for (node, mask) in zip(nodes, masks) {
            guard !control.isCancelled else { throw JobCancelled() }
            let metadata = try mask.info()
            let sha = try mask.sha256()
            let length = try PrecisionDocumentDescriptor.integer(metadata, "encoded_bytes")
            resources.append(["id": node.id, "media_type": maskMediaType,
                "sha256": sha, "byte_length": length])
        }
        return try RawLocalJSON.string(["schema_version": 1, "kind": "raw_local_masks",
            "state": ["schema_version": 1, "coordinates": "oriented-source-pixel-centers-v1",
                "width": width, "height": height, "nodes": nodes.map(\.id)], "resources": resources])
    }
}

struct RawLocalRenderRequest {
    let source: RasterDocument
    let layer: Int
    let captured: RawLocalState
    let desired: RawDevelopmentRecipe
    let operations: [[String: Any]]
    let provided: [RasterRawLocalMask]
    let scratchEstimate: UInt64
    let payload: PrecisionEditPayload
    let cache: RawLocalVariantCache

    func evaluate(control supplied: RasterJobControl? = nil) throws -> RasterDocument? {
        let preview = try evaluatePreview(control: supplied ?? RasterJobControl())
        return preview.document === source ? nil : preview.document
    }

    /// `base` is the base development the stages mixed from (the camera-only render, Lensfun-
    /// corrected when the profile applies), which a local's range reads; nil when nothing was
    /// developed. `needsBase` (the Develop overlay is on) makes
    /// an unchanged request develop anyway so the base is there to read, but only when the
    /// applied nodes hold a range: a request that nets to no range (New Range, then Remove) has
    /// nothing to draw and stays the unchanged source, so Apply publishes nothing for it.
    func evaluatePreview(control supplied: RasterJobControl, needsBase: Bool = false) throws ->
        (document: RasterDocument, editingSource: RasterDocument, base: RasterDocument?) {
        defer { withExtendedLifetime(payload) {} }
        let control = supplied
        guard !control.isCancelled else { throw JobCancelled() }
        // A subject or people mask is made from this request's base development, which does not
        // depend on the locals: the memo answers when it holds that kind for that base;
        // otherwise the snapshot is restored and the base developed (each at most once here,
        // then handed to `developStages`) and Vision runs.
        var global = captured.descriptor
        global.recipe = desired
        var restored: RawSourceSnapshot?
        var base: (RasterDocument, RawSourceDescriptor)?
        let (nodes, masks) = try RawLocalOperations.applying(operations, state: captured,
            control: control, provided: provided) { kind in
            let baseKey = try RawLocalEvaluation.baseKey(global)
            if let known = cache.generated(kind, baseKey: baseKey) { return known }
            guard captured.descriptor.compatible else {
                throw DoctoredCoreError(message: "The captured camera decoder is unavailable. The saved RAW rendering remains intact.")
            }
            let ticket = cache.ticket()
            let snapshot = try restored
                ?? RawSourceSnapshot.restore(source, layer: layer, descriptor: captured.descriptor)
            restored = snapshot
            let developed = try base ?? RawLocalEvaluation.baseDevelopment(snapshot: snapshot,
                requested: global, previous: captured.descriptor, previousSource: source,
                layer: layer, cache: cache, maximumScratch: scratchEstimate, control: control)
            base = developed
            let mask = try RawLocalGenerated.mask(kind, base: developed.0, control: control)
            cache.storeGenerated(mask, kind: kind, baseKey: baseKey, ticket: ticket)
            return mask
        }
        if desired == captured.descriptor.recipe, nodes == captured.nodes,
           zip(masks, captured.masks).allSatisfy({ $0.0 === $0.1 }),
           desired.sourceGeometry == nil, desired.enhancements?.grain == nil,
           desired.enhancements?.presence == nil,
           !(needsBase && nodes.contains(where: { $0.range != nil }))
               || !captured.descriptor.compatible { return (source, source, nil) }
        guard captured.descriptor.compatible else {
            throw DoctoredCoreError(message: "The captured camera decoder is unavailable. The saved RAW rendering remains intact.")
        }
        var requested = captured.descriptor
        requested.recipe = desired
        requested.recipe.enableEnhancements()
        requested.recipe.enhancements?.locals = nodes
        let snapshot = try restored
            ?? RawSourceSnapshot.restore(source, layer: layer, descriptor: captured.descriptor)
        let developed = try RawLocalEvaluation.developStages(snapshot: snapshot, requested: requested,
            previous: captured.descriptor, previousSource: source, layer: layer, masks: masks,
            cache: cache, maximumScratch: scratchEstimate, control: control, base: base)
        guard developed.final.width == captured.width, developed.final.height == captured.height else {
            throw DoctoredCoreError(message: "RAW development changed source geometry. Local mask remapping is unavailable; the saved result is intact.")
        }
        if developed.descriptor == captured.descriptor, masks.count == captured.masks.count,
           zip(masks, captured.masks).allSatisfy({ $0.0 === $0.1 }) {
            return (source, developed.editing, developed.base)
        }
        let replacement = try RawLocalEvaluation.replacing(source: source, layer: layer,
            asset: developed.final, descriptor: developed.descriptor, masks: masks, control: control)
        return (replacement, developed.editing, developed.base)
    }
}
