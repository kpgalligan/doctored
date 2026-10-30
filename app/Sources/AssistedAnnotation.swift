import AppKit

// This model file owns the workspace/case hashes, the clock, the 20-gesture budget and the one
// replay file (`AssistedReplayWriter`, temp + rename, refusing an existing file).
//
// Blinding is enforced, not just documented: a human session refuses every MCP stroke and MCP
// finalize, and a synthetic session records `annotator_id: null` and `scoreable: false`.

/// Phase 9I assisted edge annotation: the blinded, budgeted labelling session
/// behind `DR_ASSISTED_ANNOTATION`.
///
/// This is an INTERNAL EXPERIMENT, not a product feature. Nothing here is an
/// `ImageDocument`, an `EditorTool` or an undoable edit: the window owns its
/// own `RasterImage`, the only output is one protocol-valid replay JSON per
/// case, and no candidate, score or reference plane ever reaches the app. The
/// core (`assisted_replay.rs`) owns the schema and the rasterization; this
/// side owns the clock, the gesture budget, the hashes and the file.
enum AssistedAnnotation {
    static let enabledDefaultsKey = "AssistedAnnotationEnabled"

    /// Off unless the preference is set for this launch, or the environment
    /// asks for it — the verification isolation the app rules require.
    static var isEnabled: Bool {
        UserDefaults.standard.bool(forKey: enabledDefaultsKey)
            || ProcessInfo.processInfo.environment["DR_ASSISTED_ANNOTATION"] == "1"
    }

    static let protocolID = "phase9i-assisted-edges-v1"
    static let budgetSeconds = 300
    static let budgetMS: UInt64 = 300_000
    static let budgetGestures = 20
    static let diameters = [3, 9, 27, 81]
    static let maxStrokePoints = 256
    /// The core's marker for "the shared stroke rasterizer refused this work",
    /// which is a counted rejection rather than an input-contract error.
    static let rasterizerLimitPrefix = "rasterizer_limit: "
    static let labelPlaneEncoding = "trimap_u8_0_128_255"
    static let rasterizerIdentity = "rz_trimap_strokes/v1"

    static let disabledMessage =
        "Assisted annotation is an internal Phase 9I experiment and is off in this build. "
        + "Launch with DR_ASSISTED_ANNOTATION=1 to enable it."

    static var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "unknown"
    }

    static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }

    static func timestamp() -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter.string(from: Date())
    }
}

// MARK: - Workspace and case model

/// One case entry, byte-identical in `manifest.json` and the case's own
/// `case.json`. The hashes are what the exporter measured; the app refuses a
/// case whose files no longer produce them.
struct AssistedCase: Codable {
    let anonymousID: String
    let width: Int
    let height: Int
    let profile: String
    let sourcePNGSHA256: String
    let sourcePlaneSHA256: String
    let priorPNGSHA256: String
    let priorPlaneSHA256: String

    enum CodingKeys: String, CodingKey {
        case anonymousID = "anonymous_id"
        case width, height, profile
        case sourcePNGSHA256 = "source_png_sha256"
        case sourcePlaneSHA256 = "source_plane_sha256"
        case priorPNGSHA256 = "prior_png_sha256"
        case priorPlaneSHA256 = "prior_plane_sha256"
    }
}

private struct AssistedBudgetRecord: Codable {
    let seconds: Int
    let modifyingGestures: Int
    let brushDiameters: [Int]
    let maxStrokePoints: Int

    enum CodingKeys: String, CodingKey {
        case seconds
        case modifyingGestures = "modifying_gestures"
        case brushDiameters = "brush_diameters"
        case maxStrokePoints = "max_stroke_points"
    }
}

private struct AssistedManifest: Codable {
    let workspaceSchemaVersion: Int
    let protocolID: String
    let workspaceID: String
    let annotatorID: String
    let taskCard: String
    let taskCardSHA256: String
    let budget: AssistedBudgetRecord
    let cases: [AssistedCase]

    enum CodingKeys: String, CodingKey {
        case workspaceSchemaVersion = "workspace_schema_version"
        case protocolID = "protocol_id"
        case workspaceID = "workspace_id"
        case annotatorID = "annotator_id"
        case taskCard = "task_card"
        case taskCardSHA256 = "task_card_sha256"
        case budget, cases
    }
}

/// A prepared workspace directory: the manifest, the case list and the
/// `replays/` folder whose files are the per-case lock.
struct AssistedWorkspace {
    let root: URL
    let id: String
    let annotatorID: String
    let taskCardSHA256: String
    let cases: [AssistedCase]
    private let taskCardName: String

    init(root: URL) throws {
        let manifestURL = root.appendingPathComponent("manifest.json")
        let data: Data
        do {
            data = try Data(contentsOf: manifestURL)
        } catch {
            throw AssistedAnnotation.failure(
                "No assisted annotation manifest at \(manifestURL.path).")
        }
        let manifest = try JSONDecoder().decode(AssistedManifest.self, from: data)
        guard manifest.workspaceSchemaVersion == 1,
              manifest.protocolID == AssistedAnnotation.protocolID
        else {
            throw AssistedAnnotation.failure("This workspace declares another protocol or schema.")
        }
        guard manifest.budget.seconds == AssistedAnnotation.budgetSeconds,
              manifest.budget.modifyingGestures == AssistedAnnotation.budgetGestures,
              manifest.budget.brushDiameters == AssistedAnnotation.diameters,
              manifest.budget.maxStrokePoints == AssistedAnnotation.maxStrokePoints
        else {
            throw AssistedAnnotation.failure("This workspace declares another budget.")
        }
        guard !manifest.cases.isEmpty else {
            throw AssistedAnnotation.failure("This workspace declares no cases.")
        }
        self.root = root
        self.id = manifest.workspaceID
        self.annotatorID = manifest.annotatorID
        self.taskCardSHA256 = manifest.taskCardSHA256
        self.taskCardName = manifest.taskCard
        self.cases = manifest.cases
    }

    /// The card exactly as the manifest hashed it; a changed card refuses.
    func taskCardText() throws -> String {
        let url = root.appendingPathComponent(taskCardName)
        let data = try Data(contentsOf: url)
        guard HostSourceError.sha256(data) == taskCardSHA256 else {
            throw AssistedAnnotation.failure("The task card does not match the manifest hash.")
        }
        guard let text = String(data: data, encoding: .utf8) else {
            throw AssistedAnnotation.failure("The task card is not UTF-8 text.")
        }
        return text
    }

    func replayURL(for caseID: String) -> URL {
        root.appendingPathComponent("replays").appendingPathComponent("\(caseID).json")
    }

    /// One attempt per case: the replay file's existence IS the lock.
    func isLocked(_ caseID: String) -> Bool {
        FileManager.default.fileExists(atPath: replayURL(for: caseID).path)
    }

    func nextPendingCase() -> AssistedCase? {
        cases.first { !isLocked($0.anonymousID) }
    }

    func caseNamed(_ caseID: String) throws -> AssistedCase {
        guard let match = cases.first(where: { $0.anonymousID == caseID }) else {
            throw AssistedAnnotation.failure("No case \(caseID) in this workspace.")
        }
        return match
    }
}

/// The decoded case: the source the annotator sees, its RGB plane and the
/// starting mask. Every displayed byte is checked against the manifest BEFORE
/// the clock can start, so a session cannot record hashes it did not show.
struct AssistedLoadedCase {
    let annotationCase: AssistedCase
    let source: RasterImage
    let sourceImage: CGImage
    let prior: [UInt8]

    init(workspace: AssistedWorkspace, case annotationCase: AssistedCase) throws {
        let directory = workspace.root.appendingPathComponent("cases")
            .appendingPathComponent(annotationCase.anonymousID)
        let sourceURL = directory.appendingPathComponent("source.png")
        let priorURL = directory.appendingPathComponent("prior.png")
        let sourceData = try Data(contentsOf: sourceURL)
        let priorData = try Data(contentsOf: priorURL)
        guard HostSourceError.sha256(sourceData) == annotationCase.sourcePNGSHA256,
              HostSourceError.sha256(priorData) == annotationCase.priorPNGSHA256
        else {
            throw AssistedAnnotation.failure("A case file does not match its recorded hash.")
        }
        let source = try RasterImage.open(url: sourceURL)
        let priorImage = try RasterImage.open(url: priorURL)
        // An exported workspace never carries a profile (the exporter writes
        // IHDR/IDAT/IEND only), so an open that converted from or ignored one
        // is a case file this experiment did not write.
        if let refusal = RasterDocument.flatOpenProfileRefusal(at: sourceURL)
            ?? RasterDocument.flatOpenProfileRefusal(at: priorURL) {
            throw AssistedAnnotation.failure("A case file carries an embedded colour profile, "
                + "which an exported workspace never does. " + refusal)
        }
        guard source.width == annotationCase.width, source.height == annotationCase.height,
              priorImage.width == annotationCase.width,
              priorImage.height == annotationCase.height
        else {
            throw AssistedAnnotation.failure("A case file does not match its recorded size.")
        }
        let count = annotationCase.width * annotationCase.height
        guard let rgb = source.withPlateBytes({ Self.channels($0, count: count, stride: 3) }),
              let prior = priorImage.withPlateBytes({ Self.channels($0, count: count, stride: 1) })
        else {
            throw AssistedAnnotation.failure("A case file could not be read as pixels.")
        }
        guard HostSourceError.sha256(Data(rgb)) == annotationCase.sourcePlaneSHA256,
              HostSourceError.sha256(Data(prior)) == annotationCase.priorPlaneSHA256
        else {
            throw AssistedAnnotation.failure("A case plane does not match its recorded hash.")
        }
        guard let image = source.makeCGImage(in: ColorProfile.sRGB) else {
            throw AssistedAnnotation.failure("The case source could not be prepared for display.")
        }
        self.annotationCase = annotationCase
        self.source = source
        self.sourceImage = image
        self.prior = prior
    }

    /// The first `stride` channels of each straight RGBA8 pixel: RGB for the
    /// source, the single gray sample for the prior.
    private static func channels(
        _ pixels: UnsafePointer<UInt8>, count: Int, stride: Int
    ) -> [UInt8] {
        var out = [UInt8]()
        out.reserveCapacity(count * stride)
        for index in 0..<count {
            for channel in 0..<stride { out.append(pixels[index * 4 + channel]) }
        }
        return out
    }
}

// MARK: - Event value types

enum AssistedLabel: String, Codable {
    case foreground, background, unknown

    var plane: UInt8 {
        switch self {
        case .foreground: return 255
        case .background: return 0
        case .unknown: return 128
        }
    }
}

enum AssistedOrigin: String, Codable {
    case human, synthetic
}

enum AssistedEventKind: String, Codable {
    case stroke, undo, redo, view, submit
    case budgetExhausted = "budget_exhausted"
    case abandon

    var isModifying: Bool { self == .stroke || self == .undo || self == .redo }
}

enum AssistedOutcome: String, Codable {
    case submitted
    case budgetExhausted = "budget_exhausted"
    case incomplete
}

enum AssistedRejection: String {
    case timeBudget = "time_budget"
    case rasterizerLimit = "rasterizer_limit"

    var result: String { "rejected:\(rawValue)" }
}

struct AssistedAttestation: Codable {
    let sourceOnly: Bool
    let text: String
    let attestedAtUTC: String

    enum CodingKeys: String, CodingKey {
        case sourceOnly = "source_only"
        case text
        case attestedAtUTC = "attested_at_utc"
    }
}

/// Every optional field is written as an explicit null, so the document on
/// disk is the schema table rather than whichever keys happened to be set.
extension KeyedEncodingContainer {
    fileprivate mutating func encodeAlways<T: Encodable>(_ value: T?, forKey key: K) throws {
        if let value { try encode(value, forKey: key) } else { try encodeNil(forKey: key) }
    }
}

struct AssistedEvent: Codable {
    let index: Int
    let gestureIndex: Int?
    let kind: AssistedEventKind
    let origin: AssistedOrigin
    let tool: AssistedLabel?
    let diameter: Int?
    let points: [[Int]]?
    let elapsedMS: UInt64
    let result: String
    let detail: String?
    var labelPlaneSHA256: String

    enum CodingKeys: String, CodingKey {
        case index
        case gestureIndex = "gesture_index"
        case kind, origin, tool, diameter, points
        case elapsedMS = "elapsed_ms"
        case result, detail
        case labelPlaneSHA256 = "label_plane_sha256"
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(index, forKey: .index)
        try container.encodeAlways(gestureIndex, forKey: .gestureIndex)
        try container.encode(kind, forKey: .kind)
        try container.encode(origin, forKey: .origin)
        try container.encodeAlways(tool, forKey: .tool)
        try container.encodeAlways(diameter, forKey: .diameter)
        try container.encodeAlways(points, forKey: .points)
        try container.encode(elapsedMS, forKey: .elapsedMS)
        try container.encode(result, forKey: .result)
        try container.encodeAlways(detail, forKey: .detail)
        try container.encode(labelPlaneSHA256, forKey: .labelPlaneSHA256)
    }

    var reportFields: [String: Any] {
        [
            "index": index, "gesture_index": gestureIndex ?? NSNull(), "kind": kind.rawValue,
            "origin": origin.rawValue, "tool": tool?.rawValue ?? NSNull(),
            "diameter": diameter ?? NSNull(), "elapsed_ms": elapsedMS, "result": result,
            "detail": detail ?? NSNull(), "label_plane_sha256": labelPlaneSHA256,
            "points": points?.count ?? NSNull(),
        ]
    }
}

private struct AssistedDisplayedAssets: Encodable {
    let sourcePNGSHA256: String
    let sourcePlaneSHA256: String
    let priorPNGSHA256: String
    let priorPlaneSHA256: String
    let taskCardSHA256: String

    enum CodingKeys: String, CodingKey {
        case sourcePNGSHA256 = "source_png_sha256"
        case sourcePlaneSHA256 = "source_plane_sha256"
        case priorPNGSHA256 = "prior_png_sha256"
        case priorPlaneSHA256 = "prior_plane_sha256"
        case taskCardSHA256 = "task_card_sha256"
    }
}

private struct AssistedLabelPlaneRecord: Encodable {
    let encoding: String
    let sha256: String
}

/// Labels-only collection: there is no protect or region tool, so the write
/// and protection planes are constants and only their digests are recorded.
private struct AssistedWritePlanes: Encodable {
    let alphaWrite: String
    let rgbWrite: String
    let protection: String
    let alphaWriteSHA256: String
    let rgbWriteSHA256: String
    let protectionSHA256: String

    enum CodingKeys: String, CodingKey {
        case alphaWrite = "alpha_write"
        case rgbWrite = "rgb_write"
        case protection
        case alphaWriteSHA256 = "alpha_write_sha256"
        case rgbWriteSHA256 = "rgb_write_sha256"
        case protectionSHA256 = "protection_sha256"
    }
}

private struct AssistedProgram: Encodable {
    let appVersion: String
    let rasterizer: String

    enum CodingKeys: String, CodingKey {
        case appVersion = "app_version"
        case rasterizer
    }
}

private struct AssistedReplayDocument: Encodable {
    let workspaceID: String
    let sessionID: String
    let caseID: String
    let width: Int
    let height: Int
    let origin: AssistedOrigin
    let annotatorID: String?
    let reportsPriorExposure: Bool?
    let attestation: AssistedAttestation?
    let assets: AssistedDisplayedAssets
    let startedAtUTC: String
    let endedAtUTC: String?
    let elapsedMS: UInt64
    let modifyingGestures: Int
    let outcome: AssistedOutcome?
    let events: [AssistedEvent]
    let labelPlane: AssistedLabelPlaneRecord
    let writePlanes: AssistedWritePlanes

    enum CodingKeys: String, CodingKey {
        case replaySchemaVersion = "replay_schema_version"
        case protocolID = "protocol_id"
        case workspaceID = "workspace_id"
        case sessionID = "session_id"
        case caseID = "case_id"
        case width, height, origin
        case humanOrigin = "human_origin"
        case scoreable
        case annotatorID = "annotator_id"
        case reportsPriorExposure = "annotator_reports_prior_exposure"
        case attestation = "blinding_attestation"
        case assets = "displayed_assets"
        case budget
        case startedAtUTC = "started_at_utc"
        case endedAtUTC = "ended_at_utc"
        case elapsedMS = "elapsed_ms"
        case modifyingGestures = "modifying_gestures"
        case outcome, events
        case labelPlane = "label_plane"
        case writePlanes = "write_planes"
        case program
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(1, forKey: .replaySchemaVersion)
        try container.encode(AssistedAnnotation.protocolID, forKey: .protocolID)
        try container.encode(workspaceID, forKey: .workspaceID)
        try container.encode(sessionID, forKey: .sessionID)
        try container.encode(caseID, forKey: .caseID)
        try container.encode(width, forKey: .width)
        try container.encode(height, forKey: .height)
        try container.encode(origin, forKey: .origin)
        try container.encode(origin == .human, forKey: .humanOrigin)
        try container.encode(origin == .human, forKey: .scoreable)
        try container.encodeAlways(annotatorID, forKey: .annotatorID)
        try container.encodeAlways(reportsPriorExposure, forKey: .reportsPriorExposure)
        try container.encodeAlways(attestation, forKey: .attestation)
        try container.encode(assets, forKey: .assets)
        try container.encode(
            AssistedBudgetRecord(
                seconds: AssistedAnnotation.budgetSeconds,
                modifyingGestures: AssistedAnnotation.budgetGestures,
                brushDiameters: AssistedAnnotation.diameters,
                maxStrokePoints: AssistedAnnotation.maxStrokePoints),
            forKey: .budget)
        try container.encode(startedAtUTC, forKey: .startedAtUTC)
        try container.encodeAlways(endedAtUTC, forKey: .endedAtUTC)
        try container.encode(elapsedMS, forKey: .elapsedMS)
        try container.encode(modifyingGestures, forKey: .modifyingGestures)
        try container.encodeAlways(outcome, forKey: .outcome)
        try container.encode(events, forKey: .events)
        try container.encode(labelPlane, forKey: .labelPlane)
        try container.encode(writePlanes, forKey: .writePlanes)
        try container.encode(
            AssistedProgram(
                appVersion: AssistedAnnotation.appVersion,
                rasterizer: AssistedAnnotation.rasterizerIdentity),
            forKey: .program)
    }
}

// MARK: - The session

/// One case's whole session: the clock, the gesture budget, the recorded
/// events and the current label plane. It never touches AppKit state beyond a
/// refusal beep, and it writes no file — `onFinished` hands the finished
/// replay to the window controller, which is the only writer.
final class AssistedAnnotationSession {
    enum State {
        case awaitingStart
        case active
        case finished(AssistedOutcome)
    }

    let workspace: AssistedWorkspace
    let loaded: AssistedLoadedCase
    let origin: AssistedOrigin

    private(set) var state: State = .awaitingStart
    private(set) var events: [AssistedEvent] = []
    private(set) var modifyingGestures = 0
    private(set) var labelPlane: [UInt8]
    private(set) var labelPlaneSHA256: String

    var onFinished: ((AssistedOutcome) -> Void)?

    private let sessionID = UUID().uuidString
    private let constantWriteSHA256: String
    private let constantProtectionSHA256: String
    private var startUptime: TimeInterval?
    private var frozenElapsedMS: UInt64 = 0
    private var startedAtUTC = ""
    private var endedAtUTC: String?
    private var annotatorID: String?
    private var reportsPriorExposure: Bool?
    private var attestation: AssistedAttestation?
    private var activeStack: [Int] = []
    private var redoStack: [Int] = []

    init(workspace: AssistedWorkspace, loaded: AssistedLoadedCase, origin: AssistedOrigin) {
        self.workspace = workspace
        self.loaded = loaded
        self.origin = origin
        let count = loaded.annotationCase.width * loaded.annotationCase.height
        let plane = [UInt8](repeating: 128, count: count)
        self.labelPlane = plane
        self.labelPlaneSHA256 = HostSourceError.sha256(Data(plane))
        // The labels-only write planes are constants: alpha and RGB writes are
        // 65535 everywhere and protection 0 everywhere, little-endian u16.
        self.constantWriteSHA256 = HostSourceError.sha256(
            Data([UInt8](repeating: 255, count: count * 2)))
        self.constantProtectionSHA256 = HostSourceError.sha256(
            Data([UInt8](repeating: 0, count: count * 2)))
    }

    var caseID: String { loaded.annotationCase.anonymousID }
    var width: Int { loaded.annotationCase.width }
    var height: Int { loaded.annotationCase.height }
    var activeStrokes: Int { activeStack.count }
    var remainingGestures: Int { max(0, AssistedAnnotation.budgetGestures - modifyingGestures) }
    var undoAvailable: Bool { isActive && !activeStack.isEmpty }
    var redoAvailable: Bool { isActive && !redoStack.isEmpty }
    var isOverTime: Bool { elapsedMS > AssistedAnnotation.budgetMS }

    var isActive: Bool {
        if case .active = state { return true }
        return false
    }

    var stateName: String {
        switch state {
        case .awaitingStart: return "awaiting_start"
        case .active: return "active"
        case .finished(let outcome): return outcome.rawValue
        }
    }

    /// Monotonic wall clock from the moment `begin()` ran; frozen at finish.
    var elapsedMS: UInt64 {
        guard let start = startUptime else { return frozenElapsedMS }
        let seconds = max(0, ProcessInfo.processInfo.systemUptime - start)
        return UInt64((seconds * 1000).rounded())
    }

    /// The clock starts HERE, after decoding and hash checking, and the window
    /// only then shows the source. The task-card sheet's Begin (human) or
    /// `assisted_open_case` (synthetic) is the single caller.
    func begin(
        annotatorID: String?, reportsPriorExposure: Bool?, attestation: AssistedAttestation?
    ) throws {
        guard case .awaitingStart = state else {
            throw AssistedAnnotation.failure("This session has already started.")
        }
        switch origin {
        case .human:
            guard let identifier = annotatorID, !identifier.isEmpty else {
                throw AssistedAnnotation.failure("A human session needs an annotator id.")
            }
            guard let declaration = attestation, declaration.sourceOnly else {
                throw AssistedAnnotation.failure("A human session needs a source-only attestation.")
            }
        case .synthetic:
            guard annotatorID == nil else {
                throw AssistedAnnotation.failure("A synthetic session records no annotator id.")
            }
        }
        self.annotatorID = annotatorID
        self.reportsPriorExposure = reportsPriorExposure
        self.attestation = attestation
        startedAtUTC = AssistedAnnotation.timestamp()
        startUptime = ProcessInfo.processInfo.systemUptime
        state = .active
    }

    /// One brush gesture. Returns whether it was applied; a refusal that was
    /// still a gesture (over time, or past the rasterizer's work cap) returns
    /// false after recording it. Only an input-contract error throws.
    @discardableResult
    func stroke(tool: AssistedLabel, diameter: Int, rawPoints: [[Int]]) throws -> Bool {
        guard isActive else {
            throw AssistedAnnotation.failure("No annotation session is accepting gestures.")
        }
        guard AssistedAnnotation.diameters.contains(diameter) else {
            throw AssistedAnnotation.failure(
                "Brush diameter must be one of \(AssistedAnnotation.diameters).")
        }
        let points = Self.decimate(cleaned(rawPoints), max: AssistedAnnotation.maxStrokePoints)
        guard !points.isEmpty else {
            throw AssistedAnnotation.failure("A stroke needs at least one canvas point.")
        }
        if isOverTime {
            record(
                kind: .stroke, tool: tool, diameter: diameter, points: points,
                result: AssistedRejection.timeBudget.result, hash: labelPlaneSHA256)
            try finish(.budgetExhausted)
            return false
        }
        let candidate = makeEvent(
            kind: .stroke, gesture: modifyingGestures + 1, tool: tool, diameter: diameter,
            points: points, result: "accepted", detail: nil, hash: labelPlaneSHA256)
        let plane: [UInt8]
        do {
            plane = try RasterImage.assistedReplayPlane(
                try replayText(appending: [candidate], outcome: nil),
                width: width, height: height)
        } catch let error as DoctoredCoreError
            where error.message.hasPrefix(AssistedAnnotation.rasterizerLimitPrefix) {
            record(
                kind: .stroke, tool: tool, diameter: diameter, points: points,
                result: AssistedRejection.rasterizerLimit.result, hash: labelPlaneSHA256,
                detail: error.message)
            try finishIfGesturesSpent()
            return false
        }
        labelPlane = plane
        labelPlaneSHA256 = HostSourceError.sha256(Data(plane))
        var applied = candidate
        applied.labelPlaneSHA256 = labelPlaneSHA256
        events.append(applied)
        modifyingGestures += 1
        activeStack.append(events.count - 1)
        redoStack.removeAll()
        try finishIfGesturesSpent()
        return true
    }

    func undo() -> Bool { stackGesture(.undo) }

    func redo() -> Bool { stackGesture(.redo) }

    /// Zoom, pan and overlay toggles cost time but never a gesture.
    func view(_ detail: String) {
        guard isActive else { return }
        record(kind: .view, result: "accepted", hash: labelPlaneSHA256, detail: detail)
    }

    /// The one-second fallback: an active session past its time budget ends
    /// here even if nothing else was clicked. True when this call finished it.
    func tick() -> Bool {
        guard isActive, isOverTime else { return false }
        do {
            try finish(.budgetExhausted)
        } catch {
            NSSound.beep()
        }
        return true
    }

    func submit() throws {
        guard isActive else {
            throw AssistedAnnotation.failure("No annotation session is open.")
        }
        record(kind: .submit, result: "accepted", hash: labelPlaneSHA256)
        try finish(.submitted)
    }

    func abandon() throws {
        guard isActive else {
            throw AssistedAnnotation.failure("No annotation session is open.")
        }
        record(kind: .abandon, result: "accepted", hash: labelPlaneSHA256)
        try finish(.incomplete)
    }

    func replayJSON() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(document(appending: [], outcome: outcomeIfFinished))
    }

    func stateJSON() -> [String: Any] {
        [
            "case_id": caseID, "origin": origin.rawValue, "state": stateName,
            "elapsed_ms": elapsedMS,
            "remaining_ms": AssistedAnnotation.budgetMS > elapsedMS
                ? AssistedAnnotation.budgetMS - elapsedMS : 0,
            "modifying_gestures": modifyingGestures, "remaining_gestures": remainingGestures,
            "active_strokes": activeStrokes, "label_plane_sha256": labelPlaneSHA256,
            "width": width, "height": height,
            "events": events.map(\.reportFields),
        ]
    }

    /// First and last sample kept, the rest evenly spaced: the recorded points
    /// are exactly what is rasterized and shown, never a resampled curve.
    static func decimate(_ points: [[Int]], max: Int) -> [[Int]] {
        guard max >= 2, points.count > max else { return points }
        let last = Double(points.count - 1)
        return (0..<max).map { step in
            points[Int((Double(step) * last / Double(max - 1)).rounded())]
        }
    }

    // MARK: - Internals

    private var outcomeIfFinished: AssistedOutcome? {
        if case .finished(let outcome) = state { return outcome }
        return nil
    }

    /// Integer canvas pixels, clamped, with consecutive duplicates dropped.
    private func cleaned(_ raw: [[Int]]) -> [[Int]] {
        var out: [[Int]] = []
        for point in raw where point.count == 2 {
            let x = min(max(point[0], 0), width - 1)
            let y = min(max(point[1], 0), height - 1)
            if let last = out.last, last[0] == x, last[1] == y { continue }
            out.append([x, y])
        }
        return out
    }

    private func makeEvent(
        kind: AssistedEventKind, gesture: Int?, tool: AssistedLabel?, diameter: Int?,
        points: [[Int]]?, result: String, detail: String?, hash: String
    ) -> AssistedEvent {
        AssistedEvent(
            index: events.count, gestureIndex: gesture, kind: kind, origin: origin, tool: tool,
            diameter: diameter, points: points, elapsedMS: elapsedMS, result: result,
            detail: detail, labelPlaneSHA256: hash)
    }

    private func record(
        kind: AssistedEventKind, tool: AssistedLabel? = nil, diameter: Int? = nil,
        points: [[Int]]? = nil, result: String, hash: String, detail: String? = nil
    ) {
        let gesture = kind.isModifying ? modifyingGestures + 1 : nil
        events.append(
            makeEvent(
                kind: kind, gesture: gesture, tool: tool, diameter: diameter, points: points,
                result: result, detail: detail, hash: hash))
        if kind.isModifying { modifyingGestures += 1 }
    }

    /// Undo and Redo replay the whole document through the same core call, so
    /// the plane after a stack move is the one the core would reconstruct.
    private func stackGesture(_ kind: AssistedEventKind) -> Bool {
        guard isActive else {
            NSSound.beep()
            return false
        }
        if isOverTime {
            NSSound.beep()
            do { try finish(.budgetExhausted) } catch { NSSound.beep() }
            return false
        }
        guard kind == .undo ? !activeStack.isEmpty : !redoStack.isEmpty else {
            NSSound.beep()
            return false
        }
        let candidate = makeEvent(
            kind: kind, gesture: modifyingGestures + 1, tool: nil, diameter: nil, points: nil,
            result: "accepted", detail: nil, hash: labelPlaneSHA256)
        let plane: [UInt8]
        do {
            plane = try RasterImage.assistedReplayPlane(
                try replayText(appending: [candidate], outcome: nil),
                width: width, height: height)
        } catch {
            NSSound.beep()
            return false
        }
        labelPlane = plane
        labelPlaneSHA256 = HostSourceError.sha256(Data(plane))
        var applied = candidate
        applied.labelPlaneSHA256 = labelPlaneSHA256
        events.append(applied)
        modifyingGestures += 1
        if kind == .undo {
            if let moved = activeStack.popLast() { redoStack.append(moved) }
        } else if let moved = redoStack.popLast() {
            activeStack.append(moved)
        }
        do { try finishIfGesturesSpent() } catch { NSSound.beep() }
        return true
    }

    private func finishIfGesturesSpent() throws {
        guard modifyingGestures >= AssistedAnnotation.budgetGestures, isActive else { return }
        try finish(.budgetExhausted)
    }

    private func finish(_ outcome: AssistedOutcome) throws {
        guard isActive else { return }
        frozenElapsedMS = elapsedMS
        startUptime = nil
        endedAtUTC = AssistedAnnotation.timestamp()
        state = .finished(outcome)
        onFinished?(outcome)
    }

    private func replayText(appending extra: [AssistedEvent], outcome: AssistedOutcome?) throws
        -> String
    {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let data = try encoder.encode(document(appending: extra, outcome: outcome))
        return String(decoding: data, as: UTF8.self)
    }

    private func document(appending extra: [AssistedEvent], outcome: AssistedOutcome?)
        -> AssistedReplayDocument
    {
        let all = events + extra
        let gestures = all.filter { $0.kind.isModifying }.count
        let annotationCase = loaded.annotationCase
        return AssistedReplayDocument(
            workspaceID: workspace.id, sessionID: sessionID, caseID: caseID,
            width: width, height: height, origin: origin, annotatorID: annotatorID,
            reportsPriorExposure: reportsPriorExposure, attestation: attestation,
            assets: AssistedDisplayedAssets(
                sourcePNGSHA256: annotationCase.sourcePNGSHA256,
                sourcePlaneSHA256: annotationCase.sourcePlaneSHA256,
                priorPNGSHA256: annotationCase.priorPNGSHA256,
                priorPlaneSHA256: annotationCase.priorPlaneSHA256,
                taskCardSHA256: workspace.taskCardSHA256),
            startedAtUTC: startedAtUTC, endedAtUTC: endedAtUTC, elapsedMS: elapsedMS,
            modifyingGestures: gestures, outcome: outcome, events: all,
            labelPlane: AssistedLabelPlaneRecord(
                encoding: AssistedAnnotation.labelPlaneEncoding, sha256: labelPlaneSHA256),
            writePlanes: AssistedWritePlanes(
                alphaWrite: "constant_65535", rgbWrite: "constant_65535", protection: "constant_0",
                alphaWriteSHA256: constantWriteSHA256, rgbWriteSHA256: constantWriteSHA256,
                protectionSHA256: constantProtectionSHA256))
    }
}

// MARK: - The only writer

/// One attempt per case: a replay is written once, atomically, and an existing
/// file is a refusal rather than an overwrite.
enum AssistedReplayWriter {
    static func write(_ data: Data, to url: URL) throws {
        let directory = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard !FileManager.default.fileExists(atPath: url.path) else {
            throw AssistedAnnotation.failure("This case already has a recorded replay.")
        }
        let temporary = directory.appendingPathComponent(".\(UUID().uuidString).json")
        try data.write(to: temporary, options: .atomic)
        do {
            try FileManager.default.moveItem(at: temporary, to: url)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            throw error
        }
        DurableFile.persistDirectory(directory)
    }
}
