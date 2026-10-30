import AppKit

/// A spot channel's ink (#1 phase 1a-2, `designs/spot-channels.md`): D50 CIE Lab and
/// Solidity as an integer percent, 0–100. A spot channel is an alpha channel with an
/// ink: its plane is the ink's coverage (255 / 65535 = full ink), painted, duplicated, deleted and
/// loaded as a selection exactly as an alpha channel is. The core checks the ranges
/// (`spot_channel::SpotInk::new`); the wrappers in `DoctoredCore+SpotChannels.swift` refuse what
/// cannot cross the boundary at all.
struct SpotInk: Equatable {
    var lab: [Double]
    var solidity: Int

    /// The typed info row's `spot` object: `{"lab": [L, a, b], "solidity": n}`; nil for any other
    /// shape.
    init?(json: Any) {
        guard let object = json as? [String: Any],
              let lab = (object["lab"] as? [NSNumber])?.map(\.doubleValue), lab.count == 3,
              lab.allSatisfy(\.isFinite),
              let solidity = (object["solidity"] as? NSNumber)?.intValue, (0...100).contains(solidity)
        else { return nil }
        self.init(lab: lab, solidity: solidity)
    }

    init(lab: [Double], solidity: Int) {
        self.lab = lab
        self.solidity = solidity
    }

    /// Solidity as the C byte; a value outside 0–100 refuses with the core's sentence.
    func solidityByte() throws -> UInt8 {
        guard (0...100).contains(solidity) else {
            throw DoctoredCoreError(message: "solidity must be from 0 to 100 %")
        }
        return UInt8(solidity)
    }

    /// L, a and b; anything but three values refuses with the core's sentence.
    func labTriple() throws -> [Double] {
        guard lab.count == 3 else { throw DoctoredCoreError(message: SpotChannels.labSentence) }
        return lab
    }

    /// The reply/record form: `lab` and `solidity`.
    var fields: [String: Any] { ["lab": lab, "solidity": solidity] }
}

/// The spot channels' sentences, gates and notes.
enum SpotChannels {
    /// An ink colour outside the Lab ranges (the core's `lab_sentence`, word for word).
    static let labSentence = "the spot ink colour must be [L, a, b] with L from 0 to 100 and a and b "
        + "from -128 to 127"
    /// A spot edit addressed to an alpha channel (the core's `NOT_SPOT_REFUSAL`).
    static let notSpotRefusal = "the channel is not a spot channel"
    /// A flat save's loss line for a typed document holding spot channels (the core's `FLAT_LOSS`,
    /// word for word): the flattened file carries their overprint, not the channels.
    static let flatLoss = "spot channels (composited into the image as their overprint)"
    /// The note every flattened export with spot channels carries, at every depth and mode.
    static let exportNote = "Spot channels are composited into this flattened file as their overprint "
        + "(each ink at its coverage, Multiply at 0 % solidity toward covering at 100 %), Doctored's "
        + "reading; the file holds no spot channels."

    /// The default ink for New Spot Channel: red at 0 % solidity.
    static var defaultInk: SpotInk {
        SpotInk(lab: RasterLab.lab(srgb: [1, 0, 0]) ?? [54.29, 80.81, 69.89], solidity: 0)
    }

    /// A PDF export of a document holding spot channels; nil without them. Doctored's PDF cannot
    /// write them as `/Separation` inks, the form #1 1c-2a measured Photoshop's PDF keeping them
    /// in (`spot_tiff_take.py` take2: PDF-FORM SEPARATIONS).
    static func pdfRefusal(_ doc: RasterDocument?) -> String? {
        guard doc?.hasSpotChannels == true else { return nil }
        return "Spot channels are not saved to PDF: Doctored's PDF cannot write them as separate inks; "
            + "save as TIFF, PSD or .doctored, or delete the spot channels first."
    }

    /// A byte TIFF save's working memory for the spot channels it keeps (#1 1c-2a), the core's
    /// `tiff_spot_scratch_bytes` at one byte a sample and three colour samples: each plane stored
    /// as u16 and written at one byte, the interleaved strip and its copies (at most three 8 MiB
    /// strips), and 512 bytes of resources a spot plus two a UTF-16 unit of its name. 0 without
    /// spot channels.
    static func tiffSaveScratch(_ doc: RasterDocument) -> UInt64 {
        let spots = doc.spotIndices()
        guard !spots.isEmpty else { return 0 }
        let count = UInt64(spots.count)
        let pixels = UInt64(doc.width) &* UInt64(doc.height)
        let units = spots.reduce(UInt64(0)) {
            $0 &+ UInt64(doc.channelInfo($1)?.name.utf16.count ?? 0)
        }
        let strip = min(pixels &* (3 + 1 + count), 8 * 1024 * 1024) &* 3
        return count &* pixels &* 3 &+ strip &+ count &* 512 &+ units &* 2
    }

    /// MCP `save_copy`'s byte flat save plan: 12 bytes a pixel, plus `tiffSaveScratch` for a TIFF.
    static func byteSaveScratch(_ doc: RasterDocument, format: ExportFormat) -> UInt64 {
        UInt64(doc.width) &* UInt64(doc.height) &* 12 &+ (format == .tiff ? tiffSaveScratch(doc) : 0)
    }

    /// The reply fields of a byte flat save that kept spot channels (a TIFF):
    /// `working_memory_plan_bytes`, the plan it was admitted at; empty otherwise.
    static func keptPlan(_ doc: RasterDocument, format: ExportFormat) -> [String: Any] {
        guard format == .tiff, doc.hasSpotChannels else { return [:] }
        return ["working_memory_plan_bytes": byteSaveScratch(doc, format: format)]
    }

    /// `fields` with `spot_channels` (the export note) when `doc`'s flattened export composited
    /// spot channels, at any depth.
    static func noting(_ fields: [String: Any], for doc: RasterDocument?) -> [String: Any] {
        guard let doc, doc.hasSpotChannels else { return fields }
        var out = fields
        out["spot_channels"] = exportNote
        return out
    }

    /// `render`'s full canvas with every spot channel overprinted (the composite view's
    /// overprint), and how many were: a byte document through its own profile
    /// (`spotOverprint`, the flattened export's pixels), a typed one's 8-bit display through the
    /// display's profile.
    static func renderComposite(_ image: RasterImage, doc: RasterDocument) throws -> (RasterImage, Int) {
        let spots = doc.spotIndices()
        guard !spots.isEmpty else { return (image, 0) }
        if !doc.isHighDepth { return (try doc.spotOverprint(image) ?? image, spots.count) }
        let icc = doc.colorSpace.copyICCData().map { $0 as Data } ?? Data()
        return (try doc.spotOverprinted(image, channels: spots, icc: icc), spots.count)
    }

    /// `render`'s text after an overprinted full canvas; "" for none.
    static func renderNote(_ count: Int) -> String {
        count == 0 ? "" : ", \(count) spot channel\(count == 1 ? "" : "s") overprinted"
    }

    /// `render`'s region or preview_divisor image with every spot channel overprinted at the
    /// samples the full render's crop holds (output (i, j) is canvas (x + i·d, y + j·d)), and how
    /// many were: a byte document through `spotOverprintRegion`, a typed one's 8-bit display per
    /// channel through its display bytes and the display's profile — `renderComposite`'s inputs.
    static func regionComposite(_ image: RasterImage, doc: RasterDocument,
                                region: RegionRenderRequest) throws -> (RasterImage, Int) {
        let spots = doc.spotIndices()
        guard !spots.isEmpty else { return (image, 0) }
        if !doc.isHighDepth {
            let printed = try doc.spotOverprintRegion(image, x: region.x, y: region.y,
                                                      divisor: region.previewDivisor)
            return (printed ?? image, spots.count)
        }
        let icc = doc.colorSpace.copyICCData().map { $0 as Data } ?? Data()
        var printed = image
        for index in spots {
            guard let ink = doc.channelInfo(index)?.spot, let coverage = doc.channelDisplayBytes(index) else {
                throw DoctoredCoreError(message: "Spot channel \(index) could not be read.")
            }
            printed = try printed.spotOverprintedRegion(
                coverage: coverage, canvasWidth: doc.width, canvasHeight: doc.height, x: region.x,
                y: region.y, divisor: region.previewDivisor, ink: ink, icc: icc)
        }
        return (printed, spots.count)
    }
}

extension RasterDocument {
    /// The indices of the spot channels, in list order.
    func spotIndices() -> [Int] {
        if let precisionMetadata {
            return precisionMetadata.channels.indices.filter { precisionMetadata.channels[$0].spot != nil }
        }
        return (0..<channelCount).filter { byteChannelSpot($0) != nil }
    }

    var hasSpotChannels: Bool { !spotIndices().isEmpty }

    /// Channels ▸ Merge Spot Channel (#1 1c-1): the document flattened with spot channel `i`'s
    /// ink baked in and the channel deleted. A byte document runs directly; a typed one refuses
    /// while the worker is busy, reads the core plan's refusal (a Duotone document, an alpha
    /// channel), admits the plan and runs on the bounded worker with Cancel. nil for an alpha
    /// channel or out of range at byte depth.
    func mergingSpotChannel(_ i: Int) throws -> RasterDocument? {
        guard let precision else { return try byteMergingSpotChannel(i) }
        if BackgroundWork.occupied { throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage) }
        let plan = try precision.mergeSpotChannelPlan(i)
        if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
        let scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        let merged = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Merge Spot Channel") { control in
            let context = try PrecisionWorkContext(source: precision)
            return try precision.mergingSpotChannel(i, workingLimit: context.limit,
                                                    otherRetained: context.otherRetained, control: control)
        }
        return try RasterDocument(precision: merged)
    }

    /// The next free default spot channel name ("Spot Color 1", …), `nextChannelName`'s rule.
    var nextSpotChannelName: String {
        let taken = Set((0..<channelCount).compactMap { channelInfo($0)?.name })
        var n = 1
        while taken.contains("Spot Color \(n)") { n += 1 }
        return "Spot Color \(n)"
    }

    /// `composite` with the spot channels `channels` overprinted in list order, each ink encoded
    /// in the profile `icc`, coverage the channel's 8-bit display bytes: the composite view's
    /// overprint at every depth. Throws when a channel has no ink or a plane will not read.
    func spotOverprinted(_ composite: RasterImage, channels: [Int], icc: Data) throws -> RasterImage {
        var image = composite
        for index in channels.sorted() {
            guard let ink = channelInfo(index)?.spot, let coverage = channelDisplayBytes(index) else {
                throw DoctoredCoreError(message: "Spot channel \(index) could not be read.")
            }
            image = try image.spotOverprinted(coverage: coverage, ink: ink, icc: icc)
        }
        return image
    }

    /// A Channels panel thumbnail for channel `index`: a spot channel's coverage draws ink dark;
    /// an alpha channel's as it is.
    func spotThumbnail(_ index: Int, _ image: RasterImage?) -> RasterImage? {
        guard channelInfo(index)?.spot != nil else { return image }
        return image?.inverted()
    }
}

extension ImageDocument {
    /// A flattened byte save's pixels: the current projection with every spot channel
    /// overprinted (`RasterDocument.spotOverprint`), else the projection itself.
    func flatExportProjection() throws -> RasterImage {
        let projection = try currentProjection()
        guard let doc else { return projection }
        return try doc.spotOverprint(projection) ?? projection
    }

    /// A flat byte save's pixels in `format` (#1 1c-2a): a TIFF keeps the spot channels as
    /// extra samples, so it takes the plain projection; every other format the overprinted one
    /// (`flatExportProjection`).
    func flatSaveProjection(format: ExportFormat?) throws -> RasterImage {
        format == .tiff ? try currentProjection() : try flatExportProjection()
    }
}
