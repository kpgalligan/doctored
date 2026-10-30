import AppKit

/// STYLE PRESETS, the Window ▸ Styles panel's model — a named
/// layer style (and, when it was saved with them, the layer's blending mode
/// and opacity) in an optional GROUP. Twelve original built-ins live in the
/// core (`core/src/style_presets.rs`) in the read-only group "Default
/// Styles"; the user's own presets are a bounded `PreferenceLibrary`
/// (`DR_STYLE_PRESETS_DIR`). `StylesPanelController` is the panel,
/// `AgentServer+StylePresets.swift` the MCP twin.
///
/// A preset's style is the core's PRESET form (`RasterStylePresets.canonical`:
/// strict, no CMYK inks — a preset applies to documents of every mode — and no
/// pattern anchors — the setter re-anchors on the layer it lands on). A name
/// and a group hold no "/", because the library's key is `group/name`.
///
/// Library writes are PREFERENCES: no undo step, no dirty flag, never an
/// Action step. APPLYING a preset is an edit: one undo step, recorded as
/// `apply_style_preset`.
struct StylePresetBlending: Equatable {
    /// The style JSON's blend name (`LayerStyleBlend`).
    var mode: String
    /// 0…1.
    var opacity: Double

    var json: [String: Any] { ["mode": mode, "opacity": opacity] }

    /// Normal at 100 % changes nothing on a fresh layer.
    var isDefault: Bool { mode == "normal" && opacity >= 1 }

    /// `{mode, opacity}` with a known blend name and an opacity in 0…1, or
    /// nil for an absent or null value; anything else throws.
    static func decode(_ raw: Any?) throws -> StylePresetBlending? {
        guard let raw, !(raw is NSNull) else { return nil }
        guard let object = raw as? [String: Any], Set(object.keys) == ["mode", "opacity"],
              let mode = object["mode"] as? String, LayerStyleBlend.mode(named: mode) != nil,
              let number = object["opacity"] as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID(),
              (0...1).contains(number.doubleValue)
        else {
            throw DoctoredCoreError(message: "A style preset's blending is {mode, opacity}: a layer "
                + "blend name and an opacity from 0 to 1.")
        }
        return StylePresetBlending(mode: mode, opacity: number.doubleValue)
    }
}

struct StylePreset {
    static let schemaVersion = 1
    static let builtinGroup = "Default Styles"
    static let maximumNameLength = 72
    static let maximumGroupLength = 40
    /// The style's blending options: kept from the TARGET when a preset
    /// without blending replaces a layer's style.
    static let blendingKeys = ["fill_opacity", "blend_if", "knockout", "blend_interior_as_group"]

    let name: String
    let group: String?
    /// The canonical preset style JSON.
    let style: String
    let blending: StylePresetBlending?
    let builtin: Bool

    /// The library's key: `group/name`, or the name alone.
    var key: String { group.map { "\($0)/\(name)" } ?? name }

    var styleObject: [String: Any] { Self.object(style) ?? [:] }

    /// The effect `type`s, in render order.
    var effectTypes: [String] {
        (styleObject["effects"] as? [[String: Any]] ?? []).compactMap { $0["type"] as? String }
    }

    /// The on-disk object (`StylePresetCodec`).
    var json: [String: Any] {
        var out: [String: Any] = ["schema_version": Self.schemaVersion, "name": name,
                                  "style": styleObject]
        if let group { out["group"] = group }
        if let blending { out["blending"] = blending.json }
        return out
    }

    // MARK: - Names

    private typealias Library = PreferenceLibrary<StylePresetCodec>

    static func object(_ json: String) -> [String: Any]? {
        (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any]
    }

    static func text(_ object: [String: Any]) throws -> String {
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8)
        else { throw DoctoredCoreError(message: "The style is not a JSON object.") }
        return text
    }

    /// `name` trimmed and normalized, 1…72 characters, no control character
    /// and no "/".
    static func normalized(name: String) throws -> String {
        let normalized = try? Library.normalizedName(name)
        guard let normalized, normalized.count <= maximumNameLength else {
            throw DoctoredCoreError(message: "Use a style name of 1–\(maximumNameLength) characters "
                + "without control characters.")
        }
        guard !normalized.contains("/") else {
            throw DoctoredCoreError(message: "A style name cannot contain “/”.")
        }
        return normalized
    }

    /// nil or empty is "no group"; else 1…40 characters, no control
    /// character and no "/". Not checked against the built-in group here.
    static func normalized(group: String?) throws -> String? {
        guard let group, !group.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }
        let normalized = try? Library.normalizedName(group)
        guard let normalized, normalized.count <= maximumGroupLength else {
            throw DoctoredCoreError(message: "Use a style group name of 1–\(maximumGroupLength) "
                + "characters without control characters.")
        }
        guard !normalized.contains("/") else {
            throw DoctoredCoreError(message: "A style group name cannot contain “/”.")
        }
        return normalized
    }

    /// Whether `group` is the built-in group, compared as the library
    /// compares keys (case-insensitively).
    static func isBuiltinGroup(_ group: String?) -> Bool {
        group.map { Library.key($0) == Library.key(builtinGroup) } ?? false
    }

    static let reservedGroupRefusal = "“\(builtinGroup)” is the built-in style group; choose "
        + "another group name."

    /// `name`, `group` and `style` checked and canonicalized: what every save
    /// goes through, so nothing the library writes is unloadable.
    static func validated(name: String, group: String?, style: String,
                          blending: StylePresetBlending?) throws -> StylePreset {
        let name = try normalized(name: name)
        let group = try normalized(group: group)
        guard !isBuiltinGroup(group) else { throw DoctoredCoreError(message: reservedGroupRefusal) }
        return StylePreset(name: name, group: group, style: try RasterStylePresets.canonical(style),
                           blending: blending, builtin: false)
    }

    /// Strict: exactly `{schema_version, name, group?, style, blending?}`, and
    /// a style the core's canonicalizer gives back unchanged.
    static func decode(_ object: [String: Any]) throws -> StylePreset {
        let refusal = DoctoredCoreError(message: "Invalid or unsupported style preset.")
        let keys = Set(object.keys)
        guard keys.isSubset(of: ["schema_version", "name", "group", "style", "blending"]),
              keys.isSuperset(of: ["schema_version", "name", "style"]),
              let name = object["name"] as? String,
              object["group"] == nil || object["group"] is String,
              let style = object["style"] as? [String: Any]
        else { throw refusal }
        let group = object["group"] as? String
        let preset = try validated(name: name, group: group, style: try text(style),
                                   blending: try StylePresetBlending.decode(object["blending"]))
        guard preset.name == name, preset.group == group,
              let canonical = Self.object(preset.style),
              NSDictionary(dictionary: canonical).isEqual(to: style)
        else { throw refusal }
        return preset
    }

    // MARK: - Layers

    /// nil when layer `layer` can hold a style; else the refusal sentence.
    /// The ONE eligibility rule the panel and `apply_style_preset` share.
    static func ineligibility(_ doc: RasterDocument, layer: Int) -> String? {
        guard layer >= 0, layer < doc.layerCount, doc.layerInfo(layer) != nil else {
            return "There is no layer \(layer)."
        }
        // Every layer holds one: an adjustment layer (its mask over the
        // canvas is the effects' shape; a CMYK one included), and a Pass
        // Through group, at every depth.
        return nil
    }

    /// Whether a style object renders anything (the core's identity rule,
    /// restated for the "nothing to save" refusal).
    static func rendersSomething(_ object: [String: Any]) -> Bool {
        let effects = object["effects"] as? [[String: Any]] ?? []
        if effects.contains(where: { ($0["enabled"] as? Bool) ?? true }) { return true }
        if let fill = (object["fill_opacity"] as? NSNumber)?.doubleValue, fill < 1 { return true }
        if let knockout = object["knockout"] as? String, knockout != "none" { return true }
        if let blendIf = object["blend_if"], !(blendIf is NSNull) { return true }
        return false
    }

    /// New Style… from layer `layer` (the dialog's two options): without
    /// effects the style's effects are dropped; without blending its
    /// blending options reset and no mode or opacity is kept; with blending
    /// the layer's mode and opacity are captured. Saving nothing refuses.
    static func fromLayer(_ doc: RasterDocument, layer: Int, name: String, group: String?,
                          includeEffects: Bool, includeBlending: Bool) throws -> StylePreset {
        if let refusal = ineligibility(doc, layer: layer) {
            throw DoctoredCoreError(message: refusal)
        }
        var object = doc.layerStyle(layer).flatMap(Self.object) ?? [:]
        if !includeEffects { object["effects"] = [Any]() }
        var blending: StylePresetBlending?
        if includeBlending {
            if let info = doc.layerInfo(layer) {
                // Four decimals, as every style number is stored: the layer's
                // opacity is a Float in the core.
                blending = StylePresetBlending(mode: LayerStyleBlend.name(for: info.blendMode),
                                               opacity: (info.opacity * 10000).rounded() / 10000)
            }
        } else {
            for key in blendingKeys { object.removeValue(forKey: key) }
        }
        guard rendersSomething(object) || !(blending?.isDefault ?? true) else {
            throw DoctoredCoreError(message: "Layer \(layer) has nothing to save as a style: "
                + (includeEffects ? "no effect, " : "effects are excluded, ")
                + (includeBlending ? "and its blending is the default." : "and blending options are excluded."))
        }
        return try validated(name: name, group: group, style: try text(object), blending: blending)
    }

    /// This preset on `layers`, bottom first, as ONE document (nil when no
    /// layer changed). Replace sets the preset's style (a preset without
    /// blending keeps each target's own fill, Blend If, knockout and interior
    /// option; one with blending also sets the layer's mode and opacity). Add
    /// (shift-click) merges the effects into the current style: a repeating
    /// kind appends on top — the core refuses an eleventh by its own sentence
    /// — and a single kind replaces; blending is left alone.
    func applied(to doc: RasterDocument, layers: [Int], add: Bool) throws -> RasterDocument? {
        let preset = styleObject
        var current = doc
        for index in layers.sorted() {
            let existing = current.layerStyle(index).flatMap(Self.object)
            if add {
                let merged = Self.merged(existing ?? [:], adding: preset)
                // nil is "this step changed nothing", never "stop": fold on.
                current = try current.withLayerStyle(index, try Self.text(merged)) ?? current
                continue
            }
            var wanted = preset
            if blending == nil {
                for key in Self.blendingKeys {
                    wanted[key] = existing?[key]
                }
            }
            current = try current.withLayerStyle(index, try Self.text(wanted)) ?? current
            if let blending, let mode = LayerStyleBlend.mode(named: blending.mode) {
                current = try current.withLayerBlendMode(index, mode) ?? current
                current = try current.withLayerOpacity(index, blending.opacity) ?? current
            }
        }
        return current === doc ? nil : current
    }

    /// `base`'s effects with `adding`'s merged in (`applied`'s Add).
    static func merged(_ base: [String: Any], adding preset: [String: Any]) -> [String: Any] {
        var out = base
        var effects = base["effects"] as? [[String: Any]] ?? []
        for effect in preset["effects"] as? [[String: Any]] ?? [] {
            let type = effect["type"] as? String ?? ""
            if let kind = LayerStyleEffectKind(rawValue: type), !kind.repeats {
                effects.removeAll { $0["type"] as? String == type }
            }
            effects.append(effect)
        }
        out["effects"] = effects
        return out
    }
}

/// The style-preset library's codec on the shared `PreferenceLibrary` contract.
enum StylePresetCodec: PreferenceLibraryCodec {
    typealias Item = StylePreset

    static let noun = "style preset"
    static let directoryEnvironment = "DR_STYLE_PRESETS_DIR"
    static let directoryName = "Doctored/Style Presets"
    static let maximumEntries = 256
    /// 2.25 MiB: a style holding a 512 × 512 custom pattern is a 1 MiB tile,
    /// 1.33 MiB in base64, with room for its effects.
    static let maximumFileBytes = 2_359_296
    static let schemaVersions = StylePreset.schemaVersion...StylePreset.schemaVersion
    static let didChange = Notification.Name("StylePresetLibraryDidChange")

    static func name(of item: StylePreset) -> String { item.key }
    static func json(of item: StylePreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> StylePreset {
        try StylePreset.decode(object)
    }
}

/// The built-ins plus the bounded user library. Writes are PREFERENCES.
enum StylePresetLibrary {
    private typealias Library = PreferenceLibrary<StylePresetCodec>

    static let maximumPresets = StylePresetCodec.maximumEntries
    static let maximumFileBytes = StylePresetCodec.maximumFileBytes
    static let didChange = StylePresetCodec.didChange

    static var builtins: [StylePreset] {
        RasterStylePresets.defaults().map {
            StylePreset(name: $0.name, group: StylePreset.builtinGroup, style: $0.style,
                        blending: nil, builtin: true)
        }
    }

    /// Every preset with its library file (nil for a built-in), in panel
    /// order: the built-ins, the user groups A–Z, then the ungrouped
    /// presets; names A–Z within a group.
    static func listing() throws
        -> (presets: [(preset: StylePreset, file: String?)], broken: [PreferenceLibraryBroken])
    {
        let listing = try Library.listing()
        let user = listing.entries.sorted { a, b in
            switch (a.item.group, b.item.group) {
            case (nil, .some): return false
            case (.some, nil): return true
            case let (.some(x), .some(y)) where Library.key(x) != Library.key(y):
                return x.localizedStandardCompare(y) == .orderedAscending
            default:
                return a.item.name.localizedStandardCompare(b.item.name) == .orderedAscending
            }
        }
        let rows: [(preset: StylePreset, file: String?)] =
            builtins.map { ($0, nil) } + user.map { ($0.item, $0.file) }
        return (rows, listing.broken)
    }

    static func all() throws -> (presets: [StylePreset], broken: [PreferenceLibraryBroken]) {
        let listing = try listing()
        return (listing.presets.map(\.preset), listing.broken)
    }

    /// The groups in panel order, the built-in group first.
    static func groups(_ presets: [StylePreset]) -> [String] {
        var seen = Set<String>()
        return presets.compactMap(\.group).filter { seen.insert(Library.key($0)).inserted }
    }

    private static func missing(name: String, group: String?) -> DoctoredCoreError {
        DoctoredCoreError(message: "No style preset named “\(name)” "
            + (group.map { "in the group “\($0)”." } ?? "outside a group."))
    }

    private static func readOnly(_ preset: StylePreset) -> DoctoredCoreError {
        DoctoredCoreError(message: "“\(preset.name)” is a built-in style in “\(StylePreset.builtinGroup)”, "
            + "which cannot be changed; save a copy under another name or group.")
    }

    /// The preset `name` in `group` (nil: an ungrouped preset, else the
    /// built-in of that name).
    static func preset(name: String, group: String?) throws -> StylePreset {
        let name = try StylePreset.normalized(name: name)
        let group = try StylePreset.normalized(group: group)
        let presets = try all().presets
        let matches = { (preset: StylePreset, wanted: String?) -> Bool in
            Library.key(preset.name) == Library.key(name)
                && preset.group.map(Library.key) == wanted.map(Library.key)
        }
        if let found = presets.first(where: { matches($0, group) }) { return found }
        if group == nil,
           let builtin = presets.first(where: { $0.builtin && matches($0, StylePreset.builtinGroup) }) {
            return builtin
        }
        throw missing(name: name, group: group)
    }

    private static func stored(name: String, group: String?) throws -> StylePreset? {
        try Library.entries().first {
            Library.key($0.key) == Library.key(group.map { "\($0)/\(name)" } ?? name)
        }
    }

    @discardableResult
    static func save(_ preset: StylePreset, replace: Bool) throws -> StylePreset {
        guard !preset.builtin else { throw readOnly(preset) }
        let checked = try StylePreset.validated(name: preset.name, group: preset.group,
                                                style: preset.style, blending: preset.blending)
        if !replace, try stored(name: checked.name, group: checked.group) != nil {
            throw DoctoredCoreError(message: "A style preset named “\(checked.name)” already exists "
                + (checked.group.map { "in the group “\($0)”." } ?? "outside a group."))
        }
        return try Library.save(checked, replace: replace)
    }

    /// Rename and Move to Group: `newGroup` nil ungroups. A change of case
    /// alone writes the entry's own file in place.
    @discardableResult
    static func rename(name: String, group: String?, to newName: String, group newGroup: String?)
        throws -> StylePreset
    {
        let old = try preset(name: name, group: group)
        guard !old.builtin else { throw readOnly(old) }
        let renamed = try StylePreset.validated(name: newName, group: newGroup, style: old.style,
                                                blending: old.blending)
        if Library.key(old.key) == Library.key(renamed.key) {
            return try Library.save(renamed, replace: true)
        }
        let saved = try save(renamed, replace: false)
        try Library.delete(name: old.key)
        return saved
    }

    static func delete(name: String, group: String?) throws {
        let found = try preset(name: name, group: group)
        guard !found.builtin else { throw readOnly(found) }
        try Library.delete(name: found.key)
    }

    static func delete(file: String) throws {
        try Library.delete(file: file)
    }

    /// The largest `.asl` an import reads (the core's `MAX_ASL_BYTES`).
    static let maximumLibraryBytes = 64 << 20

    /// The bytes of the style library at `url`, refused by size before a
    /// byte is read, with the core's sentence.
    static func readASL(_ url: URL) throws -> Data {
        let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?
            .intValue ?? 0
        guard size <= maximumLibraryBytes else {
            throw DoctoredCoreError(message: "style library is \(size) bytes, over the 64 MiB limit")
        }
        do {
            return try Data(contentsOf: url)
        } catch {
            throw DoctoredCoreError(message: "Could not read \(url.lastPathComponent).")
        }
    }

    /// The group an import of `url` defaults to: the file's basename with
    /// "/" as "-", cut to 40 characters ("Imported Styles" when nothing is
    /// left); the built-in group's name becomes "Default Styles 2".
    static func defaultGroup(for url: URL) -> String {
        let base = url.deletingPathExtension().lastPathComponent
        var group = String(sanitized(base).prefix(StylePreset.maximumGroupLength))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if group.isEmpty { group = "Imported Styles" }
        if StylePreset.isBuiltinGroup(group) { group = StylePreset.builtinGroup + " 2" }
        return group
    }

    /// "/" as "-", control characters dropped, trimmed.
    static func sanitized(_ text: String) -> String {
        let cleaned = text.replacingOccurrences(of: "/", with: "-").unicodeScalars
            .filter { !CharacterSet.controlCharacters.contains($0) }
        return String(String.UnicodeScalarView(cleaned)).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Writes `presets` as a Photoshop style library at `url` (Export
    /// Styles… / Export Group… and MCP `export_style_presets`), atomically:
    /// how many were written and a "<name>: <reason>" for each one the core
    /// could not write. Throws the core's sentence when none can be, or the
    /// write's failure.
    static func exportASL(_ presets: [StylePreset], to url: URL) throws
        -> (exported: Int, skipped: [String])
    {
        let (data, skipped) = try RasterStylePresets.encodeASL(presets)
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            throw DoctoredCoreError(message: "Could not write \(url.lastPathComponent).")
        }
        return (presets.count - skipped.count, skipped)
    }

    /// Imports a Photoshop style library into `group` (refused when it is the
    /// built-in group). Each style's name has "/" as "-" and is cut to 72
    /// characters; a name the group already holds gets " 2", " 3"…. A style
    /// with nothing Doctored models is `skipped`; every effect or blending
    /// option the core could not keep is a `dropped` "<name>: <sentence>".
    static func importASL(_ data: Data, group: String) throws
        -> (imported: [StylePreset], dropped: [String], skipped: [String])
    {
        guard let group = try StylePreset.normalized(group: group) else {
            throw DoctoredCoreError(message: "Name the style group to import into.")
        }
        guard !StylePreset.isBuiltinGroup(group) else {
            throw DoctoredCoreError(message: StylePreset.reservedGroupRefusal)
        }
        let report = try RasterStylePresets.decodeASL(data)
        var taken = Set(try Library.entries().filter {
            $0.group.map(Library.key) == Library.key(group)
        }.map { Library.key($0.name) })
        var imported: [StylePreset] = [], dropped: [String] = [], skipped: [String] = []
        for entry in report["styles"] as? [[String: Any]] ?? [] {
            var base = String(sanitized(entry["name"] as? String ?? "")
                .prefix(StylePreset.maximumNameLength)).trimmingCharacters(in: .whitespaces)
            if base.isEmpty { base = "Style" }
            let sentences = entry["dropped"] as? [String] ?? []
            guard let style = entry["style"] as? [String: Any] else {
                dropped.append(contentsOf: sentences.map { "\(base): \($0)" })
                skipped.append("\(base): nothing Doctored models is left")
                continue
            }
            var name = base, suffix = 2
            while taken.contains(Library.key(name)) {
                let tail = " \(suffix)"
                name = String(base.prefix(StylePreset.maximumNameLength - tail.count))
                    .trimmingCharacters(in: .whitespaces) + tail
                suffix += 1
            }
            // Named by the style's final name, suffix included.
            dropped.append(contentsOf: sentences.map { "\(name): \($0)" })
            do {
                let preset = try StylePreset.validated(
                    name: name, group: group, style: try StylePreset.text(style),
                    blending: try StylePresetBlending.decode(entry["blending"]))
                imported.append(try Library.save(preset, replace: false))
                taken.insert(Library.key(name))
            } catch {
                skipped.append("\(base): \(error.localizedDescription)")
            }
        }
        return (imported, dropped, skipped)
    }
}
