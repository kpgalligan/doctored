// The RAW decode harness: the phys_footprint of RAW decode stages,
// one case per process. Built by measure.py with the app sources (minus app
// main.swift), -O, whole module, against the release core; see README.md.
//
//   probe calibrate
//   probe <open|warm|cold|prep|preview|resident> <file> <16|8>   (cold takes a .rz)
//   probe [baseline] open <raw> <16|8> OP / preview <raw> 16 VP   (with Presence)
//   probe [baseline] open <raw> <16|8> OL / preview <raw> 16 VL   (v1-lensfun: Lensfun stage on)
//   probe [baseline] open <raw> 32 OG / preview <raw> 32 VG        (with grain)
//   probe [baseline] grain <raw> 32                                (applyingRawGrain alone, GR)
//   probe [baseline] frame <raw> 32 <F|D>                          (a viewport-only HDR frame, VF)
//   probe baseline <open|warm|cold|prep|preview> <file> <16|8>
//   probe save <dng> <16|8> <out.rz>
//   probe [baseline] restore <raw|.rz> <16|8> [none|L4]           (phase 2, RS)
//   probe [baseline] local <raw|.rz> <16|8> <L0|L1|L1B|L1C|L4|FX|PX|LS|LP|LT1|LT16|LTS|LTD>  (L)
//   probe [baseline] publish <raw> <16|8> <L0|L1>                 (PB)
//   probe menu <raw> <16|8> <M0|M1|M11|MC>                        (no E)
//
// Prints one JSON line. E = ri_interval_max_phys_footprint − pre, where pre
// is ri_phys_footprint read right after proc_reset_footprint_interval.

import AppKit
import Darwin

@_silgen_name("proc_reset_footprint_interval")
func drResetFootprintInterval(_ pid: Int32) -> Int32
@_silgen_name("proc_pid_rusage")
func drPidRusage(_ pid: Int32, _ flavor: Int32, _ buffer: UnsafeMutableRawPointer) -> Int32

func usage() -> rusage_info_v4 {
    var info = rusage_info_v4()
    let status = withUnsafeMutablePointer(to: &info) { drPidRusage(getpid(), RUSAGE_INFO_V4, $0) }
    precondition(status == 0, "proc_pid_rusage failed")
    return info
}

func emit(_ fields: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]),
          let text = String(data: data, encoding: .utf8) else { return }
    print(text)
    fflush(stdout)
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(2)
}

/// Reset the interval, read pre, run the call, read E while its result is alive.
func measure<T>(_ body: () throws -> T) -> (pre: UInt64, e: UInt64, lifetime: UInt64, result: Result<T, Error>) {
    precondition(drResetFootprintInterval(getpid()) == 0, "proc_reset_footprint_interval failed")
    let pre = usage().ri_phys_footprint
    let result = Result { try body() }
    let after = usage()
    let e = after.ri_interval_max_phys_footprint > pre ? after.ri_interval_max_phys_footprint - pre : 0
    return (pre, e, after.ri_lifetime_max_phys_footprint, result)
}

/// Depth 32 is the 32-bit float (linear) development: the open is
/// headless with `output_encoding: linear_float`, so every stage after it
/// (redevelop, the Develop session, locals) runs on a float recipe.
func setDepth(_ depth: Int) {
    var settings = RawDevelopSettings()
    if depth == 32 { settings.outputEncoding = .linearFloat }
    RawImportRequest.mode = .headless(settings)
    PrecisionImportRequest.mode = depth == 8 ? .rgba8 : .preserve
}

func probe(_ url: URL) -> RawProbe {
    guard let probe = RawImage.inspect(url: url) else { fail("RawImage.inspect refused \(url.path)") }
    return probe
}

func redevelopSettings() -> RawDevelopSettings {
    var settings = RawDevelopSettings()
    settings.exposure = 0.5
    return settings
}

/// The grain every grain case develops (FX, OG, VG, GR).
let probeGrain = RawGrainSettings(amount: 50, size: 1.5, roughness: 50)

/// Develop Presence at 50 on all three sliders, so every guided pass runs (OP, VP, PX).
func withPresence(_ settings: RawDevelopSettings) -> RawDevelopSettings {
    var settings = settings
    settings.texture = 50
    settings.clarity = 50
    settings.dehaze = 50
    return settings
}

/// The opened (or reopened) retained RAW document, ready for redevelopment.
struct Retained {
    let source: RasterDocument
    let descriptor: RawSourceDescriptor
    let desired: RawSourceDescriptor
    let width: Int
    let height: Int
    let encoded: UInt64

    init(_ source: RasterDocument) throws {
        self.source = source
        let info = try LinkedImageWork.requireSource(source, layer: 0, editing: true)
        descriptor = try RawSourceDescriptor.read(info)
        var desired = descriptor
        desired.recipe = try descriptor.recipe.updating(redevelopSettings())
        self.desired = desired
        let raw = info["raw"] as? [String: Any] ?? [:]
        encoded = try PrecisionDocumentDescriptor.integer(raw, "encoded_bytes")
        width = Int(try PrecisionDocumentDescriptor.integer(info, "width"))
        height = Int(try PrecisionDocumentDescriptor.integer(info, "height"))
    }

    /// `RawSourceWork.prepare`'s worker body.
    func decode() throws -> (RawSourceSnapshot, (RasterDocument, RawSourceDescriptor)) {
        let snapshot = try RawSourceSnapshot.restore(source, layer: 0, descriptor: descriptor)
        let decoded = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: desired, maximumScratch: .max,
                                                      whiteBalanceChanged: false)
        return (snapshot, decoded)
    }
}

/// A setup or resident step drains its own autorelease pool, as the app's
/// main run loop does after every event; the harness's top level never drains.
func step<T>(_ body: () throws -> T) rethrows -> T { try autoreleasepool(invoking: body) }

/// Core Image and Metal return a finished render's memory asynchronously
/// (within ~250 ms on macOS 26). Every measured call and residency reading
/// waits 1 s first, so a setup step's release cannot land inside the window.
func settle() { usleep(1_000_000) }

func retainedBytes() -> UInt64 {
    guard let report = try? RasterResources.shared.report(),
          let value = report["unique_retained_bytes"] as? NSNumber else { return 0 }
    return value.uint64Value
}

/// Calibration blocks escape through this global, so -O cannot elide the
/// malloc/memset/free pairs.
var calibrationSink: UnsafeMutableRawPointer?

@inline(never) func touched(_ bytes: Int) -> UInt8 {
    guard let block = malloc(bytes) else { fail("malloc \(bytes)") }
    memset(block, 1, bytes)
    calibrationSink = block
    return block.load(fromByteOffset: bytes - 1, as: UInt8.self)
}

@inline(never) func release() {
    free(calibrationSink)
    calibrationSink = nil
}

func calibrate() {
    let gib = 1 << 30
    // One small touch first, so first-call runtime and malloc-zone setup is
    // not read as memory the 1 GiB drop failed to return.
    _ = touched(1 << 20)
    release()
    // Let launch-time background threads (AppKit, dispatch) settle, so their
    // allocations do not land between pre and the drop read.
    usleep(500_000)
    precondition(drResetFootprintInterval(getpid()) == 0)
    let pre = usage().ri_phys_footprint
    var check = Int(touched(gib))
    release()
    let dropped = usage().ri_phys_footprint
    let drop = Int64(bitPattern: dropped) - Int64(bitPattern: pre)
    let quarter = 256 << 20
    let run = measure { touched(quarter) }
    if case .success(let value) = run.result { check += Int(value) }
    release()
    emit(["mode": "calibrate", "pre": pre, "drop_return": drop, "drop_ok": abs(drop) <= 65_536, "check": check,
          "e256": run.e, "e256_ok": run.e >= UInt64(quarter) && run.e <= UInt64(quarter + (1 << 20)),
          "lifetime_peak_minus_pre": usage().ri_lifetime_max_phys_footprint - pre])
}

_ = NSApplication.shared
// The core picks up the process working limit (DR_WORKING_LIMIT_BYTES) here.
_ = RasterResources.shared
BackgroundLaunch.applyActivationPolicy()
AutomationAlerts.installIfEnabled()

let arguments = Array(CommandLine.arguments.dropFirst())
guard let first = arguments.first else { fail("usage: probe <mode> …") }
if first == "calibrate" { calibrate(); exit(0) }
let baseline = first == "baseline"
let rest = baseline ? Array(arguments.dropFirst()) : arguments
guard rest.count >= 3, let depth = Int(rest[2]), depth == 8 || depth == 16 || depth == 32 else {
    fail("usage: probe [baseline] <open|warm|cold|prep|preview|resident|save|restore|local|publish|menu> "
        + "<file> <16|8|32> [out.rz|shape]")
}
/// The float development's cases (`measure.py --float`).
let floatDepth = depth == 32
let mode = rest[0]
let file = URL(fileURLWithPath: rest[1])
setDepth(depth)
let encodedFile = (try? RawSourceSnapshot.size(file)) ?? 0
var fields: [String: Any] = ["mode": mode, "file": file.lastPathComponent, "depth": depth,
                             "baseline": baseline, "file_bytes": encodedFile,
                             "working_limit": RasterResources.shared.workingLimit]

func record<T>(_ run: (pre: UInt64, e: UInt64, lifetime: UInt64, result: Result<T, Error>)) {
    fields["pre"] = run.pre
    fields["e"] = run.e
    fields["lifetime_peak"] = run.lifetime
    if case .failure(let error) = run.result { fields["refused"] = error.localizedDescription }
}

// MARK: - Phase 2: restore, local, publish and menu

let shapeName = rest.count >= 4 ? rest[3] : "none"
fields["shape"] = shapeName

/// Exposure plus what `pendingSettings` sends: the neutral manual lens
/// geometry when no lens is authored (`RawSourceGeometryControls`), or FX's
/// grain, vignette and authored lens; PX is FX plus Presence.
func phase2Settings(exposure: Double, effects: Bool, presence: Bool = false) -> RawDevelopSettings {
    var settings = presence ? withPresence(RawDevelopSettings()) : RawDevelopSettings()
    settings.exposure = exposure
    if effects {
        // Grain applies at the float encoding too, so FX is the same at every depth.
        settings.grain = probeGrain
        settings.vignette = RawVignetteSettings(edgeExposure: -1.25, centerX: 0.31, centerY: 0.67, falloff: 1.7)
        settings.sourceGeometry = RawLensGeometry(profileID: "fx", center: [0.43, 0.59],
            radial: [0.012, -0.001, 0.0001], redScale: 0.985, blueScale: 1.015, filter: .bilinear,
            referenceSize: nil, referenceFromSource: nil)
    } else {
        settings.sourceGeometry = RawLensGeometry(profileID: "manual", center: [0.5, 0.5],
            radial: [0, 0, 0], redScale: 1, blueScale: 1, filter: .bilinear,
            referenceSize: nil, referenceFromSource: nil)
    }
    return settings
}

func radialCreate(id: String, offset: Double, center: [Double], radius: [Double]) -> [String: Any] {
    let mask: [String: Any] = ["op": "radial", "center": center, "radius": radius, "hardness": 0.4]
    return ["op": "create", "id": id, "exposure_offset": offset, "mask": mask]
}

func createL1(_ width: Int, _ height: Int) -> [String: Any] {
    let w = Double(width), h = Double(height)
    return radialCreate(id: "l1", offset: -1.0, center: [w / 2, h / 2], radius: [w / 3, h / 3])
}

func createL4(_ width: Int, _ height: Int) -> [[String: Any]] {
    let w = Double(width), h = Double(height)
    let points: [(Double, Double)] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
    let offsets: [Double] = [-1.0, 0.5, -0.5, 1.0]
    var operations: [[String: Any]] = []
    for index in 0..<4 {
        let center: [Double] = [w * points[index].0, h * points[index].1]
        operations.append(radialCreate(id: "l\(index + 1)", offset: offsets[index], center: center,
                                       radius: [w / 5, h / 5]))
    }
    return operations
}

/// The local tone path: a radial with a tone and no camera control, so no variant.
func toneCreate(id: String, center: [Double], radius: [Double], tone: [String: Any]) -> [String: Any] {
    let mask: [String: Any] = ["op": "radial", "center": center, "radius": radius, "hardness": 0.4]
    return ["op": "create", "id": id, "tone": tone, "mask": mask]
}

/// LT1's pointwise tone: exposure, contrast, whites, blacks and saturation.
let pointwiseTone: [String: Any] = ["exposure": 0.5, "contrast": 40, "whites": 20, "blacks": -20, "saturation": 20]

/// LT1: one centred radial with the pointwise tone.
func createLT1(_ width: Int, _ height: Int) -> [String: Any] {
    let w = Double(width), h = Double(height)
    return toneCreate(id: "l1", center: [w / 2, h / 2], radius: [w / 3, h / 3], tone: pointwiseTone)
}

/// LT16: sixteen such radials on a 4×4 grid.
func createLT16(_ width: Int, _ height: Int) -> [[String: Any]] {
    let w = Double(width), h = Double(height)
    var operations: [[String: Any]] = []
    for row in 0..<4 {
        for column in 0..<4 {
            let center: [Double] = [w * (Double(column) + 0.5) / 4, h * (Double(row) + 0.5) / 4]
            operations.append(toneCreate(id: "l\(row * 4 + column + 1)", center: center,
                                         radius: [w / 6, h / 6], tone: pointwiseTone))
        }
    }
    return operations
}

/// LTS: one centred radial whose tone reads a neighbourhood (the guide and the Presence plane).
func createLTS(_ width: Int, _ height: Int) -> [String: Any] {
    let w = Double(width), h = Double(height)
    return toneCreate(id: "l1", center: [w / 2, h / 2], radius: [w / 3, h / 3],
                      tone: ["highlights": 50, "shadows": 50, "clarity": 50, "texture": 50])
}

/// LTD: LTS's radial and tone plus Sharpness 60 and Noise Reduction 60, so the
/// detail stage runs over the Presence plane.
func createLTD(_ width: Int, _ height: Int) -> [String: Any] {
    let w = Double(width), h = Double(height)
    return toneCreate(id: "l1", center: [w / 2, h / 2], radius: [w / 3, h / 3],
                      tone: ["highlights": 50, "shadows": 50, "clarity": 50, "texture": 50,
                             "sharpness": 60, "noise_reduction": 60])
}

/// The core's evaluated-pixel count for the L1B grid at `radius`: each dab's
/// clipped bounding box, as `raw_local_coverage.rs`'s `bounds` sums it.
func brushWork(_ width: Int, _ height: Int, radius: Double) -> Int {
    var total = 0
    for row in 0..<32 {
        for column in 0..<64 {
            let x = Double((2 * column + 1) * width / 128), y = Double((2 * row + 1) * height / 64)
            let x0 = min(max((x - radius - 0.5).rounded(.up), 0), Double(width))
            let y0 = min(max((y - radius - 0.5).rounded(.up), 0), Double(height))
            let x1 = min(max((x + radius - 0.5).rounded(.down) + 1, 0), Double(width))
            let y1 = min(max((y + radius - 0.5).rounded(.down) + 1, 0), Double(height))
            total += Int(x1 - x0) * Int(y1 - y0)
        }
    }
    return total
}

/// L1B: an empty local, then one brush command of 2048 dabs on a 64×32 grid,
/// at the largest radius ≤ 180 whose evaluated pixels stay within the core's
/// 256,000,000 cap (`MAX_RAW_LOCAL_WORK`), so it bounds the worst admitted brush.
func brushedL1(_ width: Int, _ height: Int) -> [([String: Any], Int)] {
    var radius = 180.0
    while radius > 1, brushWork(width, height, radius: radius) > 256_000_000 { radius -= 1 }
    fields["brush_radius"] = radius
    fields["brush_evaluated_px"] = brushWork(width, height, radius: radius)
    var dabs: [[String: Any]] = []
    for row in 0..<32 {
        for column in 0..<64 {
            let center: [Int] = [(2 * column + 1) * width / 128, (2 * row + 1) * height / 64]
            let dab: [String: Any] = ["center": center, "radius": radius, "hardness": 0.5, "opacity": 1]
            dabs.append(dab)
        }
    }
    let empty: [String: Any] = ["op": "empty"]
    let create: [String: Any] = ["op": "create", "id": "l1", "mask": empty]
    let command: [String: Any] = ["op": "brush", "mode": "add", "dabs": dabs]
    let brush: [String: Any] = ["op": "mask", "id": "l1", "command": command]
    return [(create, 0), (brush, dabs.count)]
}

struct PreviewShape {
    let operations: [([String: Any], Int)]
    let exposure: Double
    let effects: Bool
    let selected: String?
    /// L1C: the source is `prepare(exposure 0.5, [L1])`'s result.
    let chained: Bool
}

func previewShape(_ name: String, _ width: Int, _ height: Int) -> PreviewShape {
    switch name {
    case "L0", "M0": return PreviewShape(operations: [], exposure: 0.5, effects: false, selected: nil, chained: false)
    case "L1", "M1", "M11":
        return PreviewShape(operations: [(createL1(width, height), 0)], exposure: 0.5, effects: false,
                            selected: "l1", chained: false)
    case "L1B": return PreviewShape(operations: brushedL1(width, height), exposure: 0.5, effects: false,
                                    selected: "l1", chained: false)
    case "L1C", "MC": return PreviewShape(operations: [], exposure: 0.2, effects: false, selected: "l1", chained: true)
    case "L4": return PreviewShape(operations: createL4(width, height).map { ($0, 0) }, exposure: 0.5,
                                   effects: false, selected: "l1", chained: false)
    case "FX", "PX": return PreviewShape(operations: [(createL1(width, height), 0)], exposure: 0.5, effects: true,
                                         selected: "l1", chained: false)
    case "LT1": return PreviewShape(operations: [(createLT1(width, height), 0)], exposure: 0.5, effects: false,
                                    selected: "l1", chained: false)
    case "LT16": return PreviewShape(operations: createLT16(width, height).map { ($0, 0) }, exposure: 0.5,
                                     effects: false, selected: "l1", chained: false)
    case "LTS": return PreviewShape(operations: [(createLTS(width, height), 0)], exposure: 0.5, effects: false,
                                    selected: "l1", chained: false)
    case "LTD": return PreviewShape(operations: [(createLTD(width, height), 0)], exposure: 0.5, effects: false,
                                    selected: "l1", chained: false)
    case "LS", "LP":
        // L1 with a Vision-generated mask: the subject, or the people, of the base development.
        guard let kind = generatedShape(name) else { fail("unknown shape \(name)") }
        let mask: [String: Any] = ["op": kind.rawValue]
        let create: [String: Any] = ["op": "create", "id": "l1", "exposure_offset": 1.0, "mask": mask]
        return PreviewShape(operations: [(create, 0)], exposure: 0.5, effects: false, selected: "l1", chained: false)
    default: fail("unknown shape \(name)")
    }
}

/// The generated-mask kind of shapes LS and LP; nil for every other shape.
func generatedShape(_ name: String) -> RawLocalGeneratedKind? {
    switch name {
    case "LS": return .subject
    case "LP": return .people
    default: return nil
    }
}

/// A warm RAW opens through `openRetainedRaw`; a cold `.rz` through the
/// native reopen, so no decode or CIRAWFilter has run in the process.
func openPhase2Source() throws -> RasterDocument {
    if file.pathExtension == "rz" {
        guard let reopened = try step({ try ImageDocument.openPrecisionDocumentIfRequested(at: file) }) else {
            fail("the .doctored did not open")
        }
        fields["cold"] = true
        return reopened
    }
    fields["cold"] = false
    return try step { try ImageDocument.openRetainedRaw(probe(file)).doc }
}

/// The L1C/MC source: `RawLocalWork.prepare(exposure 0.5, [L1])`'s result.
func chainedSource(_ source: RasterDocument, _ width: Int, _ height: Int) throws -> RasterDocument {
    guard let prepared = try step({
        try RawLocalWork.prepare(source: source, layer: 0, settings: phase2Settings(exposure: 0.5, effects: false),
                                 operations: [createL1(width, height)])
    }) else { fail("prepare(L1) made no change") }
    return prepared
}

func phase2Fields(_ retained: Retained) {
    fields["px"] = retained.width * retained.height
    fields["width"] = retained.width
    fields["height"] = retained.height
    fields["encoded"] = retained.encoded
}

/// The session's request, then the main run loop until `onStatus` reports
/// ready or anything other than the updating line. Answers (status, ready).
func previewRequest(_ session: RawLocalDevelopSession, settings: RawDevelopSettings,
                    selected: String?) -> (String, Bool) {
    var outcome: (String, Bool)?
    session.onStatus = { status, ready in
        if ready || status != "Updating native RAW development…" { outcome = (status, ready) }
    }
    session.request(settings: settings, viewport: RawDevelopViewport(), selected: selected,
                    overlay: selected != nil, canonicalEditing: selected != nil)
    let deadline = Date().addingTimeInterval(1500)
    while outcome == nil, Date() < deadline {
        _ = RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.02))
    }
    session.onStatus = nil
    return outcome ?? ("the preview did not finish within 1500 s", false)
}

/// What `RawLocalWork.request` charges for this session's next request
/// (before the session's +64 MiB).
func localCharge(_ source: RasterDocument, settings: RawDevelopSettings,
                 operations: [[String: Any]], retained: Retained) throws -> [String: Any] {
    try step {
        let captured = try RawLocalWork.state(source, layer: 0)
        let desired = try captured.descriptor.recipe.updating(settings)
        let owned = try RawLocalOperations.capture(operations)
        let shape = RawLocalWork.chargeShape(captured: captured, desired: desired, operations: owned)
        let float = desired.isLinearFloat
        let charge = try RawLocalWork.measuredScratch(width: retained.width, height: retained.height,
            encoded: retained.encoded, locals: shape.locals, toneLocals: shape.toneLocals,
            toneStage: shape.toneStage, effects: shape.effects, float: float)
        // `request` charges the replace stage's forecast when it is larger.
        let replace = try RawLocalWork.replaceScratch(source: source, layer: 0, width: retained.width,
            height: retained.height, locals: shape.locals, float: float)
        // What the Develop session admits beside the request's budget for a
        // float development's inspection copies (`RawLocalDevelopSession`).
        let inspection = float ? UInt64(retained.width) * UInt64(retained.height) * 24 : 0
        return ["locals": shape.locals, "tone_locals": shape.toneLocals, "tone_stage": shape.toneStage,
                "effects": shape.effects, "charge": charge,
                "budget": max(charge, replace), "replace_charge": replace,
                "gate_charge": charge + inspection,
                "decode_charge": try RawSourceWork.scratch(width: retained.width, height: retained.height,
                                                           encoded: retained.encoded, float: float),
                "old_local_charge": try RawLocalWork.scratch(width: retained.width, height: retained.height,
                                                             encoded: retained.encoded)]
    }
}

func phase2Restore() throws {
    var source = try openPhase2Source()
    let retained = try Retained(source)
    phase2Fields(retained)
    if shapeName == "L4" {
        let operations = createL4(retained.width, retained.height)
        guard let prepared = try step({
            try RawLocalWork.prepare(source: source, layer: 0,
                settings: phase2Settings(exposure: 0.5, effects: false), operations: operations)
        }) else { fail("prepare(L4) made no change") }
        source = prepared
    }
    fields["charge"] = try RawLocalWork.restoreScratch(width: retained.width, height: retained.height,
                                                        encoded: retained.encoded)
    fields["setup_retained"] = retainedBytes()
    let target = source
    settle()
    if baseline {
        record(measure { () })
    } else {
        // Layer ▸ Redevelop RAW… up to the open window: the restore, then
        // what `runRetained` holds (the before master and the session).
        let run = measure { () throws -> ((descriptor: RawSourceDescriptor, snapshot: RawSourceSnapshot,
                                           probe: RawProbe), RasterPrecisionImage, RawLocalDevelopSession) in
            let restored = try RawLocalWork.restoreForRedevelop(source: target, layer: 0)
            let master = try target.sourceMaster(0)
            let session = try RawLocalDevelopSession(source: target, layer: 0)
            return (restored, master, session)
        }
        record(run)
        withExtendedLifetime(run) {}
    }
    withExtendedLifetime(target) {}
}

func phase2Local() throws {
    var source = try openPhase2Source()
    var restored: (descriptor: RawSourceDescriptor, snapshot: RawSourceSnapshot, probe: RawProbe)?
    if fields["cold"] as? Bool == true {
        // The menu's restore creates the process's first CIRAWFilter without
        // rendering; the measured request is then the first render.
        restored = try step { try RawLocalWork.restoreForRedevelop(source: source, layer: 0) }
    }
    let retained = try Retained(source)
    phase2Fields(retained)
    let shape = previewShape(shapeName, retained.width, retained.height)
    if shape.chained { source = try chainedSource(source, retained.width, retained.height) }
    let target = source
    let master = try step { try target.sourceMaster(0) }
    let session = try step { try RawLocalDevelopSession(source: target, layer: 0) }
    for (operation, dabs) in shape.operations { try step { try session.append(operation, dabs: dabs) } }
    let settings = phase2Settings(exposure: shape.exposure, effects: shape.effects, presence: shapeName == "PX")
    for (key, value) in try localCharge(target, settings: settings, operations: session.operations,
                                        retained: retained) {
        fields[key] = value
    }
    fields["setup_retained"] = retainedBytes()
    settle()
    if baseline {
        record(measure { () })
    } else {
        let run = measure { previewRequest(session, settings: settings, selected: shape.selected) }
        record(run)
        if case .success(let (status, ready)) = run.result {
            fields["status"] = status
            let generated = generatedShape(shapeName)
            if !ready {
                // Vision finding nothing is the stage's result, not a refusal: the run keeps its E.
                if let generated, status == generated.nothingFound {
                    fields["no_result"] = status
                } else { fields["refused"] = status }
            }
            // The session drops a generating create that found nothing on its first run (0 left);
            // one that made a mask stays (1).
            if generated != nil { fields["operations_after"] = session.operations.count }
        }
    }
    withExtendedLifetime((master, session, restored, target)) {}
}

func phase2Publish() throws {
    let source = try openPhase2Source()
    let retained = try Retained(source)
    phase2Fields(retained)
    let operations = shapeName == "L1" ? [createL1(retained.width, retained.height)] : []
    guard let result = try step({
        try RawLocalWork.prepare(source: source, layer: 0, settings: phase2Settings(exposure: 0.5, effects: false),
                                 operations: operations)
    }) else { fail("prepare made no change") }
    fields["projection_scratch_estimate"] = try result.projectionScratchEstimate()
    fields["setup_retained"] = retainedBytes()
    settle()
    if baseline {
        record(measure { () })
    } else {
        let run = measure { PreparedDocumentEdit(result) }
        record(run)
        if case .success(let prepared) = run.result, let error = prepared.error { fields["refused"] = error }
        withExtendedLifetime(run) {}
    }
    withExtendedLifetime((source, result)) {}
}

func menuDocument(_ source: RasterDocument) -> ImageDocument {
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: source.isHighDepth ? 16 : 8, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    return document
}

/// Layer ▸ Redevelop RAW… stage by stage at the process's working limit:
/// restore, the window's holdings, each preview request, then the hand-off
/// to `redevelopRaw`'s publish. Each stage prints ok or the sentence, its
/// charge, and `unique_retained_bytes` / `transient_estimate_bytes` read
/// just before it.
func phase2Menu() throws {
    var source = try openPhase2Source()
    let retained = try Retained(source)
    phase2Fields(retained)
    let shape = previewShape(shapeName, retained.width, retained.height)
    if shape.chained { source = try chainedSource(source, retained.width, retained.height) }
    let document = menuDocument(source)
    let capture = try ImageSourceCapture(document)
    var stages: [[String: Any]] = []
    func stage(_ name: String, charge: () throws -> UInt64?, _ body: () throws -> Void) -> Bool {
        var entry: [String: Any] = ["stage": name]
        if let report = try? RasterResources.shared.report() {
            entry["unique_retained_bytes"] = report["unique_retained_bytes"]
            entry["transient_estimate_bytes"] = report["transient_estimate_bytes"]
        }
        if let value = try? charge() { entry["charge"] = value }
        do {
            try autoreleasepool { try body() }
            entry["ok"] = true
        } catch {
            entry["ok"] = false
            entry["sentence"] = error.localizedDescription
        }
        stages.append(entry)
        return entry["ok"] as? Bool == true
    }
    var restored: (descriptor: RawSourceDescriptor, snapshot: RawSourceSnapshot, probe: RawProbe)?
    var master: RasterPrecisionImage?
    var session: RawLocalDevelopSession?
    defer { withExtendedLifetime((restored, master, session)) {} }
    let target = capture.source
    var going = stage("restore", charge: {
        try RawLocalWork.restoreScratch(width: retained.width, height: retained.height, encoded: retained.encoded)
    }) {
        restored = try RawLocalWork.restoreForRedevelop(source: target, layer: 0)
        try capture.requireCurrent(document)
    }
    if going {
        going = stage("session", charge: { UInt64(RawLocalWork.maximumOperationBytes) * 128 + 4 * 1024 * 1024 }) {
            master = try target.sourceMaster(0)
            session = try RawLocalDevelopSession(source: target, layer: 0)
        }
    }
    var requests: [(RawDevelopSettings, String?)] = [
        (phase2Settings(exposure: shape.exposure, effects: shape.effects), shape.selected)]
    if shapeName == "M11" { requests.append((phase2Settings(exposure: 0.2, effects: false), shape.selected)) }
    if going, let session {
        for (operation, dabs) in shape.operations { try session.append(operation, dabs: dabs) }
        for (index, request) in requests.enumerated() where going {
            going = stage("preview \(index + 1)", charge: {
                let charge = try localCharge(target, settings: request.0, operations: session.operations,
                                             retained: retained)
                guard let budget = charge["budget"] as? UInt64 else { return nil }
                return try PrecisionDocumentDescriptor.sum(budget, 64 * 1024 * 1024)
            }) {
                let (status, ready) = previewRequest(session, settings: request.0, selected: request.1)
                guard ready else { throw DoctoredCoreError(message: status) }
            }
        }
    }
    if going, let current = session {
        let held = current.result
        going = stage("publish", charge: { try held?.document.projectionScratchEstimate() }) {
            guard let held else { throw DoctoredCoreError(message: "The session holds no result.") }
            current.cancel()
            session = nil
            try RawLocalWork.publish(prepared: held.document, in: document, capture: capture,
                layer: 0, settings: held.settings, operations: held.operations)
        }
        fields["published"] = going && document.doc !== target
    }
    fields["stages"] = stages
    fields["ok"] = going
}

/// GR: `applyingRawGrain` alone on an opened float development (the open is setup,
/// outside the interval). The gate is the core's own selected-edit plan (`scratch_bytes`) for
/// the request `applyingRawGrain` sends.
func floatGrain() throws {
    guard floatDepth else { fail("grain runs at depth 32") }
    let opened = try step { try ImageDocument.openRetainedRaw(probe(file)).doc }
    let retained = try Retained(opened)
    phase2Fields(retained)
    fields["master_is_float"] = retained.descriptor.recipe.isLinearFloat
    let sha = retained.descriptor.content_sha256
    guard let precision = opened.precision else { fail("the float open has no precision document") }
    let seed = try RawGrainSettings.seed(contentSHA256: sha)
    let request = try RawLocalJSON.string(["layer": 0, "op": "add_grain", "params": [
        "amount": probeGrain.amount, "size": probeGrain.size, "roughness": probeGrain.roughness,
        "seed": Int(bitPattern: UInt(seed))]])
    let budget = try PrecisionWorkContext(source: precision)
    let plan = try precision.selectedEditPlan(request: request, workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained)
    fields["gate_charge"] = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
    fields["setup_retained"] = retainedBytes()
    settle()
    if baseline {
        record(measure { () })
    } else {
        let run = measure { try opened.applyingRawGrain(probeGrain, contentSHA256: sha) }
        record(run)
        if case .success(let grained) = run.result { fields["grained"] = grained !== opened }
        withExtendedLifetime(run) {}
    }
    withExtendedLifetime(opened) {}
}

/// VF: one viewport-only `RawDevelopPreviewFrame` of two known f32 sides on a screen with
/// headroom, with Clipping's highlights, at Fit (F) or a 1024 × 1024 100 % region (D). Setup,
/// outside the interval: the two float developments (the file's own, and exposure 0.5), their
/// inspection sources and the first frame (Clipping off), which stays alive as the panel holds
/// it. The gate is the session's own charge for that frame: 48 MiB +
/// `extendedFramePerPixel` × the f32 sides' output pixels.
func floatFrame() throws {
    guard floatDepth, shapeName == "F" || shapeName == "D" else { fail("frame runs at depth 32 with F or D") }
    let found = step { probe(file) }
    let count = try RawSourceSnapshot.size(file)
    let snapshot = try step { try RawSourceSnapshot.capture(file, maximumBytes: count) }
    let captured = try step {
        try RetainedRawDecoder.capture(url: snapshot.url, filename: file.lastPathComponent,
                                       sha256: snapshot.sha256)
    }
    func floatSource(_ exposure: Double?) throws -> RawDevelopInspectionSource {
        try step {
            var settings = RawDevelopSettings()
            settings.outputEncoding = .linearFloat
            if let exposure { settings.exposure = exposure }
            var descriptor = captured
            descriptor.recipe = try captured.recipe.updating(settings)
            let developed = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: descriptor,
                maximumScratch: .max, whiteBalanceChanged: false)
            guard let image = developed.0.precision?.layerSource(0), image.isFloatStorage else {
                throw DoctoredCoreError(message: "The float development has no f32 samples.")
            }
            return try RawDevelopInspectionSource(image)
        }
    }
    let before = try floatSource(nil), after = try floatSource(0.5)
    var viewport = RawDevelopViewport()
    viewport.extendedRange = true
    if shapeName == "D" {
        viewport.nativeDetail = true
        viewport.backingSize = CGSize(width: 1024, height: 1024)
    }
    let first = try step { try RawDevelopPreviewFrame(before: before, after: after, viewport: viewport) }
    viewport.clipping = 2
    let pixels = [before, after].reduce(UInt64(0)) {
        $0 + RawDevelopPreviewFrame.extendedOutputPixels($1.extendedMaster, viewport: viewport)
    }
    fields["px"] = Int(found.pixelSize.width) * Int(found.pixelSize.height)
    fields["encoded"] = UInt64(snapshot.bytes.count)
    fields["frame_px"] = pixels
    fields["gate_charge"] = 48 * 1024 * 1024 + RawDevelopPreviewSession.extendedFramePerPixel * pixels
    fields["setup_retained"] = retainedBytes()
    settle()
    if baseline {
        record(measure { () })
    } else {
        let target = viewport
        let run = measure { try RawDevelopPreviewFrame(before: before, after: after, viewport: target) }
        record(run)
        if case .success(let frame) = run.result {
            fields["extended"] = frame.beforeExtendedPeak != nil && frame.afterExtendedPeak != nil
            fields["after_peak"] = frame.afterExtendedPeak.map { Double($0) }
        }
        withExtendedLifetime(run) {}
    }
    withExtendedLifetime((snapshot, first, before, after)) {}
}

do {
    switch mode {
    case "grain":
        try floatGrain()
    case "frame":
        try floatFrame()
    case "open":
        // OP: the open develops with Presence, the stage before grain.
        if shapeName == "OP" { RawImportRequest.mode = .headless(withPresence(RawDevelopSettings())) }
        // OG: the open develops grain, the last stage, at the probe's depth.
        if shapeName == "OG" {
            var settings = RawDevelopSettings()
            if floatDepth { settings.outputEncoding = .linearFloat }
            settings.grain = probeGrain
            RawImportRequest.mode = .headless(settings)
        }
        // OL (v1-lensfun): Profile Corrections on, which runs the Lensfun stage on a file whose
        // decoder has no lens profile; `lensfun` reports whether the recipe applies it.
        if shapeName == "OL" {
            var lens = RawDevelopSettings()
            lens.lensCorrection = true
            RawImportRequest.mode = .headless(lens)
        }
        let found = step { probe(file) }
        if shapeName == "OL" {
            // The capture's match is the probe's (`RawDevelopmentRecipe.init(filter:)`).
            fields["lensfun"] = !found.capabilities.lensCorrection && found.lensfun?.profile != nil
        }
        fields["px"] = Int(found.pixelSize.width) * Int(found.pixelSize.height)
        fields["gate_charge"] = try RawSourceWork.scratch(width: Int(found.pixelSize.width),
            height: Int(found.pixelSize.height), encoded: encodedFile, float: floatDepth)
        settle()
        if baseline {
            record(measure { () })
        } else {
            let run = measure { try ImageDocument.openRetainedRaw(found) }
            record(run)
            if case .success(let opened) = run.result { fields["retained"] = retainedBytes(); _ = opened }
        }
    case "warm", "cold", "prep":
        let source: RasterDocument
        if mode == "cold" {
            guard let reopened = try step({ try ImageDocument.openPrecisionDocumentIfRequested(at: file) }) else {
                fail("the .doctored did not open")
            }
            source = reopened
        } else {
            source = try step { try ImageDocument.openRetainedRaw(probe(file)).doc }
        }
        let retained = try Retained(source)
        fields["px"] = retained.width * retained.height
        fields["encoded"] = retained.encoded
        fields["setup_retained"] = retainedBytes()
        fields["master_is_float"] = retained.descriptor.recipe.isLinearFloat
        fields["gate_charge"] = try RawSourceWork.scratch(width: retained.width, height: retained.height,
            encoded: retained.encoded, float: retained.desired.recipe.isLinearFloat)
        settle()
        if baseline {
            record(measure { () })
        } else if mode == "prep" {
            let run = measure { try RawSourceWork.prepare(source: source, layer: 0, settings: redevelopSettings()) }
            record(run)
            fields["prepared"] = (try? run.result.get()) != nil
            withExtendedLifetime(run) {}
            // Stage 2's inputs, once the measured result is gone: the decoded
            // asset beside the source, and the plan `replacing` admits.
            let (snapshot, decoded) = try retained.decode()
            _ = snapshot
            let command = try PrecisionPixelCommand(["op": "replace", "layer": 0,
                "revision_id": UUID().uuidString, "depth_conversion": "convert_fallback", "link": NSNull()])
            let plan = try source.retainedSourcePlan(asset: decoded.0, request: command.request)
            fields["stage2_scratch_bytes"] = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            fields["stage2_unique_retained_bytes"] = retainedBytes()
        } else {
            let run = measure { try retained.decode() }
            record(run)
            withExtendedLifetime(run) {}
        }
    case "preview":
        // Setup: what RawDevelopPreviewSession holds before its first request.
        let found = step { probe(file) }
        let count = try RawSourceSnapshot.size(file)
        let snapshot = try step { try RawSourceSnapshot.capture(file, maximumBytes: count) }
        let captured = try step {
            try RetainedRawDecoder.capture(url: snapshot.url, filename: file.lastPathComponent,
                                           sha256: snapshot.sha256)
        }
        // The import window at the float depth row: before is the captured
        // 16-bit development, after the float one, and the session charges
        // the float decode figure plus 24 B/px of inspection copies.
        let px = UInt64(found.pixelSize.width) * UInt64(found.pixelSize.height)
        let scratch = try RawSourceWork.scratch(width: Int(found.pixelSize.width),
            height: Int(found.pixelSize.height), encoded: UInt64(snapshot.bytes.count),
            float: floatDepth) + (floatDepth ? px * 24 : 0)
        fields["gate_charge"] = scratch
        var requested = redevelopSettings()
        if floatDepth { requested.outputEncoding = .linearFloat }
        var desired = captured
        // VP: the after develop runs Presence too, as the import preview does once a
        // slider moved (16/8 only: the float development refuses nonzero Presence).
        if shapeName == "VL" { requested.lensCorrection = true }
        // VG: the after develop runs grain, at the float encoding too.
        if shapeName == "VG" { requested.grain = probeGrain }
        desired.recipe = try captured.recipe.updating(shapeName == "VP" ? withPresence(requested)
                                                                         : requested)
        // VL (v1-lensfun): before and after both develop with the Lensfun stage.
        if shapeName == "VL" { fields["lensfun"] = captured.recipe.lensfunApplies && desired.recipe.lensfunApplies }
        fields["px"] = Int(found.pixelSize.width) * Int(found.pixelSize.height)
        fields["encoded"] = UInt64(snapshot.bytes.count)
        fields["setup_retained"] = retainedBytes()
        settle()
        if baseline {
            record(measure { () })
        } else {
            // RawDevelopPreviewSession's worker body: before, after, frame.
            let run = measure { () throws -> (RawDevelopInspectionSource, RawDevelopInspectionSource,
                                              RawDevelopPreviewFrame) in
                let developed = try RetainedRawDecoder.develop(url: snapshot.url,
                    descriptor: captured, maximumScratch: scratch)
                guard let image = developed.0.precision?.layerSource(0) else {
                    throw DoctoredCoreError(message: "The RAW before image has no native samples.")
                }
                let before = try RawDevelopInspectionSource(image)
                let developedAfter = try RetainedRawDecoder.develop(url: snapshot.url,
                    descriptor: desired, maximumScratch: scratch, whiteBalanceChanged: false)
                guard let afterImage = developedAfter.0.precision?.layerSource(0) else {
                    throw DoctoredCoreError(message: "The RAW after image has no native samples.")
                }
                let after = try RawDevelopInspectionSource(afterImage)
                // At the float depth the window is on a screen with
                // headroom, so the frame draws the f32 side's extended Fit.
                var viewport = RawDevelopViewport()
                viewport.extendedRange = floatDepth
                let frame = try RawDevelopPreviewFrame(before: before, after: after, viewport: viewport)
                return (before, after, frame)
            }
            record(run)
            withExtendedLifetime(run) {}
        }
        withExtendedLifetime(snapshot) {}
    case "resident":
        // P0 after app setup, before any RAW call; then open and redevelop,
        // each read with its result alive: once right after the step and once
        // after `settle()` (the residual is the settled reading).
        let p0 = usage().ri_phys_footprint
        fields["p0"] = p0
        func residual(_ footprint: UInt64, _ retained: UInt64) -> Int64 {
            Int64(bitPattern: footprint) - Int64(bitPattern: p0) - Int64(bitPattern: retained)
        }
        let opened = try step { try ImageDocument.openRetainedRaw(probe(file)) }
        let openImmediate = usage().ri_phys_footprint
        settle()
        let openFootprint = usage().ri_phys_footprint
        let openRetained = retainedBytes()
        fields["px"] = opened.doc.width * opened.doc.height
        fields["open_footprint"] = openFootprint
        fields["open_retained"] = openRetained
        fields["residual_open_immediate"] = residual(openImmediate, openRetained)
        fields["residual_open"] = residual(openFootprint, openRetained)
        let prepared = try step {
            try RawSourceWork.prepare(source: opened.doc, layer: 0, settings: redevelopSettings())
        }
        let redevelopImmediate = usage().ri_phys_footprint
        settle()
        let redevelopFootprint = usage().ri_phys_footprint
        let redevelopRetained = retainedBytes()
        fields["prepared"] = prepared != nil
        fields["redevelop_footprint"] = redevelopFootprint
        fields["redevelop_retained"] = redevelopRetained
        fields["residual_redevelop_immediate"] = residual(redevelopImmediate, redevelopRetained)
        fields["residual_redevelop"] = residual(redevelopFootprint, redevelopRetained)
        withExtendedLifetime((opened, prepared)) {}
    case "restore":
        try phase2Restore()
    case "local":
        try phase2Local()
    case "publish":
        try phase2Publish()
    case "menu":
        try phase2Menu()
    case "save":
        guard rest.count >= 4 else { fail("save needs an output path") }
        let opened = try ImageDocument.openRetainedRaw(probe(file))
        try opened.doc.saveNative(to: URL(fileURLWithPath: rest[3]))
        fields["saved"] = rest[3]
    default:
        fail("unknown mode \(mode)")
    }
} catch {
    fields["setup_refused"] = error.localizedDescription
}
emit(fields)
exit(0)
