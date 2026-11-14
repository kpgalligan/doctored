import Foundation

// HDR Toning ▸ Preset ▸ Import Preset… (`HdrToningPresetControl`) and its MCP twin
// `import_hdr_toning_preset` (`AgentServer+HdrToning.swift`): a Photoshop `.hdt` HDR Toning preset
// file the user owns becomes one user preset. The core decodes it (`RasterHdrToningPresetFile`,
// every format rule and refusal sentence); this file reads the bounded bytes, names the preset
// and saves it. Every sample file stores the name "Default", so the FILE's name is the preset's.

extension HdrToningPresetLibrary {
    /// The file extension the import reads.
    static let importExtension = "hdt"

    /// The `.hdt` at `url` saved as a user preset named `name`, else the file's name without its
    /// extension; `replace` overwrites a user preset of that name (the popup never passes it).
    /// `report` is the core's lines naming each value imported other than as stored. Every error
    /// starts with the file's name.
    static func importFile(_ url: URL, name: String? = nil,
                           replace: Bool = false) throws -> (preset: HdrToningPreset, report: [String]) {
        do {
            let decoded = try RasterHdrToningPresetFile.decode(read(url))
            let saved = try save(name: name ?? url.deletingPathExtension().lastPathComponent,
                                 arguments: decoded.arguments, replace: replace)
            return (saved, decoded.report)
        } catch {
            throw DoctoredCoreError(message: "\(url.lastPathComponent): \(error.localizedDescription)")
        }
    }

    /// At most one byte past the core's cap, so an oversized file reaches the core's own refusal
    /// without being read whole.
    private static func read(_ url: URL) throws -> Data {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: RasterHdrToningPresetFile.maximumBytes + 1) ?? Data()
    }

    /// The open panel's starting folder: the newest installed Photoshop's `Presets/HDR Toning`,
    /// else its `Presets`, else nil.
    static func installedPresetsDirectory() -> URL? {
        guard let presets = PhotoshopAdjustmentPreset.installedPresetsDirectory() else { return nil }
        let folder = presets.appendingPathComponent("HDR Toning", isDirectory: true)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: folder.path, isDirectory: &isDirectory),
              isDirectory.boolValue
        else { return presets }
        return folder
    }
}
