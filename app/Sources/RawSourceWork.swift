import AppKit
import CryptoKit

// Private snapshots pin decoded bytes; actual output dimensions are admitted before buffers,
// against `RawSourceWork.scratch`, the measured decode charge whose record sits beside its
// constants (Layer ▸ Redevelop RAW…'s restore admit takes `RawLocalWork.restoreScratch`
// instead), and `develop` renders through its own `CIContext`
// (`RetainedRawDecoder.contextOptions`), cleared and dropped after each render,
// because clearing the shared `context`'s caches left Core Image memory resident.

/// A private immutable file supplies both decoder bytes and retained bytes.
/// Keeping the owner in preview closures keeps its file alive through cancellation.
final class RawSourceSnapshot {
    let url: URL
    let bytes: Data
    let sha256: String
    let payload: PrecisionEditPayload
    private let directory: URL

    private init(directory: URL, url: URL, bytes: Data, sha256: String) throws {
        self.directory = directory
        self.url = url
        self.bytes = bytes
        self.sha256 = sha256
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(bytes.count) + 4096)
    }
    deinit { try? FileManager.default.removeItem(at: directory) }

    static func size(_ url: URL) throws -> UInt64 {
        let info = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard info.isRegularFile == true, let count = info.fileSize, count > 0,
              count <= 256 * 1024 * 1024 else {
            throw DoctoredCoreError(message: "Choose a regular camera RAW file of at most 256 MiB.")
        }
        return UInt64(count)
    }

    static func capture(_ source: URL, maximumBytes: UInt64) throws -> RawSourceSnapshot {
        let directory = try folder()
        do {
            let url = directory.appendingPathComponent(source.lastPathComponent)
            let hash = try ImageSourceFile.fingerprint(source, copyingTo: url, maximumBytes: maximumBytes)
            return try RawSourceSnapshot(directory: directory, url: url,
                bytes: Data(contentsOf: url, options: .uncached), sha256: hash)
        } catch { try? FileManager.default.removeItem(at: directory); throw error }
    }

    static func restore(_ source: RasterDocument, layer: Int,
                        descriptor: RawSourceDescriptor) throws -> RawSourceSnapshot {
        let directory = try folder()
        do {
            let bytes = try source.rawSourceBytes(layer)
            let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
            guard hash == descriptor.content_sha256 else {
                throw DoctoredCoreError(message: "The retained RAW fingerprint does not match its saved descriptor. The saved rendering remains intact.")
            }
            let url = directory.appendingPathComponent(descriptor.original_filename)
            guard FileManager.default.createFile(atPath: url.path, contents: bytes,
                                                attributes: [.posixPermissions: 0o600]) else {
                throw DoctoredCoreError(message: "Could not restore the retained RAW for development.")
            }
            return try RawSourceSnapshot(directory: directory, url: url, bytes: bytes, sha256: hash)
        } catch { try? FileManager.default.removeItem(at: directory); throw error }
    }
    private static func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("doctored-raw-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false,
                                               attributes: [.posixPermissions: 0o700])
        return url
    }
}

enum RawSourceWork {
    /// The measured RAW decode charge: `scratchFixed +
    /// scratchPerPixel·px + scratchPerEncodedByte·encoded`. CIRAWFilter owns
    /// unreported platform allocations; this covers the whole measured
    /// phys_footprint peak of a decode (platform, host and core buffers,
    /// metadata). It is admission policy, not an OS memory ceiling or a
    /// cancellation promise. Every decoding caller inherits it, and none is
    /// lowered (the menu's restore admit, which decodes nothing, takes
    /// `RawLocalWork.restoreScratch`):
    /// the open (`openRetainedRaw`, the O cases); `prepare` (MCP
    /// `redevelop_raw`, settings synchronization, decoder review; W and K);
    /// the import preview (`RawDevelopPreviewSession`, V); local development
    /// takes `RawLocalWork.measuredScratch`, measured on its own and never
    /// below this charge; decoder migration (restore +
    /// capture + develop, the K stage) keeps `RawLocalWork.scratch`, this
    /// charge + 56 B/px + 4 MiB; and `develop`'s own guard and inner admit.
    /// Layer ▸ Redevelop RAW…'s restore admit decodes nothing and takes
    /// `RawLocalWork.restoreScratch`; `replacing`'s local branch decodes
    /// nothing, so the figure is above its need. Sequential stages are
    /// charged with max(), not summed.
    ///
    /// `float` is the 32-bit float (linear) development: the charge
    /// plus `floatPerPixel`·px, the formula fixed before it was measured (the
    /// render buffer and the master are 16 B/px where the 16-bit development's
    /// are 8, so 8 more for each).
    static func scratch(width: Int, height: Int, encoded: UInt64,
                        float: Bool = false) throws -> UInt64 {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "The RAW dimensions exceed the supported canvas size.")
        }
        let perPixel = scratchPerPixel + (float ? floatPerPixel : 0)
        return try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(scratchFixed, UInt64(width * height) * perPixel),
            encoded * scratchPerEncodedByte)
    }

    /// `linear_float` exists only in a 32-bit float document: an 8- or
    /// 16-bit document's RAW layer keeps a 16-bit SDR master. Refused by name
    /// before anything is decoded.
    static func requireFloatDocument(_ recipe: RawDevelopmentRecipe, in source: RasterDocument) throws {
        try requireFloatDocument(recipe.outputEncoding, in: source)
    }

    /// The same refusal for a request's encoding, checked before the recipe
    /// is updated so the document's depth is named ahead of a control the
    /// float development refuses.
    static func requireFloatDocument(_ settings: RawDevelopSettings, in source: RasterDocument) throws {
        try requireFloatDocument(settings.outputEncoding, in: source)
    }

    private static func requireFloatDocument(_ encoding: RawOutputEncoding?,
                                             in source: RasterDocument) throws {
        guard encoding == .linearFloat, !source.isFloat else { return }
        throw DoctoredCoreError(message: "raw.output_encoding linear_float needs a 32-bit float "
            + "document: this document is \(source.isHighDepth ? 16 : 8)-bit, where a RAW layer "
            + "keeps a 16-bit SDR master (sdr16).")
    }

    /// Measurement record (refit on real 24–61 MP camera files,
    /// `scripts/raw-decode-probe`). The fit is deterministic: need = ⌈1.25 ×
    /// max E⌉ per fitted case (O, W, K, V), c = 3 per encoded byte; p_ref = the
    /// largest fitted px; `a_floor` = ⌈max (need − 3·encoded) / p_ref⌉ over the
    /// cases at p_ref, the largest measured file's whole cost per pixel; for
    /// each integer a from `a_floor` to the worst ⌈(need − 3·encoded) / px⌉
    /// at ≥ 2 MP, F(a) = the smallest MiB multiple
    /// covering every need − a·px − 3·encoded; the a minimising F(a) + a·p_ref
    /// is taken (the larger on a tie). Every fitted case is charged at least
    /// its need, and above p_ref the charge grows at no less than `a_floor` up
    /// to `maxResizePixels` (100 MP), so no fixed excess is assumed beyond the
    /// measured sizes. One formula for every RAW; no format-specific charge.
    ///
    /// Method: that of `HostStillDecoder`'s record. A -O WMO build of the
    /// app sources with a harness `main`, release core; one case per process
    /// with `MallocLargeCache=0` and `DR_WORKING_LIMIT_BYTES` 32 GiB (no
    /// admission refuses), 3 runs plus a baseline twin; macOS 26.5.1. Each
    /// setup step drains its autorelease pool, then 1 s settles Core Image's
    /// asynchronous release; then `proc_reset_footprint_interval`, pre =
    /// `ri_phys_footprint`, the call, E = `ri_interval_max_phys_footprint` −
    /// pre, read with the results alive. Calibration: 1 GiB touched and
    /// dropped returns 0 B; 256 MiB reads E 268,566,720. Every twin reads E 0.
    /// Modes: O `openRetainedRaw`; W `prepare`'s worker body (restore +
    /// develop, exposure 0.5) after an open; K the same after a `.rz` reopen
    /// (the first CIRAWFilter in the process); V the import preview worker
    /// body (develop, inspection, develop exposure 0.5, inspection, frame).
    ///
    /// Fixtures (dims, encoded bytes, SHA-256): synthetic uncompressed 16-bit
    /// DNGs (`phase3_fixtures.synthetic_dng`) D0 512×384, 394,220 B, de54f3e1…;
    /// D3 2000×1500, 6,001,004 B, 46012d42…; D12 4000×3000, 24,001,004 B,
    /// e94cccde…; D24 6000×4000, 48,001,004 B, d68ab2a2…; D26 6240×4160,
    /// 51,917,804 B, ba345509…; D45 8256×5504, 90,883,052 B, a0f3fe32…; the
    /// lossy-compressed Nikon D70s NEF 3008×2000, 5,193,213 B, 6c46f985…
    /// (`scripts/raw-probe/camera.py`); and CC0 camera files from raw.pixls.us
    /// (listing id): Z6 Nikon Z 6 lossless NEF 6048×4024, 30,164,134 B,
    /// c079345f… (3582); A73 Sony ILCE-7M3 ARW 6000×4000, 25,639,680 B,
    /// 25078458… (2414); K1D Pentax K-1 Mark II compressed DNG 7360×4912,
    /// 46,069,312 B, 190d5878… (3345); A7R3 Sony ILCE-7RM3 ARW 7952×5304,
    /// 43,002,368 B, a7b0279a… (2045); R5 Canon EOS R5 CR3 8192×5464,
    /// 73,484,232 B, d52fca2f… (4693); D850 Nikon D850 lossless NEF 8256×5504,
    /// 55,196,715 B, 0c943825… (1840); A7R4 Sony ILCE-7RM4 ARW 9504×6336,
    /// 61,983,744 B, e6dafe42… (3480).
    ///
    /// Per case, max E over 3 runs of measurement run m1 (its run's pre); the
    /// committed constants are the combined m1 + m2 fit (`--fit-from m1 m2`),
    /// whose binding V16-NEF need is 1,051,607,340 (margin 446,667):
    ///   O16 D0 26,951,776 (21,152,344); D3 242,647,256 (21,037,680); D12 768,000,384
    ///     (21,119,600); D24 1,385,497,272 (21,054,088); D26 1,481,835,264 (21,054,064); D45
    ///     1,684,210,888 (21,004,912); NEF 807,158,288 (17,777,216); Z6 1,787,757,264 (18,727,560);
    ///     A73 1,611,334,472 (18,137,688); K1D 1,992,704,840 (18,186,888); A7R3 2,124,678,056
    ///     (18,055,792); R5 2,151,924,792 (17,580,656); D850 2,059,289,608 (18,186,864); A7R4
    ///     3,639,428,392 (18,416,240)
    ///   O8 D0 27,918,456 (21,021,272); D3 242,630,872 (21,054,064); D12 816,628,144
    ///     (21,037,680); D24 1,381,778,080 (21,070,472); D26 1,481,736,912 (21,021,320); D45
    ///     1,684,210,912 (21,004,912); NEF 806,437,416 (17,777,216); Z6 1,787,577,064 (18,727,536);
    ///     A73 1,611,236,144 (18,121,304); K1D 1,992,590,152 (18,186,888); A7R3 2,125,202,392
    ///     (18,137,664); R5 2,151,908,552 (17,498,688); D850 2,059,174,896 (18,268,784); A7R4
    ///     3,639,690,560 (18,367,112)
    ///   W16 D0 16,629,760 (31,441,592); D3 233,668,680 (60,425,008); D12 759,513,352
    ///     (150,504,336); D24 1,377,092,064 (270,877,704); D26 1,473,151,504 (290,604,112); NEF
    ///     806,174,816 (80,200,496); Z6 1,776,288,008 (256,148,560); A73 1,599,848,688
    ///     (246,907,984); K1D 1,980,563,840 (365,577,440); A7R3 2,112,962,896 (411,731,360); R5
    ///     2,139,505,168 (461,440,440); D850 2,047,017,416 (450,250,192); A7R4 3,626,091,024
    ///     (575,374,944)
    ///   W8 D0 16,891,904 (31,900,344); D3 222,363,720 (72,729,392); D12 666,386,600
    ///     (199,148,600); D24 1,285,964,184 (367,609,008); D26 1,369,506,272 (395,412,680); NEF
    ///     796,426,336 (104,547,168); Z6 1,776,713,992 (354,387,240); A73 1,599,488,264
    ///     (343,606,472); K1D 1,980,416,384 (511,755,680); A7R3 2,113,126,736 (582,829,712); R5
    ///     2,139,537,912 (642,238,096); D850 2,046,886,368 (633,587,344); A7R4 3,625,222,672
    ///     (819,038,152)
    ///   K16 D0 33,898,544 (15,139,464); D3 248,201,360 (66,405,072); D12 773,538,104
    ///     (231,654,240); D24 1,391,313,496 (448,709,592); D26 1,487,094,408 (483,853,296); D45
    ///     1,689,420,976 (836,060,408); NEF 808,829,432 (113,935,080); Z6 1,789,215,464
    ///     (436,224,984); A73 1,613,742,728 (425,821,144); K1D 1,993,818,880 (641,582,208); A7R3
    ///     2,126,349,128 (735,429,808); R5 2,152,743,896 (807,290,080); D850 2,060,583,824
    ///     (799,818,976); A7R4 3,640,853,680 (1,043,547,528)
    ///   K8 D0 35,553,376 (8,700,456); D3 249,905,320 (48,841,304); D12 776,307,024
    ///     (175,391,416); D24 1,393,836,656 (343,917,360); D26 1,489,437,344 (371,376,968); NEF
    ///     810,533,368 (84,296,304); Z6 1,791,591,096 (330,269,488); A73 1,616,020,176
    ///     (321,356,592); K1D 1,996,047,104 (488,424,336); A7R3 2,128,708,472 (558,072,768); R5
    ///     2,154,710,000 (619,840,496); D850 2,062,975,936 (609,502,168); A7R4 3,642,721,480
    ///     (794,346,624)
    ///   V16 D3 253,427,856 (27,312,752); D12 833,503,592 (45,384,376); D24 1,523,532,568
    ///     (69,501,624); NEF 841,040,136 (23,151,192); Z6 1,945,256,896 (49,382,048); A73
    ///     1,770,324,880 (44,073,608); K1D 2,228,192,264 (64,668,368); A7R3 2,412,643,576
    ///     (61,522,568); R5 2,429,928,744 (91,718,376); D850 2,360,804,672 (73,876,176); A7R4
    ///     4,051,453,680 (80,855,760)
    ///   Information: whole `prepare` P16-D24 1,377,288,672, P8-D24
    ///     1,286,177,176.
    ///
    /// Fit: p_ref 60,217,344 (A7R4); a = `a_floor` = 82 B/px, set by V16-A7R4
    /// (need 5,064,317,100); F = 518 MiB, set by V16-NEF (need 1,051,300,170
    /// at 6,016,000 px); objective F + a·p_ref = 5,480,984,576, the least over
    /// a in [82, 173] (each B/px above 82 costs 60 MB at p_ref and saves about
    /// 6 MB of F). Max E per pixel is 45–80 B/px for the real camera files
    /// (the most at 24 MP, Z6), 132–140 for the 6 MP NEF, which the fixed
    /// term covers. Charge ÷ max E is ≥ 1.251 in
    /// every fitted case: V16-NEF 1.251 (margin 753,837), the NEF's other
    /// cases 1.298–1.321, the real files 1.352 (V16-Z6) to 2.167, the
    /// synthetic DNGs ≥ 3 MP 1.743–3.630, D0 15.8–33.7. Charges (previous
    /// 173 B/px + 9 MiB + 3·file in brackets): NEF 1,052,054,007
    /// (1,065,784,823); Z6 2,629,301,234 (4,310,256,882); D850 4,434,916,481
    /// (8,036,324,481); A7R4 5,666,935,808 (10,612,988,928). A second full
    /// run (the confirmation) fits the same 518 MiB and 82 B/px, and so does
    /// the union of both (`--fit-from`), with every fitted case ≥ 1.251×.
    ///
    /// Develop Presence (check-only variants, not fitted: OP opens with
    /// texture, clarity and dehaze 50, VP is V with them on its second
    /// develop; `--only OP16-NEF OP8-NEF VP16-NEF OP16-D850 OP8-D850
    /// VP16-D850`, same method). Max E over 3 runs (twins 0): OP16 NEF 807,256,640, D850
    /// 2,250,000,592; OP8 NEF 807,207,440, D850 2,250,115,304; VP16 NEF
    /// 850,018,592 (the other two runs 840,352,080 and 840,778,040), D850
    /// 2,551,548,400. VP16-NEF's need 1,062,523,240 was above the 518 MiB
    /// charge 1,052,054,007, so F rose by the least MiB multiple that covers
    /// it, 10 MiB, to 528 MiB; a larger fixed term raises every charge, so
    /// every fitted case above stays ≥ 1.251×. Under 528 MiB: VP16-NEF 1.250
    /// (margin 16,527), OP16-NEF 1.316, OP8-NEF 1.316, VP16-D850 1.742,
    /// OP16-D850 and OP8-D850 1.976. Charges now: NEF 1,062,539,767; Z6
    /// 2,639,786,994; D850 4,445,402,241; A7R4 5,677,421,568. A confirmation
    /// run under 528 MiB had no STOP: max E VP16-NEF 840,859,960 (1.264),
    /// OP16-NEF 807,551,552 (1.316), VP16-D850 2,551,351,744 (1.742), OP-D850
    /// 2,250,705,152 (1.975).
    ///
    /// Residency (RES: footprint − P0 − `unique_retained_bytes` after open,
    /// then after `prepare`, settled 1 s): formerly, with the shared
    /// static `CIContext`, D24/16 up to 504 MB, NEF/16 703 MB, still resident;
    /// `clearCaches()` after each render alone still left up to 703 MB.
    /// Since, each `develop` renders through its own context, cleared and
    /// dropped: ≤ 26,462,233 B in every run of both (D24/16, NEF/16, D24/8,
    /// D850/16). Read right after the step it is still up to 776 MB for under
    /// a second.
    ///
    /// Ceilings at the 8 GiB a 16 GB Mac gets (`WorkingLimit.automatic`): the
    /// open's charge plus its captured snapshot (encoded + 4096 B, kept
    /// admitted across the open's second admit and `develop`'s) fits an empty
    /// process up to about 89.3 MP of uncompressed DNG (2 B/px), and 90.7–94.0
    /// MP at the camera files' bytes per pixel (R5 90.7, D850 92.5, A7R4
    /// 93.3, NEF 94.0; 528 MiB), so every measured file, 45 MP included, opens. A
    /// redevelop also carries the open document's retained bytes.
    static let scratchFixed: UInt64 = 528 * 1024 * 1024
    static let scratchPerPixel: UInt64 = 82
    static let scratchPerEncodedByte: UInt64 = 3
    /// The 32-bit float (linear) development's term on top of
    /// `scratchPerPixel`: its render buffer and master are 16 B/px
    /// where the 16-bit development's are 8, so 8 more for each. Fixed before
    /// it was measured. Measurement record (`scripts/raw-decode-probe`,
    /// `measure.py --float`, Release, one case per process, three runs plus a
    /// twin, every twin E 0): charge ÷ max E for O32/W32/K32 is 1.271/1.286/
    /// 1.269 on the 6 MP NEF, 1.548/1.555/2.006 on D24 and 2.091/2.102/2.090
    /// on the 45 MP D850; V32 (the Develop session's float charge, + 24 B/px of
    /// inspection copies) 1.378, 1.721 and 2.257. Since #680 (grain at
    /// float, the extended Fit in V's frame): OG32 (the open with grain)
    /// 1.283/1.553/1.394, VG32 (its preview) 1.387/1.726/1.716 and V32
    /// 1.387/1.726/2.261. Every case ≥ 1.25, so no
    /// constant was raised. At 8 GiB a float open fits an empty process up to
    /// about 75.9 MP of uncompressed DNG and 78.2 MP at D850's bytes per pixel
    /// (at the 518 MiB fixed term; Develop Presence's 528 MiB lowers each by
    /// about 0.1 MP).
    static let floatPerPixel: UInt64 = 16

    static func info(source: RasterDocument, layer: Int) throws -> [String: Any] {
        guard var info = try source.retainedSourceInfo(layer), info["raw"] is [String: Any] else {
            throw DoctoredCoreError(message: "Choose a retained RAW layer.")
        }
        info["saved_rendering_available"] = true
        info["original_embedded"] = true
        do {
            let descriptor = try RawSourceDescriptor.read(info)
            info["decoder_status"] = descriptor.compatible ? "captured_implementation_available" : "unavailable"
            info["redevelopment_note"] = descriptor.compatible
                ? "The captured implementation is installed. File-specific decoder support is checked before redevelopment."
                : "The saved rendering remains usable. Redevelopment requires the captured decoder implementation."
            info["local_count"] = descriptor.recipe.enhancements?.locals.count ?? 0
            // Which master the recipe develops, and its order of evaluation.
            info["output_encoding"] = (descriptor.recipe.outputEncoding ?? .normalized16).rawValue
            info["source_evaluation"] = descriptor.recipe.isLinearFloat
                ? "Camera and vendor lens per variant → chosen output space as linear 32-bit float (tone curve and Shadows in the decoder under the extended range, local tone map off, values above white kept) → Lensfun lens profile when the decoder has none, on the base and each variant (distortion, chromatic aberration and vignetting in linear float) → ordered native local coverage blended in float → manual vignette as a linear gain → explicit source geometry resampled in linear float → develop grain on the display encoding → document profile → retained layer affine/fallback → layer masks and retouch."
                : "Camera and vendor lens per variant → chosen SDR profile → Lensfun lens profile when the decoder has none, on the base and each variant (distortion, chromatic aberration and vignetting) → ordered native local coverage → manual calibrated vignette → explicit source geometry → develop Presence (dehaze, then texture and clarity) → develop grain → document profile → retained layer affine/fallback → layer masks and retouch."
            // v1-lensfun: which lens profile corrects this development.
            info["lens_profile"] = RawImage.lensProfileReport(lens: nil, recipe: descriptor.recipe)
            if descriptor.recipe.enhancements != nil || source.hasSourceAttachment(layer) {
                do {
                    _ = try RawLocalWork.state(source, layer: layer)
                    info["local_resources_available"] = true
                } catch {
                    info["local_resources_available"] = false
                    info["redevelopment_note"] = error.localizedDescription
                }
            }
        } catch {
            info["decoder_status"] = "unavailable"
            info["redevelopment_note"] = error.localizedDescription
        }
        return info
    }

    static func redevelop(in document: ImageDocument, layer: Int, settings: RawDevelopSettings,
                          record: [ActionStep] = .notACommand) throws {
        let capture = try ImageSourceCapture(document)
        guard let result = try prepare(source: capture.source, layer: layer, settings: settings) else { return }
        try publish(result, in: document, capture: capture, title: "Redevelop RAW", record: record)
    }

    /// Prepare privately so settings synchronization can validate and develop
    /// every target before publishing any document. The caller owns stale checks.
    static func prepare(source: RasterDocument, layer: Int,
                        settings: RawDevelopSettings) throws -> RasterDocument? {
        let info = try LinkedImageWork.requireSource(source, layer: layer, editing: true)
        let descriptor = try RawSourceDescriptor.read(info)
        if descriptor.recipe.enhancements != nil || settings.baselineCompensation != nil
            || settings.moire != nil || settings.vignette != nil || settings.grain != nil
            || settings.texture != nil || settings.clarity != nil || settings.dehaze != nil
            || source.hasSourceAttachment(layer) {
            return try RawLocalWork.prepare(source: source, layer: layer, settings: settings)
        }
        // Strict recipe decoding leaves a fixed set of scalar controls and
        // bounded identity strings. Keep its host dictionaries counted while
        // workers run and captured source info remains live.
        let infoPayload = try PrecisionEditPayload.admitted(bytes: 1024 * 1024)
        defer { withExtendedLifetime(infoPayload) {} }
        try requireFloatDocument(settings, in: source)
        var desired = descriptor
        desired.recipe = try descriptor.recipe.updating(settings)
        try requireFloatDocument(desired.recipe, in: source)
        if desired == descriptor { return nil }
        guard descriptor.compatible else {
            throw DoctoredCoreError(message: "The captured RAW decoder implementation is unavailable. Its saved rendering is intact.")
        }
        let raw = info["raw"] as? [String: Any] ?? [:]
        let encoded = try PrecisionDocumentDescriptor.integer(raw, "encoded_bytes")
        let width = try PrecisionDocumentDescriptor.integer(info, "width")
        let height = try PrecisionDocumentDescriptor.integer(info, "height")
        let budget = try scratch(width: Int(width), height: Int(height), encoded: encoded,
                                 float: desired.recipe.isLinearFloat)
        _ = try RasterResources.shared.admit(scratch: budget)
        let requested = desired
        let whiteBalanceChanged = desired.recipe.values.customWhiteBalance
            && (desired.recipe.values.temperature != descriptor.recipe.values.temperature
                || desired.recipe.values.tint != descriptor.recipe.values.tint)
        let decoded = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            let snapshot = try RawSourceSnapshot.restore(source, layer: layer, descriptor: descriptor)
            return try RetainedRawDecoder.develop(url: snapshot.url, descriptor: requested, maximumScratch: budget,
                                                  whiteBalanceChanged: whiteBalanceChanged)
        }, title: "Redeveloping RAW")
        if decoded.1 == descriptor { return nil }
        return try replacing(source: source, layer: layer, asset: decoded.0, descriptor: decoded.1)
    }

    static func replacing(source: RasterDocument, layer: Int, asset: RasterDocument,
                          descriptor: RawSourceDescriptor) throws -> RasterDocument {
        if descriptor.recipe.enhancements != nil || source.hasSourceAttachment(layer) {
            let state = try RawLocalWork.state(source, layer: layer)
            let budget = try RawSourceWork.scratch(width: state.width, height: state.height, encoded: 0)
            return try PrecisionCapturedWork.run(scratchEstimate: budget, title: "Updating Local RAW Layer") { control in
                try RawLocalEvaluation.replacing(source: source, layer: layer, asset: asset,
                    descriptor: descriptor, masks: state.masks, control: control)
            }
        }
        // A linked RAW layer stays linked: the record's own link, which
        // still names the camera file the embedded original came from.
        let link = try source.retainedSourceInfo(layer)?["link"] as? [String: Any]
        let command = try PrecisionPixelCommand(["op": "replace", "layer": layer,
            "revision_id": UUID().uuidString, "depth_conversion": "convert_fallback",
            "link": link.map { $0 as Any } ?? NSNull()])
        let plan = try source.retainedSourcePlan(asset: asset, request: command.request)
        let editScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        let result = try PrecisionCapturedWork.run(scratchEstimate: editScratch, title: "Updating RAW Layer") { control in
            guard let replaced = try source.editingRetainedSource(asset: asset, request: command.request, control: control) else {
                throw DoctoredCoreError(message: "The RAW layer could not be replaced.")
            }
            return try replaced.attachingRawSource(layer, descriptor: descriptor.json(), original: source,
                                                   originalLayer: layer)
        }
        return result
    }

    /// `landing` selects the layer a place made, as `RetainedImageWork.edit` does.
    static func publish(_ result: RasterDocument, in document: ImageDocument, capture: ImageSourceCapture,
                        title: String, record: [ActionStep], landing: Int? = nil) throws {
        try capture.requireCurrent(document)
        let prepared = try BackgroundWork.wait(try WorkerTask { PreparedDocumentEdit(result) }, title: "Preparing RAW Display")
        try capture.requireCurrent(document)
        if let error = prepared.error { throw DoctoredCoreError(message: error) }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(result, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(title, record: record) { current in current === capture.source ? result : nil }
            if let landing, document.doc === result {
                document.activeLayerIndex = min(max(landing, 0), result.layerCount - 1)
            }
        }
    }

    /// Initial open creates a retained source in the requested fallback depth.
    /// The byte path uses the shared mixed-depth evaluator on a temporary empty
    /// canvas; no display preview can become its master. A 32-bit float
    /// (linear) development is `highDepth` with a float `asset`: the
    /// retain keeps it as its own f32 master and the document is 32-bit float.
    static func initialDocument(asset: RasterDocument, descriptor: RawSourceDescriptor,
                                bytes: Data, highDepth: Bool) throws -> RasterDocument {
        let id = UUID().uuidString
        let source: RasterDocument
        if highDepth {
            let command = try PrecisionPixelCommand(["op": "retain", "source_id": id])
            guard let retained = try asset.editingRetainedSource(request: command.request) else {
                throw DoctoredCoreError(message: "Could not retain the developed RAW source.")
            }
            source = retained
        } else {
            guard let blank = RasterImage.from(rgba: [UInt8](repeating: 0, count: asset.width * asset.height * 4),
                                               width: asset.width, height: asset.height),
                  let canvas = RasterDocument.from(image: blank) else {
                throw DoctoredCoreError(message: "Could not prepare the RAW canvas.")
            }
            let command = try PrecisionPixelCommand(["op": "place", "source_id": id, "name": "Background",
                "x": 0, "y": 0, "depth_conversion": "convert_fallback"])
            guard let placed = try canvas.editingRetainedSource(asset: asset, request: command.request),
                  let retained = try placed.removingLayer(0) else {
                throw DoctoredCoreError(message: "Could not retain the RAW source in an 8-bit document.")
            }
            source = retained
        }
        return try source.attachingRawSource(0, bytes: bytes, descriptor: descriptor.json())
    }
}

extension ImageDocument {
    static func openRetainedRaw(_ originalProbe: RawProbe) throws -> Opened {
        if let reason = RawImage.oversizeReason(originalProbe.pixelSize) {
            throw RawImportError(url: originalProbe.url, reason: reason)
        }
        let count = try RawSourceSnapshot.size(originalProbe.url)
        let budget = try RawSourceWork.scratch(width: Int(originalProbe.pixelSize.width),
            height: Int(originalProbe.pixelSize.height), encoded: count)
        _ = try RasterResources.shared.admit(scratch: budget)
        let snapshot = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            try RawSourceSnapshot.capture(originalProbe.url, maximumBytes: count)
        }, title: "Capturing Camera RAW")
        guard let probe = RawImage.inspect(url: snapshot.url), probe.pixelSize == originalProbe.pixelSize else {
            throw RawImportError(url: originalProbe.url, reason: "the source changed while it was being captured")
        }
        let captured = try RetainedRawDecoder.capture(url: snapshot.url,
            filename: originalProbe.url.lastPathComponent, sha256: snapshot.sha256)
        // The `.xmp` sidecar's exposure is the develop's starting point.
        let sidecar = RawSidecar.read(beside: originalProbe.url)
        let base = sidecar?.settings ?? RawDevelopSettings()
        let sidecarNotes = sidecar?.disclosures ?? []
        let settings: RawDevelopSettings
        let highDepth: Bool
        switch RawImportRequest.mode {
        case .headless(let requested):
            settings = base.overlaid(by: requested)
            if case .rgba8 = PrecisionImportRequest.mode {
                // The float development is its own document depth.
                guard settings.outputEncoding != .linearFloat else {
                    throw DoctoredCoreError(message: "raw.output_encoding linear_float opens a 32-bit "
                        + "float document: it cannot be combined with precision rgba8.")
                }
                highDepth = false
            } else { highDepth = true }
        case .ask:
            var seeded = captured
            if !base.isAsShot { seeded.recipe = try captured.recipe.updating(base) }
            guard let chosen = RawDevelopWindowController.runImport(probe, descriptor: seeded, snapshot: snapshot) else {
                throw CocoaError(.userCancelled)
            }
            settings = base.overlaid(by: chosen.settings)
            highDepth = chosen.highDepth
        }
        var desired = captured
        desired.recipe = try captured.recipe.updating(settings)
        let request = desired
        // A float open is `highDepth` with the float recipe; the develop
        // is charged at the float figure, never below the capture's.
        let linearFloat = desired.recipe.isLinearFloat
        guard !linearFloat || highDepth else {
            throw DoctoredCoreError(message: "The 32-bit float (linear) development opens a 32-bit float document.")
        }
        let developBudget = try RawSourceWork.scratch(width: Int(originalProbe.pixelSize.width),
            height: Int(originalProbe.pixelSize.height), encoded: count, float: linearFloat)
        // Which lens, whether the file carries a profile, whether it was applied; and
        // (v1-lensfun) whether the decoder's or the Lensfun profile corrected it.
        let lensReport = RawImage.lensProfileReport(lens: probe.lensModel, recipe: desired.recipe)
        let whiteBalanceChanged = desired.recipe.values.customWhiteBalance
            && (desired.recipe.values.temperature != captured.recipe.values.temperature
                || desired.recipe.values.tint != captured.recipe.values.tint)
        _ = try RasterResources.shared.admit(scratch: developBudget)
        let opened = try BackgroundWork.wait(try WorkerTask(scratchEstimate: developBudget) {
            let decoded = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: request,
                maximumScratch: developBudget, whiteBalanceChanged: whiteBalanceChanged)
            let raw = try RawSourceWork.initialDocument(asset: decoded.0, descriptor: decoded.1,
                                                        bytes: snapshot.bytes, highDepth: highDepth)
            if highDepth {
                // Phase 11D: the typed twins of the two facade calls exist, so
                // the typed RAW import reconstructs the same bounded packets
                // and the same original PPI as the byte branch below. The
                // EXACT camera metadata still lives only inside the retained
                // original file — what is copied here is the platform
                // reader's reconstructed view — and the notice says so.
                let metadata = ImageIOMetadata.read(snapshot.url)
                var captured = metadata.attaching(to: raw)
                if let dpi = Bitmap.imageDPI(snapshot.url) {
                    captured.document = try captured.document.settingResolution(x: dpi.0, y: dpi.1)
                        ?? captured.document
                }
                var report = captured.report
                report["container"] = "camera_raw"
                report["representation"] = "embedded_original_camera_file_and_reconstructed_fields"
                var warnings = (report["warnings"] as? [String]) ?? []
                warnings.append("Original capture metadata, including MakerNotes and private camera fields, is retained EXACTLY only inside the embedded camera file; what a flat export carries is the platform reader's reconstructed view.")
                warnings.append(contentsOf: sidecarNotes)
                report["warnings"] = warnings
                report["lens_profile"] = lensReport
                let withMattes = try NativeAuxiliaryMattes.attaching(to: captured.document,
                                                                     from: snapshot.url)
                return Opened(doc: withMattes, metadataNotCaptured: !metadata.inspected,
                              rawDevelop: settings, metadataReport: report)
            }
            let metadata = ImageIOMetadata.read(snapshot.url)
            var captured = metadata.attaching(to: raw)
            if let dpi = Bitmap.imageDPI(snapshot.url) {
                captured.document = try captured.document.settingResolution(x: dpi.0, y: dpi.1) ?? captured.document
            }
            let withMattes = AuxiliaryMattes.attaching(to: captured.document, from: snapshot.url) ?? captured.document
            var report = captured.report
            if !sidecarNotes.isEmpty {
                report["warnings"] = ((report["warnings"] as? [String]) ?? []) + sidecarNotes
            }
            report["lens_profile"] = lensReport
            return Opened(doc: withMattes, metadataNotCaptured: !metadata.inspected,
                          rawDevelop: settings, metadataReport: report)
        }, title: linearFloat ? "Developing 32-bit Float RAW Source" : "Developing 16-bit RAW Source")
        return opened
    }
}
