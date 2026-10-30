import AppKit

// The New Document model owns every rule — units, the ppi, the profile, the depth, the background,
// and the layer name ("Layer 1" transparent, "Background" filled) — so the sheet, the MCP tool and
// a preset are refused by identical sentences. A 16-bit document names its layer at CONSTRUCTION
// (`RasterDocument.fromRawRGBA16(…, layerName:)` → `dr_precision_doc_from_rgba16`'s `layer_name`),
// so it makes no rename edit and New Document works while a background job runs.
// `ImageDocument.makeNew` is the fourth document-creating path beside open, New from Clipboard and
// recovery, and deliberately does NOT call `adoptWorkingSpace()`: the request already names its
// space. A 32-bit float document is the 16-bit blank converted by the core's conversion into float,
// so the fill is that conversion's decode. GRAY is refused BY NAME rather than substituted — a
// document is RGB — and the sheet simply does not offer it.

/// File ▸ New… — starting a document with no seed image.
///
/// Until 12C a document could only begin as an OPENED file or as New from
/// Clipboard, which is a real gap for print work: the natural first move is
/// "an 8 × 10 inch page at 300 ppi in Display P3, transparent". This file is
/// the model half — the units, the background, the profile, the request and
/// its bounded named-preset library — with the sheet in
/// `NewDocumentSheetController.swift` and the MCP twin in
/// `AgentServer+NewDocument.swift`.
///
/// Depth 8, 16 or 32: a 32-bit FLOAT document is the 16-bit blank
/// converted into float (`core/src/precision_conversion_float.rs`), so white
/// is exactly 1.0, black and transparent 0, and a `#rrggbb` colour its
/// profile's tone-curve decode. One deliberate omission, a refusal BY NAME
/// rather than a silent substitution: GRAY (the core stores RGB documents
/// only — `RasterProfileKind.notRGB`).
enum NewDocumentUnit: String, CaseIterable {
    case px, inches = "in", cm, mm

    /// How many of this unit make one inch; meaningless for `px`, which is
    /// not a physical length and never divides.
    var perInch: Double {
        switch self {
        case .px: return 1
        case .inches: return 1
        case .cm: return 2.54
        case .mm: return 25.4
        }
    }

    var displayName: String {
        switch self {
        case .px: return "Pixels"
        case .inches: return "Inches"
        case .cm: return "Centimetres"
        case .mm: return "Millimetres"
        }
    }
}

/// What a new document's one layer is filled with.
enum NewDocumentBackground: Equatable {
    case white, black, transparent
    /// An explicit opaque colour, written `#rrggbb`.
    case color(UInt8, UInt8, UInt8)

    var name: String {
        switch self {
        case .white: return "white"
        case .black: return "black"
        case .transparent: return "transparent"
        case let .color(r, g, b): return String(format: "#%02x%02x%02x", Int(r), Int(g), Int(b))
        }
    }

    init(name: String) throws {
        let text = name.trimmingCharacters(in: .whitespaces).lowercased()
        switch text {
        case "white": self = .white
        case "black": self = .black
        case "transparent": self = .transparent
        default:
            guard text.hasPrefix("#"), text.count == 7,
                  let value = Int(text.dropFirst(), radix: 16) else {
                throw DoctoredCoreError(
                    message: "background must be transparent, white, black or a #rrggbb colour.")
            }
            self = .color(UInt8((value >> 16) & 0xff), UInt8((value >> 8) & 0xff), UInt8(value & 0xff))
        }
    }

    /// Straight RGBA. Transparent is (0, 0, 0, 0) — the canonical zero-alpha
    /// colour the core writes everywhere else.
    var rgba: (UInt8, UInt8, UInt8, UInt8) {
        switch self {
        case .white: return (255, 255, 255, 255)
        case .black: return (0, 0, 0, 255)
        case .transparent: return (0, 0, 0, 0)
        case let .color(r, g, b): return (r, g, b, 255)
        }
    }

    var isTransparent: Bool { self == .transparent }
}

/// File ▸ New's Color Mode (#547 phases 5 and 1a): an RGB document, a CMYK
/// document of ink in a CMYK profile at 8 or 16 bits, or a Grayscale document
/// in a Gray profile at 8, 16 or 32 bits.
enum NewDocumentColorMode: String, CaseIterable {
    case rgb, cmyk, gray
    /// A Lab Color document (#547 Lab phase 1) at 8 or 16 bits, in the core's Lab twin.
    case lab

    var displayName: String {
        switch self {
        case .rgb: return "RGB Color"
        case .cmyk: return "CMYK Color"
        case .gray: return "Grayscale"
        case .lab: return "Lab Color"
        }
    }
}

/// The colour space a new document's numbers belong to: one of the two
/// built-ins, or an RGB ICC profile on disk — or, for a CMYK document, the
/// working CMYK or a CMYK profile file.
enum NewDocumentProfile: Equatable {
    case srgb, displayP3
    /// The working CMYK profile (Edit ▸ Color Settings), a CMYK document's default.
    case workingCMYK
    /// The working Gray (Edit ▸ Color Settings), a Grayscale document's default (#547 phase 1a).
    case workingGray
    /// One of the built-in Gray spaces, by its wire name.
    case graySpace(GrayWorkingSpace)
    /// A Lab Color document's one profile, the core's Lab twin (#547 Lab phase 1).
    case lab
    /// A path to an ICC profile file.
    case file(String)

    /// The largest profile file this reads — the core's own blob cap, so a
    /// profile that fits here is one the document can carry.
    private static let maximumBytes = 16 * 1024 * 1024

    var name: String {
        switch self {
        case .srgb: return "srgb"
        case .displayP3: return "display_p3"
        case .workingCMYK: return "working"
        case .workingGray: return "working"
        case let .graySpace(space): return space.rawValue
        case .lab: return "lab"
        case let .file(path): return path
        }
    }

    /// A Lab Color document's profile: `lab` (or absent); anything else is refused by name.
    static func lab(name: String) throws -> NewDocumentProfile {
        guard name == "lab" else {
            throw DoctoredCoreError(message: "A Lab Color document's profile is lab.")
        }
        return .lab
    }

    /// True for a built-in Gray space.
    var isGraySpace: Bool {
        if case .graySpace = self { return true }
        return false
    }

    /// A Grayscale document's profile (#547 phase 1a): `working` (the working Gray), a Gray
    /// space's wire name (`dot_gain_20`, `sgray`, …) or the path of a Gray ICC profile; anything
    /// else is refused by name.
    static func gray(name: String) throws -> NewDocumentProfile {
        if ["working", "working_gray"].contains(name) { return .workingGray }
        if let space = GrayWorkingSpace(rawValue: name) { return .graySpace(space) }
        guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: name)), !bytes.isEmpty else {
            throw DoctoredCoreError(message: "Could not read the ICC profile at \(name).")
        }
        guard bytes.count <= maximumBytes, RasterProfile.describe(bytes)?.space == "gray" else {
            throw DoctoredCoreError(message: "\(name) is not a Gray profile; a Grayscale document's profile "
                + "is working, a Gray space or the path of a Gray ICC profile.")
        }
        return .file(name)
    }

    /// A CMYK document's profile (#547 phase 5): `working` (the working CMYK) or the path of a
    /// CMYK ICC profile; anything else is refused by name.
    static func cmyk(name: String) throws -> NewDocumentProfile {
        if ["working", "working_cmyk"].contains(name) { return .workingCMYK }
        guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: name)), !bytes.isEmpty else {
            throw DoctoredCoreError(message: "Could not read the ICC profile at \(name).")
        }
        guard bytes.count <= maximumBytes, RasterProfile.describe(bytes)?.space == "cmyk" else {
            throw DoctoredCoreError(message: "\(name) is not a CMYK profile; a CMYK document's profile is "
                + "working or the path of a CMYK ICC profile.")
        }
        return .file(name)
    }

    /// THROWING, deliberately: a file that has become unreadable since the
    /// request was validated used to answer empty `Data`, and the refusal then
    /// depended on `assigningProfile` rejecting empty bytes — which names
    /// nothing a reader could fix. The one sentence names the path.
    func data() throws -> Data {
        switch self {
        case .srgb: return RasterProfile.builtin(.sRGB)
        case .displayP3: return RasterProfile.builtin(.displayP3)
        case .workingCMYK:
            guard let data = WorkingCMYK.installedData else {
                throw DoctoredCoreError(message: "No working CMYK profile is set (Edit ▸ Color Settings…).")
            }
            return data
        case .workingGray, .graySpace:
            throw DoctoredCoreError(message: "A built-in Gray space is made by the core; it has no profile file.")
        case .lab:
            throw DoctoredCoreError(message: "Lab Color has no profile file; the core makes it.")
        case let .file(path):
            guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: path)), !bytes.isEmpty,
                  bytes.count <= Self.maximumBytes else {
                throw DoctoredCoreError(message: "Could not read the ICC profile at \(path).")
            }
            return bytes
        }
    }

    /// `srgb`, `display_p3`, or a path whose bytes the core accepts as an RGB
    /// profile. A GRAY or CMYK profile is refused BY NAME here rather than
    /// silently replaced: the core stores RGB documents only.
    init(name: String) throws {
        switch name {
        case "srgb", "sRGB": self = .srgb
        case "display_p3", "displayP3", "Display P3": self = .displayP3
        default:
            let url = URL(fileURLWithPath: name)
            guard let bytes = try? Data(contentsOf: url), !bytes.isEmpty else {
                // An unreadable or empty path names ITSELF: "profile must be
                // …" is the answer to a misspelled KEYWORD, not to a file
                // that is missing or that this process cannot open.
                throw DoctoredCoreError(message: "Could not read the ICC profile at \(name).")
            }
            guard bytes.count <= Self.maximumBytes else {
                throw DoctoredCoreError(
                    message: "profile must be srgb, display_p3 or the path of an ICC profile file under 16 MiB.")
            }
            let info = RasterProfile.inspect(bytes)
            guard info.kind.isStorable else {
                throw DoctoredCoreError(message: Self.refusal(info, path: name))
            }
            self = .file(name)
        }
    }

    private static func refusal(_ info: RasterProfileInfo, path: String) -> String {
        let predicate = info.kind.refusalPredicate ?? ""
        switch info.kind {
        case .notICC:
            return "\(path) " + predicate + "."
        case .notRGB:
            return "\(path) is \(info.name), which " + predicate + " — a new document is RGB. "
                + "Grayscale documents are not supported."
        case .notImageProfile:
            return "\(path) is \(info.name), which " + predicate + "."
        default:
            return "\(path) cannot be used as a document profile."
        }
    }
}

/// One New Document request, already in PIXELS: the units are a typing
/// convenience and are resolved before anything is built, so exactly one
/// number describes the canvas from here on.
struct NewDocumentRequest: Equatable {
    /// The window title; nil takes AppKit's own untitled name.
    let name: String?
    let width: Int
    let height: Int
    let ppi: Double
    let profile: NewDocumentProfile
    /// 8, 16 or 32 (32-bit float; see the file's doc comment); 8 or 16 for CMYK.
    let depth: Int
    let background: NewDocumentBackground
    /// RGB or (#547 phase 5) CMYK.
    let colorMode: NewDocumentColorMode

    init(name: String? = nil, width: Int, height: Int, ppi: Double = 300,
         profile: NewDocumentProfile = .srgb, depth: Int = 8,
         background: NewDocumentBackground = .transparent,
         colorMode: NewDocumentColorMode = .rgb) throws {
        guard (1...PrintSize.maxPixelsPerAxis).contains(width),
              (1...PrintSize.maxPixelsPerAxis).contains(height) else {
            throw DoctoredCoreError(
                message: "A new document is 1 to \(PrintSize.maxPixelsPerAxis) pixels on each side.")
        }
        guard width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(
                message: "A new document holds at most \(RasterImage.maxResizePixels) pixels.")
        }
        guard ppi.isFinite, PrintSize.ppiRange.contains(ppi) else {
            throw DoctoredCoreError(message: "ppi must be between 1 and 30000.")
        }
        guard depth == 8 || depth == 16 || depth == 32 else {
            throw DoctoredCoreError(message: "depth must be 8, 16 or 32.")
        }
        switch colorMode {
        case .cmyk:
            guard depth != 32 else {
                throw DoctoredCoreError(message: "A CMYK document is 8 or 16 bits per channel.")
            }
            if [NewDocumentProfile.srgb, .displayP3, .workingGray, .lab].contains(profile)
                || profile.isGraySpace {
                throw DoctoredCoreError(message: "A CMYK document's profile is working or a CMYK ICC profile.")
            }
        case .gray:
            if [NewDocumentProfile.srgb, .displayP3, .workingCMYK, .lab].contains(profile) {
                throw DoctoredCoreError(message: "A Grayscale document's profile is working, a Gray space "
                    + "or a Gray ICC profile.")
            }
        case .lab:
            guard depth != 32 else {
                throw DoctoredCoreError(message: "A Lab Color document is 8 or 16 bits per channel.")
            }
            guard profile == .lab else {
                throw DoctoredCoreError(message: "A Lab Color document's profile is lab.")
            }
        case .rgb:
            if profile == .lab {
                throw DoctoredCoreError(message: "lab is a Lab Color document's profile; pass color_mode lab.")
            }
            if profile == .workingCMYK {
                throw DoctoredCoreError(message: "working is a CMYK document's profile; pass color_mode cmyk.")
            }
            if profile == .workingGray || profile.isGraySpace {
                throw DoctoredCoreError(message: "A Gray space is a Grayscale document's profile; pass "
                    + "color_mode gray.")
            }
        }
        self.colorMode = colorMode
        self.name = name
        self.width = width
        self.height = height
        self.ppi = ppi
        self.profile = profile
        self.depth = depth
        self.background = background
    }

    /// The rule a user notices immediately: a transparent document's one
    /// layer is "Layer 1", a filled one's is "Background".
    var layerName: String { background.isTransparent ? "Layer 1" : "Background" }

    /// A typed length in `unit` as whole pixels. Rounded, then bounded by the
    /// same per-axis maximum the Image Size sheet uses, so a New Document can
    /// never propose a canvas that sheet would refuse.
    static func pixels(_ value: Double, unit: NewDocumentUnit, ppi: Double) throws -> Int {
        guard value.isFinite, value > 0 else {
            throw DoctoredCoreError(message: "width and height must be positive numbers.")
        }
        let raw = unit == .px ? value : value / unit.perInch * ppi
        let rounded = raw.rounded()
        guard rounded >= 1, rounded <= Double(PrintSize.maxPixelsPerAxis) else {
            throw DoctoredCoreError(
                message: "That size is \(Int(rounded.isFinite ? rounded : 0)) pixels on one side; "
                    + "a new document is 1 to \(PrintSize.maxPixelsPerAxis).")
        }
        return Int(rounded)
    }

    /// The length one axis prints at in `unit` — the inverse of `pixels`, so
    /// a preset round-trips its own numbers.
    func length(_ pixels: Int, in unit: NewDocumentUnit) -> Double {
        unit == .px ? Double(pixels) : Double(pixels) / ppi * unit.perInch
    }

    /// Builds the document. Each step answers nil when it would change
    /// nothing (the core's purity rule), which is not a failure here: the
    /// document already says what was asked for.
    func make() throws -> RasterDocument {
        if colorMode == .cmyk { return try makeCMYK() }
        if colorMode == .gray { return try makeGray() }
        if colorMode == .lab { return try makeLab() }
        var document: RasterDocument
        if depth >= 16 {
            let (r, g, b, a) = background.rgba
            document = try RasterDocument.blankRGBA16(width: width, height: height,
                                                      fill: [r, g, b, a].map { UInt16($0) * 257 },
                                                      profile: try profile.data(),
                                                      layerName: layerName)
        } else {
            guard let blank = RasterDocument.blank(width: width, height: height,
                                                   rgba: background.rgba) else {
                throw DoctoredCoreError(message: "Could not create a \(width) × \(height) document.")
            }
            document = blank
            // A byte document is born sRGB; assigning the same profile again
            // answers nil, which is the state that was asked for. A profile
            // FILE is different: falling through there would leave an
            // untagged sRGB document while the reply echoed the profile the
            // caller asked for, so a failure is refused by name.
            let bytes = try profile.data()
            if case let .file(path) = profile {
                guard let tagged = try document.assigningProfile(bytes) else {
                    throw DoctoredCoreError(message: "Could not assign the profile at \(path).")
                }
                document = tagged
            } else {
                document = try document.assigningProfile(bytes) ?? document
            }
            document = try document.withLayerName(0, layerName) ?? document
        }
        document = try document.settingResolution(x: ppi, y: ppi) ?? document
        if depth == 32 {
            // The 16-bit blank through the core's conversion into float: its
            // fill is that conversion's decode (white 1.0, black 0).
            guard let typed = document.precision else {
                throw DoctoredCoreError(message: "Could not create a \(width) × \(height) document.")
            }
            let context = try PrecisionWorkContext(source: typed)
            document = try RasterDocument(precision: typed.promotedToFloat(
                workingLimit: context.limit, otherRetained: context.otherRetained))
        }
        return document
    }

    /// File ▸ New in CMYK (#547 phase 5): one record of `background` — white is bare paper,
    /// transparent alpha 0, black or a colour that colour separated through the NEW document's
    /// profile (`dr_cmyk_color`, Color Settings' intent and BPC) — at this depth and ppi.
    private func makeCMYK() throws -> RasterDocument {
        let icc: Data? = profile == .workingCMYK ? nil : try profile.data()
        let context = try PrecisionWorkContext()
        var request: [String: Any] = ["depth": depth, "layer_name": layerName, "ppi": ppi]
        let paper = try RasterPrecisionDocument.newCMYK(
            width: width, height: height, profile: icc,
            request: request.merging(["fill": [0, 0, 0, 0, background.isTransparent ? 0 : 1]]) { $1 },
            workingLimit: context.limit, otherRetained: context.otherRetained)
        switch background {
        case .white, .transparent:
            return try RasterDocument(precision: paper)
        case .black, .color:
            let (r, g, b, _) = background.rgba
            let ink = try RasterPrecisionDocument.cmykInk(
                fromSRGB: [r, g, b].map { Double($0) / 255 }, document: paper)
            request["fill"] = ink.unit + [1]
            return try RasterDocument(precision: RasterPrecisionDocument.newCMYK(
                width: width, height: height, profile: icc, request: request,
                workingLimit: context.limit, otherRetained: context.otherRetained))
        }
    }

    /// File ▸ New in Grayscale (#547 phase 1a): one record of `background`'s luminance in the
    /// Gray profile (`dr_precision_doc_new_gray`) at this depth and ppi; 32 bits is the 16-bit
    /// blank promoted to float.
    private func makeGray() throws -> RasterDocument {
        let context = try PrecisionWorkContext()
        let (r, g, b, a) = background.rgba
        var request: [String: Any] = [
            "depth": depth, "layer_name": layerName, "ppi": ppi,
            "fill_srgb": [r, g, b, a].map { Double($0) / 255 },
        ]
        var icc: Data?
        switch profile {
        case let .graySpace(space): request["space"] = space.rawValue
        case let .file(path):
            guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: path)), !bytes.isEmpty else {
                throw DoctoredCoreError(message: "Could not read the ICC profile at \(path).")
            }
            icc = bytes
        default: request["space"] = ColorSettings.workingGray.rawValue
        }
        return try RasterDocument(precision: RasterPrecisionDocument.newGray(
            width: width, height: height, profile: icc, request: request,
            workingLimit: context.limit, otherRetained: context.otherRetained))
    }

    /// File ▸ New in Lab Color (#547 Lab phase 1): one record of `background` converted by Image ▸
    /// Mode ▸ Lab Color's rule from sRGB (`dr_precision_doc_new_lab`) at this depth and ppi.
    private func makeLab() throws -> RasterDocument {
        let context = try PrecisionWorkContext()
        let (r, g, b, a) = background.rgba
        let request: [String: Any] = [
            "depth": depth, "layer_name": layerName, "ppi": ppi,
            "fill_srgb": [r, g, b, a].map { Double($0) / 255 },
        ]
        return try RasterDocument(precision: RasterPrecisionDocument.newLab(
            width: width, height: height, request: request,
            workingLimit: context.limit, otherRetained: context.otherRetained))
    }

    /// The MCP reply's echo of what was built.
    var json: [String: Any] {
        ["width": width, "height": height, "ppi": ppi, "profile": profile.name,
         "depth": depth, "background": background.name, "color_mode": colorMode.rawValue]
    }
}

/// One saved New Document preset. The stored width and height are in the
/// preset's OWN unit, so "8 × 10 inches at 300 ppi" reopens as those numbers
/// rather than as 2400 × 3000 pixels.
struct NewDocumentPreset: Equatable {
    static let schemaVersion = 1

    let name: String
    let request: NewDocumentRequest
    let unit: NewDocumentUnit

    var json: [String: Any] {
        var out: [String: Any] = [
            "schema_version": Self.schemaVersion, "name": name,
            "width": request.length(request.width, in: unit),
            "height": request.length(request.height, in: unit),
            "unit": unit.rawValue, "ppi": request.ppi, "profile": request.profile.name,
            "depth": request.depth, "background": request.background.name]
        // #547 phases 5 and 1a: written for a CMYK or Grayscale preset only; absent is RGB.
        if request.colorMode != .rgb { out["color_mode"] = request.colorMode.rawValue }
        return out
    }

    static func decode(_ object: [String: Any]) throws -> NewDocumentPreset {
        let required: Set<String> = ["schema_version", "name", "width", "height", "unit", "ppi",
                                     "profile", "depth", "background"]
        guard Set(object.keys).subtracting(["color_mode"]) == required,
              let mode = NewDocumentColorMode(rawValue: object["color_mode"] as? String ?? "rgb"),
              let name = object["name"] as? String,
              let width = (object["width"] as? NSNumber)?.doubleValue,
              let height = (object["height"] as? NSNumber)?.doubleValue,
              let unitName = object["unit"] as? String, let unit = NewDocumentUnit(rawValue: unitName),
              let ppi = (object["ppi"] as? NSNumber)?.doubleValue,
              let profileName = object["profile"] as? String,
              let depth = (object["depth"] as? NSNumber)?.intValue,
              let backgroundName = object["background"] as? String else {
            throw DoctoredCoreError(message: "Invalid or unsupported document preset.")
        }
        let request = try NewDocumentRequest(
            width: NewDocumentRequest.pixels(width, unit: unit, ppi: ppi),
            height: NewDocumentRequest.pixels(height, unit: unit, ppi: ppi),
            ppi: ppi,
            profile: mode == .cmyk
                ? try NewDocumentProfile.cmyk(name: profileName)
                : mode == .gray ? try NewDocumentProfile.gray(name: profileName)
                : mode == .lab ? try NewDocumentProfile.lab(name: profileName)
                : try NewDocumentProfile(name: profileName),
            depth: depth,
            background: try NewDocumentBackground(name: backgroundName), colorMode: mode)
        return NewDocumentPreset(name: name, request: request, unit: unit)
    }
}

/// The document-preset library's codec on the shared `PreferenceLibrary`
/// contract — the same folder rules, lock, per-entry diagnosis and
/// delete-by-file every other preference library now has.
enum NewDocumentPresetCodec: PreferenceLibraryCodec {
    typealias Item = NewDocumentPreset

    static let noun = "document preset"
    static let directoryEnvironment = "DR_DOCUMENT_PRESETS_DIR"
    static let directoryName = "Doctored/Document Presets"
    static let maximumEntries = 64
    static let maximumFileBytes = 4096
    static let schemaVersions = NewDocumentPreset.schemaVersion...NewDocumentPreset.schemaVersion
    static let didChange = Notification.Name("NewDocumentPresetLibraryDidChange")

    static func name(of item: NewDocumentPreset) -> String { item.name }
    static func json(of item: NewDocumentPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> NewDocumentPreset {
        try NewDocumentPreset.decode(object)
    }
}

/// The bounded versioned New Document preset library. Its writes are
/// PREFERENCES: outside document Undo and never Action steps.
enum NewDocumentPresetLibrary {
    private typealias Library = PreferenceLibrary<NewDocumentPresetCodec>

    static let maximumPresets = NewDocumentPresetCodec.maximumEntries
    static let maximumFileBytes = NewDocumentPresetCodec.maximumFileBytes
    static let didChange = NewDocumentPresetCodec.didChange

    static var directory: URL { Library.directory }

    static func normalizedName(_ name: String) throws -> String {
        try Library.normalizedName(name)
    }

    static func validatedFileName(_ file: String) throws -> String {
        try Library.validatedFileName(file)
    }

    static func listing() throws -> PreferenceLibraryListing<NewDocumentPreset> {
        try Library.listing()
    }

    static func entries() throws -> [NewDocumentPreset] {
        try Library.entries()
    }

    static func named(_ name: String) throws -> NewDocumentPreset {
        try Library.named(name)
    }

    @discardableResult
    static func save(_ preset: NewDocumentPreset, replace: Bool = false) throws -> NewDocumentPreset {
        let resolved = NewDocumentPreset(name: try normalizedName(preset.name),
                                         request: preset.request, unit: preset.unit)
        return try Library.save(resolved, replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }

    static func delete(file: String) throws {
        try Library.delete(file: file)
    }
}

extension ImageDocument {
    /// The fourth document-creating path, beside open, New from Clipboard and
    /// recovery. Deliberately NO `adoptWorkingSpace()`: the request already
    /// names the space its numbers belong to, and converting it into the
    /// working-space preference would silently build a document in a profile
    /// the caller did not ask for.
    static func makeNew(_ request: NewDocumentRequest) throws -> ImageDocument {
        let document = ImageDocument()
        document.doc = try request.make()
        document.activeLayerIndex = 0
        document.refreshProjection()
        // A 16-bit or float document has no flat format that keeps it, so
        // Save offers the native one first; an 8-bit one behaves exactly like
        // New from Clipboard.
        document.fileType = request.depth >= 16 || request.colorMode == .cmyk || request.colorMode == .lab
            ? Self.nativeTypeIdentifier : "public.png"
        if let name = request.name, !name.isEmpty { document.displayName = name }
        document.countEditChange(.changeDone)
        return document
    }
}
