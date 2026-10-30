import Foundation

/// The user's own adjustment presets — a named params object
/// for one adjustment op, saved from a dialog's preset row (or MCP) and
/// loaded back into it. A bounded `PreferenceLibrary`
/// (`DR_ADJUSTMENT_PRESETS_DIR`); `AgentServer+AdjustmentPresets.swift` is
/// the MCP twin.
///
/// Names are unique PER OP: the library keys an entry by `"<op>: <name>"`
/// (`AdjustmentPresetCodec.name(of:)`), so a Curves "Punch" and a Levels
/// "Punch" are two presets. The params are exactly what the op's layer meta
/// and `apply_filter` take, minus `evaluation` (a preset is a look, the
/// document decides how it is evaluated), and with `bcs`'s `legacy` always
/// spelled out (`validated`). They are valid iff the core's destructive twin
/// (`dr_image_adjust_op`) accepts them on a 1×1 image.
///
/// Library writes are PREFERENCES: no undo step, no dirty flag, never an
/// Action step.
struct AdjustmentPreset {
    static let schemaVersion = 1
    /// The longest preset name. The library caps its KEY, `"<op>: <name>"`,
    /// at 120 characters (`PreferenceLibrary.normalizedName`), and the
    /// longest op prefix is `"shadows_highlights: "`, 20 — so 100 is the
    /// length every op can store, and the one the refusal and the catalog
    /// state.
    static let maximumNameLength = 100

    let name: String
    let op: AdjustmentLayerOp
    let params: [String: Any]
    /// #1 phase 5: a CMYK document's Levels or Curves preset (ink channels, `cmyk_adjust`'s
    /// keys), listed only by a CMYK document's dialog. Stored as `"color_mode": "cmyk"`; an RGB
    /// preset carries no key, so every earlier file reads unchanged.
    var cmyk = false

    var json: [String: Any] {
        var out: [String: Any] = ["schema_version": Self.schemaVersion, "name": name, "op": op.rawValue,
                                  "params": params]
        if cmyk { out["color_mode"] = "cmyk" }
        return out
    }

    /// The preset as a dialog loads it.
    var payload: AdjustmentLayerPayload { AdjustmentLayerPayload(op: op, params: params) }

    /// `name` normalized and `params` validated for `op` — what every save
    /// goes through, so nothing the library writes is unloadable.
    static func validated(
        name: String, op: AdjustmentLayerOp, params: [String: Any], cmyk: Bool = false
    ) throws -> AdjustmentPreset {
        let normalized = try normalizedName(name)
        guard AdjustmentPresetLibrary.supportedOps.contains(op) else {
            throw DoctoredCoreError(message: "\(op.rawValue) has no adjustment presets; "
                + "presets are for " + AdjustmentPresetLibrary.supportedOpNames + ".")
        }
        var stripped = params
        stripped.removeValue(forKey: "evaluation")
        // `bcs` with no `legacy` is the LEGACY curve in a layer's meta and in
        // `apply_filter`, while a new dialog with no key opens modern.
        // A stored preset therefore spells the form out, so the dialog, the
        // layer and the filter all read the one curve. A library file
        // written without the key decodes through here too.
        if op == .bcs, stripped["legacy"] == nil { stripped["legacy"] = true }
        // A CMYK preset holds ink channels the RGB twin cannot take; its keys are checked
        // instead (`CmykToneTools.acceptsPreset`), and the core refuses the rest on use.
        let accepted = cmyk ? CmykToneTools.acceptsPreset(op, stripped) : accepts(op, stripped)
        guard JSONSerialization.isValidJSONObject(stripped), accepted else {
            throw DoctoredCoreError(message: "The \(op.rawValue) parameters are not valid; an "
                + "adjustment preset holds exactly what add_adjustment_layer takes for that op.")
        }
        return AdjustmentPreset(name: normalized, op: op, params: stripped, cmyk: cmyk)
    }

    /// A preset's own name, normalized as the library normalizes its key
    /// and held to `maximumNameLength`, with the sentence that says so: the
    /// library's own refusal counts the `"<op>: "` prefix and states 120.
    static func normalizedName(_ name: String) throws -> String {
        let refusal = DoctoredCoreError(
            message: "Use an adjustment preset name of 1–\(maximumNameLength) characters "
                + "without control characters.")
        // 492: the library's 512 UTF-8 bytes less the same 20-byte prefix.
        guard let normalized = try? PreferenceLibrary<AdjustmentPresetCodec>.normalizedName(name),
              normalized.count <= maximumNameLength, normalized.utf8.count <= 492
        else { throw refusal }
        return normalized
    }

    /// The validity test: the ONE destructive twin, on a 1×1 grey.
    private static func accepts(_ op: AdjustmentLayerOp, _ params: [String: Any]) -> Bool {
        RasterImage.from(rgba: [128, 128, 128, 255], width: 1, height: 1)?
            .applyingAdjustment(op: op.rawValue, params: params) != nil
    }

    /// Strict: exactly `{schema_version, name, op, params}`, a supported op
    /// and params the op accepts. Anything else throws, and the library
    /// lists the file as one `invalid_schema` row.
    static func decode(_ object: [String: Any]) throws -> AdjustmentPreset {
        let keys = Set(object.keys)
        let cmyk = object["color_mode"] as? String == "cmyk"
        guard keys.subtracting(["color_mode"]) == ["schema_version", "name", "op", "params"],
              object["color_mode"] == nil || cmyk,
              let name = object["name"] as? String,
              let opName = object["op"] as? String, let op = AdjustmentLayerOp(rawValue: opName),
              let params = object["params"] as? [String: Any]
        else { throw DoctoredCoreError(message: "Invalid or unsupported adjustment preset.") }
        let preset = try validated(name: name, op: op, params: params, cmyk: cmyk)
        guard preset.name == name, params["evaluation"] == nil else {
            throw DoctoredCoreError(message: "Invalid or unsupported adjustment preset.")
        }
        return preset
    }
}

/// The adjustment-preset library's codec on the shared `PreferenceLibrary`
/// contract.
enum AdjustmentPresetCodec: PreferenceLibraryCodec {
    typealias Item = AdjustmentPreset

    static let noun = "adjustment preset"
    static let directoryEnvironment = "DR_ADJUSTMENT_PRESETS_DIR"
    static let directoryName = "Doctored/Adjustment Presets"
    static let maximumEntries = 256
    static let maximumFileBytes = 16384
    static let schemaVersions = AdjustmentPreset.schemaVersion...AdjustmentPreset.schemaVersion
    static let didChange = Notification.Name("AdjustmentPresetLibraryDidChange")

    /// `"<op>: <name>"`: the library's uniqueness key, so names are unique
    /// per op.
    static func name(of item: AdjustmentPreset) -> String { "\(item.op.rawValue): \(item.name)" }
    static func json(of item: AdjustmentPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> AdjustmentPreset {
        try AdjustmentPreset.decode(object)
    }
}

/// The bounded adjustment-preset library. Its writes are PREFERENCES:
/// outside document Undo and never Action steps.
enum AdjustmentPresetLibrary {
    private typealias Library = PreferenceLibrary<AdjustmentPresetCodec>

    static let maximumPresets = AdjustmentPresetCodec.maximumEntries
    static let maximumFileBytes = AdjustmentPresetCodec.maximumFileBytes
    static let didChange = AdjustmentPresetCodec.didChange

    /// Every op with a dialog except Color Lookup (its table is a file, not
    /// a look you dial in), the three legacy slider ops (Hue Rotate,
    /// Threshold, Posterize) and the parameterless three.
    static let supportedOps: Set<AdjustmentLayerOp> = [
        .bcs, .curves, .levels, .exposure, .vibrance, .hueSaturation, .colorBalance,
        .blackAndWhite, .photoFilter, .channelMixer, .selectiveColor, .shadowsHighlights,
        .whiteBalance, .gradientMap, .replaceColor,
    ]

    /// The supported op names, sorted, for refusals and the catalog.
    static var supportedOpNames: String {
        supportedOps.map(\.rawValue).sorted().joined(separator: ", ")
    }

    static func listing() throws -> PreferenceLibraryListing<AdjustmentPreset> {
        try Library.listing()
    }

    /// `op`'s presets, in the listing's name order: RGB ones, or (#1 phase 5) a CMYK
    /// document's.
    static func presets(for op: AdjustmentLayerOp, cmyk: Bool = false) throws -> [AdjustmentPreset] {
        try listing().entries.map(\.item).filter { $0.op == op && $0.cmyk == cmyk }
    }

    @discardableResult
    static func save(_ preset: AdjustmentPreset, replace: Bool) throws -> AdjustmentPreset {
        let checked = try AdjustmentPreset.validated(
            name: preset.name, op: preset.op, params: preset.params, cmyk: preset.cmyk)
        // Asked here so the refusal names the op and the preset's own name,
        // not the library's `"<op>: <name>"` key; the library asks again
        // under its lock.
        let existing = try stored(op: checked.op, name: checked.name)
        // RGB and CMYK presets share one key per op and name, and each dialog lists only its
        // own mode's, so a name the other mode holds refuses even with Replace (#1 phase 5).
        if let existing, existing.cmyk != checked.cmyk {
            throw DoctoredCoreError(message: "The name “\(checked.name)” is taken by "
                + "\(existing.cmyk ? "a CMYK" : "an RGB") \(checked.op.rawValue) preset.")
        }
        if !replace, existing != nil {
            throw DoctoredCoreError(message: "An adjustment preset named “\(checked.name)” "
                + "already exists for \(checked.op.rawValue).")
        }
        return try Library.save(checked, replace: replace)
    }

    /// `op`'s preset under `name` (normalized), compared as the library
    /// compares its keys.
    private static func stored(op: AdjustmentLayerOp, name: String) throws -> AdjustmentPreset? {
        try listing().entries.map(\.item).filter { $0.op == op }
            .first { Library.key($0.name) == Library.key(name) }
    }

    private static func missing(op: AdjustmentLayerOp, name: String) -> DoctoredCoreError {
        DoctoredCoreError(message: "No \(op.rawValue) adjustment preset named “\(name)”.")
    }

    /// `save` under the new name, then `delete` the old one; a rename that
    /// changes only case writes the entry's own file in place.
    @discardableResult
    static func rename(op: AdjustmentLayerOp, name: String, to newName: String) throws
        -> AdjustmentPreset
    {
        let old = try AdjustmentPreset.normalizedName(name)
        let new = try AdjustmentPreset.normalizedName(newName)
        guard let existing = try stored(op: op, name: old) else {
            throw missing(op: op, name: old)
        }
        let renamed = AdjustmentPreset(name: new, op: op, params: existing.params, cmyk: existing.cmyk)
        if Library.key(old) == Library.key(new) {
            return try save(renamed, replace: true)
        }
        let saved = try save(renamed, replace: false)
        try delete(op: op, name: existing.name)
        return saved
    }

    static func delete(op: AdjustmentLayerOp, name: String) throws {
        let normalized = try AdjustmentPreset.normalizedName(name)
        // Asked here for the same reason `save` asks: the library's own
        // refusal would quote its key.
        guard try stored(op: op, name: normalized) != nil else {
            throw missing(op: op, name: normalized)
        }
        try Library.delete(name: "\(op.rawValue): \(normalized)")
    }

    static func delete(file: String) throws {
        try Library.delete(file: file)
    }
}
