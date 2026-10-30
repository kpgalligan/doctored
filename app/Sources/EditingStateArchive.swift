import AppKit

// The host packages capture value-only geometry on main, encode/read on `BackgroundWork`, and
// materialize AppKit selection paths on main.
//
// `EditingStateArchive` v2 records storage/evaluation/export policy and retains v1 byte reads;
// recovery admission includes facade inspection retention and its parse peak.
//
// Editing-state archive v3 stores exact little-endian native selections and optional validated
// channel-view state, retaining v1/v2 readers.

/// Host editing state around an ordinary native image. The package schema is
/// independent of DRDC: no new pixel or document semantics are hidden from core.
/// Capture and materialization run on main; disk IO receives values only.
enum EditingStateArchive {
    static let version = 4
    private static let metadataLimit = 4 * 1024 * 1024

    struct SelectionValue: Codable {
        let kind: String
        let coordinates: [Double]
        let maskFile: String?
    }

    struct Manifest: Codable {
        let version: Int
        let width: Int
        let height: Int
        let selection: SelectionValue?
        let previousSelection: SelectionValue?
        let quickMaskActive: Bool
        let primaryLayer: Int
        let otherLayers: [Int]
        let target: String
        let targetChannel: Int?
        let jpegQuality: Int
        let embedProfile: Bool
        let stripMetadata: Bool
        let decodeEstimateBytes: UInt64
        // Version 1 had byte documents only. Version 2 states its native
        // storage/evaluation contract before decoding or restoring UI values.
        // Version 4 admits the linear-light tag beside the two v2 spellings
        // (`DocumentEvaluation.swift`); an older build meeting it refuses the
        // archive, which is the disclosure.
        var storage: String? = nil
        var evaluation: String? = nil
        var exportBitDepth: Int? = nil
        var exportDither: String? = nil
        var channelView: ChannelViewValue? = nil
    }

    struct ChannelViewValue: Codable {
        let rgb: Bool
        let plane: String?
        let mask: Bool
        let channels: [Int]
    }

    struct Captured {
        let document: RasterDocument
        let manifest: Manifest
        let masks: [String: [UInt8]]
        let scratchEstimate: UInt64
        let reopenedRetainedEstimate: UInt64
        let projectionScratchEstimate: UInt64
        /// The state's own non-evictable retained input, passed to `admit`.
        var ownRetainedBytes: UInt64 = 0

        var refusalRemedy: String {
            document.precision != nil ? RasterResources.depthJobRemedy : RasterResources.smallerJobRemedy
        }
    }

    struct Decoded {
        let document: RasterDocument
        let manifest: Manifest
        let masks: [String: [UInt8]]

        func editingState() throws -> EditingState {
            assert(Thread.isMainThread)
            let selection = try EditingStateArchive.materialize(
                manifest.selection, manifest: manifest, masks: masks)
            let previous = try EditingStateArchive.materialize(
                manifest.previousSelection, manifest: manifest, masks: masks)
            return EditingState(
                document: document,
                selection: SelectionHistoryState(selection: selection, previous: previous,
                                                  quickMaskActive: manifest.quickMaskActive),
                layerSelection: LayerSelection(primary: manifest.primaryLayer,
                                                others: Set(manifest.otherLayers)),
                paintTarget: try EditingStateArchive.target(manifest, document: document),
                jpegExportQuality: manifest.jpegQuality,
                embedColorProfile: manifest.embedProfile,
                stripMetadata: manifest.stripMetadata,
                exportBitDepth: manifest.version == 1 ? 16 : manifest.exportBitDepth ?? 16,
                exportDither: manifest.version == 1 ? "none" : manifest.exportDither ?? "none",
                channelVisibility: try EditingStateArchive.channelView(manifest, document: document))
        }
    }

    static func capture(_ state: EditingState) throws -> Captured {
        assert(Thread.isMainThread)
        var masks: [String: [UInt8]] = [:]
        let selection = captureSelection(state.selection.selection, file: "selection.mask", masks: &masks)
        let previous = captureSelection(state.selection.previous, file: "previous.mask", masks: &masks)
        let targetName: String
        var channel: Int?
        switch state.paintTarget {
        case .layer: targetName = "layer"
        case .mask: targetName = "mask"
        case .plane(let plane): targetName = plane.agentName
        case .channel(let index): targetName = "channel"; channel = index
        case .ink(let plate): targetName = plate.key
        }
        let inventory = try RasterDocument.resourceReport(
            documents: [state.document], images: [], scratch: 0, limit: 0)
        guard let retained = (inventory["unique_retained_bytes"] as? NSNumber)?.uint64Value,
              let caches = (inventory["cache_bytes"] as? NSNumber)?.uint64Value else {
            throw failure("could not estimate archive memory")
        }
        let selections = selectionEstimate(state.selection.selection)
            + selectionEstimate(state.selection.previous)
        let typedInfo = try state.document.precision?.descriptor()
        let descriptor = try typedInfo.map { try PrecisionDocumentDescriptor($0) }
        let ownRetained = try typedInfo.map { try PrecisionDocumentDescriptor.integer($0, "retained_bytes") }
            ?? retained - min(retained, caches)
        let decodeEstimate: UInt64
        let reopenedRetained: UInt64
        var projection = try state.document.projectionScratchEstimate()
        if let descriptor, let info = state.document.precisionInfo {
            let inspection = try PrecisionDocumentDescriptor.integer(info, "inspection_bytes")
            let (inspectionScratch, overflow) = inspection.multipliedReportingOverflow(by: 3)
            guard !overflow else { throw failure("inspection estimate overflow") }
            reopenedRetained = try PrecisionDocumentDescriptor.sum(
                PrecisionDocumentDescriptor.sum(descriptor.reopenedRetainedBytes, inspection), selections)
            let decode = try PrecisionDocumentDescriptor.sum(descriptor.reopenedRetainedBytes,
                max(descriptor.decodeScratchBytes, inspectionScratch))
            decodeEstimate = try PrecisionDocumentDescriptor.sum(decode,
                PrecisionDocumentDescriptor.sum(UInt64(metadataLimit), selections * 2))
        } else {
            // The core's reopen plan (`drdc_reopen_plan`) counts each reader-shared
            // allocation once, so retained is an upper bound on what the reader retains.
            let plan = try state.document.nativeReopenPlan()
            decodeEstimate = try PrecisionDocumentDescriptor.sum(plan.decodeBytes, selections * 2)
            reopenedRetained = try PrecisionDocumentDescriptor.sum(plan.retainedBytes, selections)
            // Gate R measured the 8-bit projection stage at 912 MB against P =
            // 1,090.6 MB (1.196×); with retained exact, the quarter keeps the
            // reopen check at ≥ 1.25× the measured reopen peak.
            projection = try PrecisionDocumentDescriptor.sum(projection, projection / 4 + (projection % 4 > 0 ? 1 : 0))
        }
        let manifest = Manifest(
            version: version, width: state.document.width, height: state.document.height,
            selection: selection, previousSelection: previous,
            quickMaskActive: state.selection.quickMaskActive,
            primaryLayer: state.layerSelection.primary, otherLayers: state.layerSelection.others.sorted(),
            target: targetName, targetChannel: channel,
            jpegQuality: state.jpegExportQuality, embedProfile: state.embedColorProfile,
            stripMetadata: state.stripMetadata, decodeEstimateBytes: decodeEstimate,
            storage: descriptor?.storageName ?? "rgba8",
            evaluation: state.document.evaluation.archiveTag(isHighDepth: descriptor != nil),
            exportBitDepth: state.exportBitDepth, exportDither: state.exportDither,
            channelView: ChannelViewValue(rgb: state.channelVisibility.rgb, plane: state.channelVisibility.plane?.agentName,
                mask: state.channelVisibility.mask, channels: state.channelVisibility.channels.sorted()))
        // The native encoder uses the core's 8-bit save plan (`drdc_save_plan`).
        // Mask Data writes can temporarily copy both coverage planes; geometric
        // metadata is capped.
        let scratch = (try descriptor?.nativeScratchBytes ?? state.document.nativeSaveScratchBytes())
            + UInt64(metadataLimit)
            + masks.values.reduce(UInt64(0)) { $0 + UInt64($1.count) * 2 }
        return Captured(document: state.document, manifest: manifest, masks: masks,
                        scratchEstimate: scratch,
                        reopenedRetainedEstimate: reopenedRetained,
                        projectionScratchEstimate: projection,
                        ownRetainedBytes: ownRetained)
    }

    /// Creates a new directory. The caller publishes it atomically only after
    /// every file is complete; an error removes only this newly created directory.
    static func write(_ captured: Captured, to directory: URL) throws {
        let files = FileManager.default
        guard mkdir(directory.path, 0o700) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        do {
            let data = try JSONEncoder().encode(captured.manifest)
            guard data.count <= metadataLimit else { throw failure("selection geometry exceeds the archive limit") }
            try captured.document.saveNative(to: directory.appendingPathComponent("image.rz"))
            for (name, bytes) in captured.masks {
                try Data(bytes).write(to: directory.appendingPathComponent(name))
            }
            try data.write(to: directory.appendingPathComponent("state.json"))
        } catch {
            try? files.removeItem(at: directory)
            throw error
        }
    }

    /// Decode native data and plain values on the worker. Never creates AppKit
    /// paths or coverage display images there; editingState() does that on main.
    static func read(from directory: URL) throws -> Decoded {
        let attributes = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard attributes.isDirectory == true, attributes.isSymbolicLink != true else {
            throw failure("expected an editing-state package")
        }
        let manifestData = try readFile(directory.appendingPathComponent("state.json"),
                                        maximum: metadataLimit)
        let manifest = try JSONDecoder().decode(Manifest.self, from: manifestData)
        guard (1...version).contains(manifest.version) else {
            throw failure("unsupported editing-state version")
        }
        // Phase 11F: a typed archive is either the u16 mode or the 32-bit
        // FLOAT one. A storage name outside the pair is refused below.
        let typed = PrecisionDocumentDescriptor.admittedStorages.contains(manifest.storage ?? "")
        let float = manifest.storage == PrecisionDocumentDescriptor.floatStorage
        var archivedPolicy = EvaluationPolicy.encoded
        if manifest.version >= 2 {
            // A tag this build does not know is REFUSED, never repaired: the
            // evaluation policy is a claim about every pixel in the archive.
            guard let policy = EvaluationPolicy.fromArchiveTag(manifest.evaluation ?? ""),
                  policy.archiveTag(isHighDepth: typed) == manifest.evaluation,
                  !float || policy == .linearLight,
                  typed || manifest.storage == "rgba8",
                  [8, 16, 32].contains(manifest.exportBitDepth ?? 0),
                  ["none", "ordered"].contains(manifest.exportDither ?? "") else {
                throw failure("unsupported storage, evaluation or export policy")
            }
            guard policy == .encoded || manifest.version >= 4 else {
                throw failure("linear-light evaluation requires editing-state version 4")
            }
            archivedPolicy = policy
        }
        // Our writer's referenced-payload estimate accounts for duplicated
        // serialized planes. Treat it as admission guidance; core still validates
        // every native record and enforces its own independent format caps.
        guard manifest.width > 0, manifest.height > 0,
              manifest.width <= RasterImage.maxResizePixels / manifest.height,
              manifest.decodeEstimateBytes >= (typed ? UInt64(metadataLimit) : 64 * 1024 * 1024) else {
            throw failure("invalid archive dimensions or decode estimate")
        }
        _ = try RasterResources.shared.admit(scratch: manifest.decodeEstimateBytes)
        let nativeURL = directory.appendingPathComponent("image.rz")
        try requireRegularFile(nativeURL)
        let native = try FileHandle(forReadingFrom: nativeURL)
        defer { try? native.close() }
        guard let head = try native.read(upToCount: 4), ImageDocument.isNativeMagic(head) else {
            throw failure("image.rz is not a native document")
        }
        let document = try RasterDocument.open(url: nativeURL)
        guard (document.precision != nil) == typed, document.isFloat == float,
              manifest.version != 1 || document.precision == nil else {
            throw failure("archive storage policy does not match its native image")
        }
        // The manifest's claim is CROSS-CHECKED against the decoded document:
        // the `.rz` carries the policy itself (the EVAL wrapper), so the two
        // must agree or the archive is describing a different picture.
        guard manifest.version < 2 || document.evaluation == archivedPolicy else {
            throw failure("archive evaluation policy does not match its native image")
        }
        guard manifest.width == document.width, manifest.height == document.height,
              manifest.primaryLayer >= 0, manifest.primaryLayer < document.layerCount,
              manifest.otherLayers.count <= document.layerCount,
              Set(manifest.otherLayers).count == manifest.otherLayers.count,
              !manifest.otherLayers.contains(manifest.primaryLayer),
              manifest.otherLayers.allSatisfy({ $0 >= 0 && $0 < document.layerCount }),
              (1...100).contains(manifest.jpegQuality) else {
            throw failure("editing state does not match its native image")
        }
        _ = try target(manifest, document: document)
        _ = try channelView(manifest, document: document)
        var masks: [String: [UInt8]] = [:]
        for (selection, file) in [(manifest.selection, "selection.mask"),
                                  (manifest.previousSelection, "previous.mask")] {
            guard let selection else { continue }
            try validate(selection, expectedMask: file)
            if selection.kind == "mask" || selection.kind == "mask16" {
                guard selection.kind != "mask16" || manifest.version >= 3 else {
                    throw failure("native selection coverage requires editing-state version 3")
                }
                let expected = document.width * document.height * (selection.kind == "mask16" ? 2 : 1)
                let bytes = try readFile(directory.appendingPathComponent(file), maximum: expected)
                guard bytes.count == expected else { throw failure("selection mask dimensions do not match") }
                guard bytes.contains(where: { $0 != 0 }) else { throw failure("archive contains an empty selection") }
                masks[file] = Array(bytes)
            } else {
                try validateBounds(selection, width: document.width, height: document.height)
            }
        }
        return Decoded(document: document, manifest: manifest, masks: masks)
    }

    static func readFile(_ url: URL, maximum: Int) throws -> Data {
        try requireRegularFile(url)
        let values = try url.resourceValues(forKeys: [.fileSizeKey])
        guard let size = values.fileSize, size >= 0, size <= maximum else {
            throw failure("\(url.lastPathComponent) exceeds its size limit")
        }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: maximum + 1) ?? Data()
        guard data.count <= maximum else { throw failure("\(url.lastPathComponent) exceeds its size limit") }
        return data
    }

    private static func requireRegularFile(_ url: URL) throws {
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true else {
            throw failure("\(url.lastPathComponent) must be a regular file")
        }
    }

    private static func captureSelection(_ selection: CanvasSelection?, file: String,
                                         masks: inout [String: [UInt8]]) -> SelectionValue? {
        guard let selection else { return nil }
        switch selection.shape {
        case .rect(let rect): return rectValue("rect", rect)
        case .ellipse(let rect): return rectValue("ellipse", rect)
        case .polygon(let points):
            return SelectionValue(kind: "polygon", coordinates: points.flatMap { [Double($0.x), Double($0.y)] }, maskFile: nil)
        case .mask16(let samples):
            masks[file] = samples.flatMap { [UInt8($0 & 255), UInt8($0 >> 8)] }
            return SelectionValue(kind: "mask16", coordinates: [], maskFile: file)
        case .mask(let bytes):
            masks[file] = bytes
            return SelectionValue(kind: "mask", coordinates: [], maskFile: file)
        }
    }

    private static func rectValue(_ kind: String, _ rect: CGRect) -> SelectionValue {
        SelectionValue(kind: kind, coordinates: [Double(rect.minX), Double(rect.minY),
            Double(rect.width), Double(rect.height)], maskFile: nil)
    }

    /// Disk records reconstruct their own arrays, display images and contours,
    /// even if the in-memory current/previous selections share those objects.
    private static func selectionEstimate(_ selection: CanvasSelection?) -> UInt64 {
        guard let selection else { return 0 }
        switch selection.shape {
        case .mask, .mask16:
            let count: Int
            switch selection.shape {
            case .mask(let bytes): count = bytes.count
            case .mask16(let samples): count = samples.count * 2
            default: preconditionFailure()
            }
            let images = [selection.maskImage, selection.inverseMaskImage].compactMap { $0 }
                .reduce(UInt64(0)) { $0 + UInt64($1.bytesPerRow * $1.height) }
            return UInt64(count) + images + UInt64(selection.maskContour?.elementCount ?? 0) * 64
        case .polygon(let points): return UInt64(points.count * MemoryLayout<CGPoint>.stride)
        case .rect, .ellipse: return 0
        }
    }

    private static func validate(_ value: SelectionValue, expectedMask: String) throws {
        guard value.coordinates.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000_000 }) else {
            throw failure("invalid selection coordinates")
        }
        switch value.kind {
        case "rect", "ellipse":
            guard value.maskFile == nil, value.coordinates.count == 4,
                  value.coordinates[2] > 0, value.coordinates[3] > 0 else {
                throw failure("invalid selection rectangle")
            }
        case "polygon":
            guard value.maskFile == nil, value.coordinates.count >= 6,
                  value.coordinates.count <= 200_000, value.coordinates.count % 2 == 0 else {
                throw failure("invalid selection polygon")
            }
        case "mask", "mask16":
            guard value.maskFile == expectedMask, value.coordinates.isEmpty else {
                throw failure("invalid selection mask record")
            }
        default: throw failure("unknown selection kind")
        }
    }

    private static func materialize(_ value: SelectionValue?, manifest: Manifest,
                                     masks: [String: [UInt8]]) throws -> CanvasSelection? {
        guard let value else { return nil }
        let coordinates = value.coordinates
        let shape: CanvasSelection.Shape
        switch value.kind {
        case "rect", "ellipse":
            let rect = CGRect(x: coordinates[0], y: coordinates[1],
                              width: coordinates[2], height: coordinates[3])
            shape = value.kind == "rect" ? .rect(rect) : .ellipse(rect)
        case "polygon":
            shape = .polygon(stride(from: 0, to: coordinates.count, by: 2).map {
                CGPoint(x: coordinates[$0], y: coordinates[$0 + 1])
            })
        case "mask", "mask16":
            guard let file = value.maskFile, let bytes = masks[file] else { throw failure("missing selection mask") }
            if value.kind == "mask16" {
                guard bytes.count == manifest.width * manifest.height * 2 else { throw failure("native selection length mismatch") }
                shape = .mask16(stride(from: 0, to: bytes.count, by: 2).map { UInt16(bytes[$0]) | UInt16(bytes[$0 + 1]) << 8 })
            } else { shape = .mask(bytes) }
        default: throw failure("unknown selection kind")
        }
        guard let result = CanvasSelection(shape: shape, canvasWidth: manifest.width,
                                           canvasHeight: manifest.height) else {
            throw failure("archive contains an empty selection")
        }
        return result
    }

    /// Mirror only CanvasSelection.init's geometric emptiness test, without
    /// building its AppKit paths/images on the worker. Reject before returning
    /// Decoded so recovery can fall back to the previous complete generation.
    private static func validateBounds(_ value: SelectionValue, width: Int, height: Int) throws {
        let coordinates = value.coordinates
        let box: CGRect
        if value.kind == "polygon" {
            var bounds = CGRect(x: coordinates[0], y: coordinates[1], width: 0, height: 0)
            for index in stride(from: 2, to: coordinates.count, by: 2) {
                bounds = bounds.union(CGRect(x: coordinates[index], y: coordinates[index + 1], width: 0, height: 0))
            }
            box = bounds
        } else {
            box = CGRect(x: coordinates[0], y: coordinates[1], width: coordinates[2], height: coordinates[3])
        }
        let clipped = box.integral.intersection(CGRect(x: 0, y: 0, width: width, height: height))
        guard !clipped.isEmpty, clipped.width >= 1, clipped.height >= 1 else {
            throw failure("archive contains an empty selection")
        }
    }

    private static func target(_ manifest: Manifest, document: RasterDocument) throws -> PaintTarget {
        switch manifest.target {
        case "layer": return .layer
        case "mask":
            guard document.layerHasMask(manifest.primaryLayer) else {
                throw failure("mask target has no layer mask")
            }
            return .mask
        case "channel":
            guard let index = manifest.targetChannel, index >= 0, index < document.channelCount else {
                throw failure("channel target does not exist")
            }
            return .channel(index)
        default:
            // #1: one ink plate of a CMYK document.
            if let plate = CmykPlane(key: manifest.target), document.isCmyk { return .ink(plate) }
            guard let plane = RasterPlane.named(manifest.target), PaintTarget.paintablePlanes.contains(plane) else {
                throw failure("invalid paint target")
            }
            return .plane(plane)
        }
    }

    private static func channelView(_ manifest: Manifest, document: RasterDocument) throws -> ChannelVisibility {
        guard let value = manifest.channelView else { return ChannelVisibility() }
        guard manifest.version >= 3, value.channels.count <= document.channelCount,
              Set(value.channels).count == value.channels.count,
              value.channels.allSatisfy({ (0..<document.channelCount).contains($0) }),
              !value.mask || document.layerHasMask(manifest.primaryLayer) else {
            throw failure("invalid channel display state")
        }
        let plane = value.plane.flatMap(RasterPlane.named)
        guard value.plane == nil || (plane != nil && plane != .mask && !value.rgb) else {
            throw failure("invalid displayed color plane")
        }
        return ChannelVisibility(rgb: value.rgb, plane: plane, mask: value.mask, channels: Set(value.channels))
    }

    private static func failure(_ message: String) -> DoctoredCoreError {
        DoctoredCoreError(message: "Editing-state archive: \(message).")
    }
}
