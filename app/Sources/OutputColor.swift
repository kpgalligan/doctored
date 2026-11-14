import AppKit

// `OutputColor` carries values only (`RenderingIntent`, `AdjustmentPolicy`, `ConversionOptions`,
// `ConversionReport`, `InstalledProfiles` — whose scan also walks the printer-driver root
// recursively, down to `printerDepth` enumerator levels and into vendor packages, through the
// read-only `DR_PRINTERS_DIR` seam for drives); every RULE, spelling, default and refusal sentence
// belongs to the core.

/// Phase 11C output colour control, host side: the rendering intents, the
/// conversion options a Convert to Profile runs under, the report it comes
/// back with, and the installed-profile list the sheets choose from.
///
/// Every RULE lives in the core (`icc_intent`, `doc_color_policy`) — the
/// spellings, the defaults, the per-kind adjustment table, every refusal
/// sentence. This file carries values and builds menus.

/// The four ICC rendering intents. The raw values ARE the core's JSON
/// spellings and the MCP argument's, so this enum is a wire contract.
enum RenderingIntent: String, CaseIterable {
    case perceptual
    case relative
    case saturation
    case absolute

    /// The `DR_INTENT_*` discriminant.
    var code: Int32 {
        switch self {
        case .perceptual: return 0
        case .relative: return 1
        case .saturation: return 2
        case .absolute: return 3
        }
    }

    /// What the popup shows: the ICC rendering intents' standard names.
    var displayName: String {
        switch self {
        case .perceptual: return "Perceptual"
        case .relative: return "Relative Colorimetric"
        case .saturation: return "Saturation"
        case .absolute: return "Absolute Colorimetric"
        }
    }

    /// One line under the popup.
    var summary: String {
        switch self {
        case .perceptual:
            return "Compresses the whole gamut so relationships between colours survive."
        case .relative:
            return "Keeps in-gamut colours exactly and clips the rest; white maps to white."
        case .saturation:
            return "Favours vivid colour over accuracy — charts and graphics, not photographs."
        case .absolute:
            return "Keeps the source white, so the medium's own paper tint shows."
        }
    }
}

/// What a conversion does to the document's adjustment layers. The raw
/// values are the core's JSON spellings.
enum AdjustmentPolicy: String, CaseIterable {
    case keep
    case preserveAppearance = "preserve_appearance"

    var displayName: String {
        switch self {
        case .keep: return "Keep parameters"
        case .preserveAppearance: return "Preserve appearance"
        }
    }

    var summary: String {
        switch self {
        case .keep:
            return "Every adjustment keeps its numbers; the stack recomputes from the "
                + "converted pixels."
        case .preserveAppearance:
            return "Light-linear adjustments are kept; every other one is baked into a "
                + "disclosed raster layer in its place."
        }
    }
}

/// Everything a Convert to Profile is parameterized by beyond the
/// destination. The defaults are today's conversion exactly.
struct ConversionOptions: Equatable {
    var intent: RenderingIntent = .relative
    var blackPointCompensation: Bool = false
    var adjustments: AdjustmentPolicy = .keep
    /// Rasterize every RAW layer first when these settings cannot keep
    /// its source bound (anything but relative colorimetric with black point
    /// compensation off), then convert, as one result. Settings the sources
    /// can follow keep them bound whatever this says.
    var rasterizeSources: Bool = false

    static let `default` = ConversionOptions()

    var isDefault: Bool { self == ConversionOptions.default }

    /// The object the core's `options_json` takes and the MCP tool echoes.
    var json: [String: Any] {
        [
            "intent": intent.rawValue,
            "black_point_compensation": blackPointCompensation,
            "adjustments": adjustments.rawValue,
            "rasterize_sources": rasterizeSources,
        ]
    }

    var jsonText: String {
        guard let data = try? JSONSerialization.data(withJSONObject: json),
              let text = String(data: data, encoding: .utf8)
        else { return "{}" }
        return text
    }

    init(intent: RenderingIntent = .relative,
         blackPointCompensation: Bool = false,
         adjustments: AdjustmentPolicy = .keep,
         rasterizeSources: Bool = false) {
        self.intent = intent
        self.blackPointCompensation = blackPointCompensation
        self.adjustments = adjustments
        self.rasterizeSources = rasterizeSources
    }

    /// Reads the four keys out of an MCP argument dictionary; an unknown
    /// spelling throws with the core's wording.
    init(arguments: [String: Any]) throws {
        self.init()
        if let raw = arguments["intent"] as? String {
            guard let value = RenderingIntent(rawValue: raw) else {
                throw DoctoredCoreError(
                    message: "intent \"\(raw)\" is not one of perceptual, relative, "
                        + "saturation or absolute")
            }
            intent = value
        }
        if let flag = arguments["black_point_compensation"] as? Bool {
            blackPointCompensation = flag
        } else if let text = arguments["black_point_compensation"] as? String {
            blackPointCompensation = (text as NSString).boolValue
        }
        if let raw = arguments["adjustments"] as? String {
            guard let value = AdjustmentPolicy(rawValue: raw) else {
                throw DoctoredCoreError(
                    message: "adjustments \"\(raw)\" is not one of keep or preserve_appearance")
            }
            adjustments = value
        }
        if let flag = arguments["rasterize_sources"] as? Bool {
            rasterizeSources = flag
        } else if let text = arguments["rasterize_sources"] as? String {
            rasterizeSources = (text as NSString).boolValue
        }
    }
}

/// What the policy decided about ONE adjustment layer — the core's own
/// sentence, carried verbatim.
struct AdjustmentOutcome: Equatable {
    let index: Int
    let name: String
    let op: String
    /// `"kept"` or `"baked"`.
    let action: String
    let reason: String

    var isBaked: Bool { action == "baked" }

    var json: [String: Any] {
        ["index": index, "name": name, "op": op, "action": action, "reason": reason]
    }
}

/// Every adjustment layer's outcome, in stack order. The preflight's report
/// and the conversion's are the same list, so a preview cannot describe a
/// different result from the one that lands.
struct ConversionReport: Equatable {
    var adjustments: [AdjustmentOutcome] = []

    var bakes: Bool { adjustments.contains { $0.isBaked } }

    var jsonArray: [[String: Any]] { adjustments.map(\.json) }

    /// One line for the sheet's note and the MCP notice; empty when the
    /// document has no adjustment layers at all.
    var notice: String {
        guard !adjustments.isEmpty else { return "" }
        let baked = adjustments.filter(\.isBaked)
        if baked.isEmpty {
            return "\(adjustments.count) adjustment "
                + (adjustments.count == 1 ? "layer keeps its" : "layers keep their")
                + " parameters."
        }
        let names = baked.map { "“\($0.name)”" }.joined(separator: ", ")
        return "\(baked.count) of \(adjustments.count) adjustment layers will be baked "
            + "into raster layers: \(names)."
    }

    /// Parses the core's report array.
    static func parse(_ value: Any?) -> ConversionReport {
        guard let rows = value as? [[String: Any]] else { return ConversionReport() }
        return ConversionReport(adjustments: rows.compactMap { row in
            guard let index = row["index"] as? Int,
                  let name = row["name"] as? String,
                  let op = row["op"] as? String,
                  let action = row["action"] as? String,
                  let reason = row["reason"] as? String
            else { return nil }
            return AdjustmentOutcome(
                index: index, name: name, op: op, action: action, reason: reason)
        })
    }

    /// Parses a whole `{"adjustments":[…]}` JSON string.
    static func parse(json text: String) -> ConversionReport {
        guard let data = text.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data)
        else { return ConversionReport() }
        if let array = root as? [[String: Any]] { return parse(array) }
        return parse((root as? [String: Any])?["adjustments"])
    }
}

// MARK: - export destinations

extension RasterProfileDescription {
    /// Why an export cannot convert into this profile, or nil when it can:
    /// a matrix/TRC profile whose tone curves fail the precision check
    /// every conversion into it runs. A LUT destination converts at every
    /// depth. The sentence is the core's (`dr_icc_describe`'s
    /// `export_refusal`).
    var exportDestinationRefusal: String? { exportRefusal }
}

// MARK: - installed profiles

/// One profile file the system offers, with the core's description of it.
struct InstalledProfile: Equatable {
    let url: URL
    let description: RasterProfileDescription

    var name: String { description.name }
    var path: String { url.path }
}

/// The ICC profiles installed on this machine, read once per launch.
///
/// The folders are ColorSync's three standard ones plus Adobe's, which is
/// where every press profile a photographer actually proofs against lives
/// (`CoatedFOGRA39.icc`, `USWebCoatedSWOP.icc`), read flat. After them the
/// printer-driver root (`/Library/Printers`, or `DR_PRINTERS_DIR` for a
/// drive) is walked recursively down to `printerDepth` levels — including
/// inside `.plugin`/`.bundle`/`.canonicc` packages, where vendors keep their
/// paper profiles — so a ColorSync or Adobe name wins a duplicate. Each file
/// is described by the CORE, so a file that is not a profile, or is one this
/// build cannot read, is filtered out with the core's own reason rather than
/// a guess from the extension.
enum InstalledProfiles {
    /// The folders scanned, in the order a duplicate name resolves.
    static let folders: [URL] = {
        var out: [URL] = []
        if let home = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask).first {
            out.append(home.appendingPathComponent("ColorSync/Profiles"))
        }
        out.append(URL(fileURLWithPath: "/Library/ColorSync/Profiles"))
        out.append(URL(fileURLWithPath: "/System/Library/ColorSync/Profiles"))
        out.append(URL(fileURLWithPath: "/Library/Application Support/Adobe/Color/Profiles"))
        out.append(
            URL(fileURLWithPath:
                    "/Library/Application Support/Adobe/Color/Profiles/Recommended"))
        return out
    }()

    /// The printer-driver root walked after `folders`: `DR_PRINTERS_DIR` when
    /// it is set and non-empty (a read-only scan root for drives), else
    /// `/Library/Printers`.
    static let printerRoot: URL = {
        if let override = ProcessInfo.processInfo.environment["DR_PRINTERS_DIR"],
           !override.isEmpty {
            return URL(fileURLWithPath: override)
        }
        return URL(fileURLWithPath: "/Library/Printers")
    }()

    /// The deepest FileManager enumerator `level` visited below
    /// `printerRoot`: a directory at this level is listed but not descended.
    /// Vendor layouts put paper profiles about seven levels down
    /// (`Canon/BJPrinter/Resources/ICCProfiles/X.canonicc/Contents/Resources`).
    static let printerDepth = 8

    /// The largest profile file this scan will read. A profile past the
    /// document blob cap could not be assigned or embedded anyway.
    private static let maximumBytes = 16 * 1024 * 1024

    private static var cache: [InstalledProfile]?
    private static let lock = NSLock()

    /// Every readable profile, cached for the session. Scanning costs one
    /// directory walk and one core parse per file; the Adobe folder alone is
    /// ~40 files, and the largest is FOGRA39's 640 KB.
    static var all: [InstalledProfile] {
        lock.lock()
        defer { lock.unlock() }
        if let cache { return cache }
        var seen = Set<String>()
        var out: [InstalledProfile] = []
        for folder in folders {
            let contents = (try? FileManager.default.contentsOfDirectory(
                at: folder, includingPropertiesForKeys: [.fileSizeKey],
                options: [.skipsHiddenFiles])) ?? []
            for url in contents.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
                accept(url, seen: &seen, into: &out)
            }
        }
        for url in printerProfileURLs() {
            accept(url, seen: &seen, into: &out)
        }
        out.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        cache = out
        return out
    }

    /// The RGB profiles a document can be converted INTO or exported into.
    static var rgb: [InstalledProfile] {
        all.filter { $0.description.space == "rgb" && $0.description.canEncode }
    }

    /// The `rgb` profiles a 32-bit float conversion accepts as a destination:
    /// the core's `high_depth_destination` answer (a matrix/TRC model that
    /// passes the precision checks), so neither a LUT profile nor a matrix
    /// profile whose curves fail them is offered. A 16-bit conversion takes
    /// `exportDestinations`, as an 8-bit one does.
    static var highDepthDestinations: [InstalledProfile] {
        rgb.filter { $0.description.highDepthDestination }
    }

    /// The `rgb` profiles an export can convert into at every depth: a LUT
    /// profile, or a matrix/TRC one whose tone curves pass the precision check.
    /// Every export conversion, 8-bit included, runs that check on a
    /// matrix destination, so a failing one is not offered.
    static var exportDestinations: [InstalledProfile] {
        rgb.filter { $0.description.exportDestinationRefusal == nil }
    }

    /// The shared per-file acceptance of both passes: an `.icc`/`.icm` file
    /// (any case) of 1 byte … `maximumBytes`, described by the core, whose
    /// name has not been seen yet.
    private static func accept(_ url: URL, seen: inout Set<String>,
                               into out: inout [InstalledProfile]) {
        let ext = url.pathExtension.lowercased()
        guard ext == "icc" || ext == "icm" else { return }
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
        guard size > 0, size <= maximumBytes else { return }
        guard let bytes = try? Data(contentsOf: url, options: .mappedIfSafe),
              let description = RasterProfile.describe(bytes)
        else { return }
        // One entry per NAME: the same profile ships in several
        // folders and a popup listing it three times is noise.
        guard seen.insert(description.name).inserted else { return }
        out.append(InstalledProfile(url: url, description: description))
    }

    /// Every regular `.icc`/`.icm` file under `printerRoot`, sorted by path.
    /// Package descendants are NOT skipped (vendor profiles live inside
    /// bundles); hidden entries are, and the enumerator does not follow
    /// directory symlinks. A directory at `printerDepth` is not descended.
    private static func printerProfileURLs() -> [URL] {
        guard let walker = FileManager.default.enumerator(
            at: printerRoot, includingPropertiesForKeys: [.fileSizeKey, .isRegularFileKey],
            options: [.skipsHiddenFiles])
        else { return [] }
        var urls: [URL] = []
        while let url = walker.nextObject() as? URL {
            let regular = (try? url.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile
                ?? false
            if !regular {
                if walker.level >= printerDepth { walker.skipDescendants() }
                continue
            }
            let ext = url.pathExtension.lowercased()
            if ext == "icc" || ext == "icm" { urls.append(url) }
        }
        return urls.sorted { $0.path < $1.path }
    }

    /// Everything a soft proof can run through: both directions readable.
    static var proofable: [InstalledProfile] {
        all.filter(\.description.proofable)
    }

    /// The profiles of one or more device spaces, for `list_installed_profiles`.
    static func matching(kinds: [String]) -> [InstalledProfile] {
        guard !kinds.isEmpty else { return all }
        let wanted = Set(kinds)
        return all.filter { wanted.contains($0.description.space) }
    }

    /// The installed profile with exactly this display name, or nil.
    static func named(_ name: String) -> InstalledProfile? {
        all.first { $0.name == name }
    }

    /// Drops the cache — used by nothing in the shipping app, and present so
    /// a drive can force a rescan after writing a profile.
    static func invalidate() {
        lock.lock()
        cache = nil
        lock.unlock()
    }
}
