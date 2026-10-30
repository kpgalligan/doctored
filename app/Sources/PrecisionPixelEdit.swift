import Foundation

// The 7D scalar commands do exact input-layout validation, and captured reads/edits run through the
// existing worker.

/// Scalar samples only. Copies share their lease; independently constructed
/// values conservatively count separately even if Swift shares Array storage.
struct PrecisionScalarBuffer {
    enum Provenance: String { case nativeU16, promotedU8, mixed }
    let samples: [UInt16]
    let width: Int
    let height: Int
    let planeCount: Int
    let provenance: Provenance
    let retainedPayload: PrecisionEditPayload

    init(samples: [UInt16], width: Int, height: Int, planeCount: Int = 1,
         provenance: Provenance = .nativeU16) throws {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              (1...2).contains(planeCount), samples.count == width * height * planeCount else {
            throw DoctoredCoreError(message: "High-depth scalar samples do not match their declared extent.")
        }
        retainedPayload = try PrecisionEditPayload.admitted(bytes: UInt64(samples.count) * 2 + 512)
        self.samples = samples
        self.width = width
        self.height = height
        self.planeCount = planeCount
        self.provenance = provenance
    }

    /// The sole permitted host depth adapter: scalar tool coverage, not RGB.
    /// All output codes are multiples of 257; no additional information is made.
    static func promoting(_ bytes: [UInt8], width: Int, height: Int) throws -> Self {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              bytes.count == width * height else {
            throw DoctoredCoreError(message: "Byte coverage does not match its declared extent.")
        }
        _ = try RasterResources.shared.admit(scratch: UInt64(bytes.count) * 3 + 512)
        return try Self(samples: bytes.map { UInt16($0) * 257 }, width: width, height: height,
                        provenance: .promotedU8)
    }

    static func joining(_ planes: [Self]) throws -> Self {
        guard let first = planes.first, (1...2).contains(planes.count),
              planes.allSatisfy({ $0.width == first.width && $0.height == first.height && $0.planeCount == 1 }) else {
            throw DoctoredCoreError(message: "Rasterization needs one or two scalar planes of equal dimensions.")
        }
        if planes.count == 1 { return first }
        let count = first.samples.count * planes.count
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 2 + 512)
        var samples: [UInt16] = []
        samples.reserveCapacity(count)
        for plane in planes { samples.append(contentsOf: plane.samples) }
        let provenance = planes.allSatisfy { $0.provenance == first.provenance } ? first.provenance : .mixed
        return try Self(samples: samples, width: first.width, height: first.height,
                        planeCount: planes.count, provenance: provenance)
    }
}

/// Exact shared core request schema, kept as a value rather than inferred from
/// the current editor. Core owns operation/range/target validation and planning.
struct PrecisionPixelCommand {
    let request: String

    /// The core's request bounds (`ffi_precision_edit.rs`): `REQUEST_LIMIT` for every op, and
    /// `STYLE_REQUEST_LIMIT` for `layer_style`, whose whole style may reach `MAX_STYLE_JSON_LEN`
    /// (2 MiB, a Pattern Overlay's custom tile) beside that envelope.
    static let maximumBytes = 64 * 1024
    static let maximumStyleBytes = 2 * 1024 * 1024 + maximumBytes

    init(_ fields: [String: Any]) throws {
        guard JSONSerialization.isValidJSONObject(fields), let op = fields["op"] as? String else {
            throw DoctoredCoreError(message: "High-depth pixel request must contain finite JSON values and an operation.")
        }
        // Unescaped slashes: an escaped `/` doubles a base64 tile's opaque texels.
        let bytes = try JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys, .withoutEscapingSlashes])
        let maximum = op == "layer_style" ? Self.maximumStyleBytes : Self.maximumBytes
        guard bytes.count <= maximum, let request = String(data: bytes, encoding: .utf8) else {
            throw DoctoredCoreError(message: "High-depth pixel request exceeds \(maximum / 1024) KiB.")
        }
        self.request = request
    }

    /// A request already serialized: a JSON object's text taken as it is (its own spelling kept,
    /// `IndexedDocument.splicedRequest`), within `maximumBytes`.
    init(text: String) throws {
        guard text.hasPrefix("{"), text.hasSuffix("}") else {
            throw DoctoredCoreError(message: "High-depth pixel request must be a JSON object.")
        }
        guard text.utf8.count <= Self.maximumBytes else {
            throw DoctoredCoreError(message: "High-depth pixel request exceeds \(Self.maximumBytes / 1024) KiB.")
        }
        request = text
    }

    static func readPlane(target: String, index: Int, canvas: Bool = false) throws -> Self {
        try Self(["op": "read_plane", "target": target, "index": index, "canvas": canvas])
    }
}

/// Internal immutable edit capture. An eventual 7E editor integration must still
/// validate its document/target revision before publishing this returned owner.
/// No existing ImageDocument state or Undo registration is touched here.
struct PrecisionPixelEdit {
    let source: RasterPrecisionDocument
    let command: PrecisionPixelCommand
    let input: PrecisionScalarBuffer?
    let scratchEstimate: UInt64
    let retainedPayload: PrecisionEditPayload

    /// `ownedWorker`: the caller is the worker of the job that holds `BackgroundWork`
    /// (Auto-Blend's Content-Aware Fill Transparent Areas), so the occupancy
    /// it would refuse is its own.
    init(_ source: RasterPrecisionDocument, command: PrecisionPixelCommand,
         input: PrecisionScalarBuffer? = nil, ownedWorker: Bool = false) throws {
        assert(!ownedWorker || !Thread.isMainThread)
        guard ownedWorker || !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
        }
        let context = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
                                            otherRetained: context.otherRetained)
        let count = try PrecisionDocumentDescriptor.integer(plan, "input_samples")
        guard count == UInt64(input?.samples.count ?? 0), plan["output_samples"] == nil else {
            throw DoctoredCoreError(message: "High-depth input samples do not match the planned edit.")
        }
        if let input {
            guard try PrecisionDocumentDescriptor.integer(plan, "input_width") == UInt64(input.width),
                  try PrecisionDocumentDescriptor.integer(plan, "input_height") == UInt64(input.height),
                  try PrecisionDocumentDescriptor.integer(plan, "input_planes") == UInt64(input.planeCount) else {
                throw DoctoredCoreError(message: "High-depth scalar layout does not match the planned edit extent.")
            }
        }
        self.source = source
        self.command = command
        self.input = input
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        retainedPayload = try PrecisionEditPayload.admitted(bytes: UInt64(command.request.utf8.count) * 2 + 512)
    }

    func compute(control: RasterJobControl) throws -> RasterPrecisionDocument? {
        let context = try PrecisionWorkContext(source: source)
        return try source.editingPixels(request: command.request, samples: input?.samples ?? [],
            workingLimit: context.limit, otherRetained: context.otherRetained, control: control)
    }

    func run(title: String = "Processing Captured High-Depth Pixels") throws -> RasterPrecisionDocument? {
        try PrecisionCapturedWork.run(scratchEstimate: scratchEstimate, title: title) {
            try compute(control: $0)
        }
    }
}

struct PrecisionPlaneRead {
    let source: RasterPrecisionDocument
    let command: PrecisionPixelCommand
    let width: Int
    let height: Int
    let scratchEstimate: UInt64
    let retainedPayload: PrecisionEditPayload

    init(_ source: RasterPrecisionDocument, target: String, index: Int, canvas: Bool = false) throws {
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Wait for the current background operation before reading a high-depth plane.")
        }
        self.source = source
        command = try .readPlane(target: target, index: index, canvas: canvas)
        let context = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
                                            otherRetained: context.otherRetained)
        let w = try PrecisionDocumentDescriptor.integer(plan, "width")
        let h = try PrecisionDocumentDescriptor.integer(plan, "height")
        let count = try PrecisionDocumentDescriptor.integer(plan, "output_samples")
        guard w > 0, h > 0, w <= UInt64(RasterImage.maxResizePixels) / h, count == w * h else {
            throw DoctoredCoreError(message: "High-depth plane plan returned an invalid extent.")
        }
        width = Int(w)
        height = Int(h)
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        retainedPayload = try PrecisionEditPayload.admitted(bytes: UInt64(command.request.utf8.count) * 2 + 512)
    }

    func run() throws -> PrecisionScalarBuffer {
        try PrecisionCapturedWork.run(scratchEstimate: scratchEstimate, title: "Reading Captured High-Depth Plane") { control in
            let context = try PrecisionWorkContext(source: source)
            let samples = try source.readingPlane(request: command.request, workingLimit: context.limit,
                                                 otherRetained: context.otherRetained, control: control)
            return try PrecisionScalarBuffer(samples: samples, width: width, height: height)
        }
    }
}
