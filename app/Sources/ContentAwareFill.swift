import AppKit

// Canonical Actions store settings/geometry rather than review IDs; nested recipe points count
// toward the recorder budget.

/// Bounded geometry describes source eligibility; only Rust rasterizes it for
/// synthesis. The selection remains a separate soft destination coverage plane.
struct ContentAwareSourceMask {
    var initial = "all"
    var operations: [[String: Any]] = []
    var fields: [String: Any] { ["version": 1, "initial": initial, "operations": operations] }

    mutating func append(_ operation: [String: Any]) throws {
        var next = self
        next.operations.append(operation)
        let data = try JSONSerialization.data(withJSONObject: next.fields)
        let points = next.operations.reduce(0) { $0 + (($1["points"] as? [[Double]])?.count ?? 0) }
        guard operations.count < 512, points <= 4096, data.count <= 60 * 1024 else {
            throw DoctoredCoreError(message: "Source editing supports 512 operations and 4096 brush points. Reset the source area to simplify it.")
        }
        self = next
    }
}

struct ContentAwareFillSettings {
    var ring = 0
    var seed: UInt64 = 0
    var choice = RetouchSamplingChoice(mode: .current, ignoreAdjustments: false)
    var output = RetouchOutput.layer
    var sourceMask: [String: Any]?
    /// The core's opt-in pad: the hole's transparent pixels are made opaque
    /// before the fill writes them. Only Auto-Blend's Content-Aware Fill Transparent Areas sets
    /// it; sent only when set, so every other request and recording is unchanged.
    var fillTransparent = false

    func arguments(layer: Int) -> [String: Any] {
        var fields: [String: Any] = ["layer": layer, "ring": ring, "seed": seed,
            "sample": choice.mode.rawValue, "ignore_adjustments": choice.ignoreAdjustments,
            "output": output.rawValue]
        if let sourceMask { fields["source_mask"] = sourceMask }
        if fillTransparent { fields["fill_transparent"] = true }
        return fields
    }
    func record(layer: Int) -> [ActionStep] {
        .step("content_aware_fill", arguments(layer: layer), note: "Edit ▸ Content-Aware Fill…")
    }
}

/// Both depths plan the exact same immutable request before admission. RGB
/// never passes through a byte display image; only declared u8 selection does.
struct ContentAwareFillWork {
    let source: RasterDocument
    let layer: Int
    let settings: ContentAwareFillSettings
    let coverage: [UInt8]
    let command: PrecisionPixelCommand
    let typed: PrecisionPixelEdit?
    let payload: PrecisionEditPayload
    let scratch: UInt64

    /// `ownedWorker`: constructed on the worker of the job that holds `BackgroundWork`
    /// (`AutoBlendFill.run`); `PrecisionPixelEdit`'s own parameter.
    init(source: RasterDocument, layer: Int, coverage: [UInt8], settings: ContentAwareFillSettings, nativeCoverage: [UInt16]? = nil, ownedWorker: Bool = false) throws {
        guard coverage.count == source.width * source.height else {
            throw DoctoredCoreError(message: "The captured fill coverage does not match the canvas.")
        }
        var x0 = source.width, y0 = source.height, x1 = -1, y1 = -1
        for (index, value) in (nativeCoverage ?? coverage.map { UInt16($0) * 257 }).enumerated() where value > 0 {
            let x = index % source.width, y = index / source.width
            x0 = min(x0, x); y0 = min(y0, y); x1 = max(x1, x); y1 = max(y1, y)
        }
        guard x1 >= x0 else { throw DoctoredCoreError(message: "There is no selection to fill.") }
        self.source = source; self.layer = layer; self.settings = settings; self.coverage = coverage
        var fields = settings.arguments(layer: layer)
        fields["op"] = "content_aware_fill"
        fields["x"] = x0; fields["y"] = y0
        fields["width"] = x1 - x0 + 1; fields["height"] = y1 - y0 + 1
        // The byte kernel's hole is every coverage byte ≥ 1, so a feathered ramp counts; the
        // count sets the admission's lift and the edit refuses any other count.
        if source.precision == nil { fields["covered"] = coverage.reduce(0) { $1 > 0 ? $0 + 1 : $0 } }
        command = try PrecisionPixelCommand(fields)
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(coverage.count) + 1024 * 1024)
        if let precision = source.precision {
            let input = try PrecisionScalarBuffer(samples: nativeCoverage ?? coverage.map { UInt16($0) * 257 }, width: source.width, height: source.height)
            let edit = try PrecisionPixelEdit(precision, command: command, input: input, ownedWorker: ownedWorker)
            typed = edit; scratch = edit.scratchEstimate
        } else {
            typed = nil
            scratch = try source.contentAwareFillScratch(command.request)
        }
    }

    func compute(control: RasterJobControl) throws -> RasterDocument? {
        let updated: RasterDocument?
        if let typed { updated = try typed.compute(control: control).map { try RasterDocument(precision: $0) } }
        else { updated = try source.contentAwareFillEdit(command.request, coverage: coverage, control: control) }
        guard let updated else { return nil }
        // Byte descriptions are host-owned. Native artwork is detached by the
        // typed kernel; a new repair must never detach the original artwork.
        if settings.output == .layer, !source.isHighDepth,
           LayerDescription.claimedKind(of: source.layerMeta(layer)) != nil {
            guard let rasterized = updated.withLayerMeta(layer, nil) else {
                throw DoctoredCoreError(message: "Could not detach the changed layer description.")
            }
            return rasterized
        }
        return updated
    }
}

struct ContentAwareFillCandidate {
    let prepared: PreparedDocumentEdit
    let before: CGImage
    let payload: PrecisionEditPayload
}

enum ContentAwareFillPublication {
    static func apply(_ candidate: ContentAwareFillCandidate, capture: ImageSourceCapture,
                      settings: ContentAwareFillSettings, layer: Int, to document: ImageDocument,
                      record: [ActionStep]) throws {
        try capture.requireCurrent(document)
        if let error = candidate.prepared.error { throw DoctoredCoreError(message: error) }
        document.preparedEdit = candidate.prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(candidate.prepared.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit("Content-Aware Fill", record: record) { current in
                current === capture.source ? candidate.prepared.document : nil
            }
        }
        guard document.doc === candidate.prepared.document else {
            throw DoctoredCoreError(message: "The reviewed fill could not be published.")
        }
    }
}
