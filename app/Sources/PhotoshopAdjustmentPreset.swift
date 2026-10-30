import Foundation

/// Photoshop's adjustment PRESET files on the host side: the
/// six extensions Layer ▸ New Adjustment Layer ▸ From Photoshop Preset… and
/// `read_photoshop_adjustment_preset` accept, the bounded file read, and
/// where an installed Photoshop keeps its shipped presets. Decoding is the
/// core's (`RasterAdjustmentPresetFile`); nothing here interprets a byte.
enum PhotoshopAdjustmentPreset {
    /// Curves, Levels, Hue/Saturation, Exposure, Black & White, Channel Mixer.
    static let fileExtensions = ["acv", "alv", "ahu", "eap", "blw", "cha"]

    /// The two a CMYK document reads (#547 phase 5): Levels and Curves with ink channels.
    static let cmykFileExtensions = ["acv", "alv"]

    /// The adjustment `url` holds. Reads at most one byte past the core's
    /// 64 KiB cap, so an oversized file is refused by the core's own sentence
    /// without ever being read whole. Errors carry the file name. `cmyk` reads
    /// the file's ink channels for a CMYK document (`decodeCMYK`).
    static func read(_ url: URL, cmyk: Bool = false) throws -> AdjustmentLayerPayload {
        let name = url.lastPathComponent
        let bytes: Data
        do {
            let handle = try FileHandle(forReadingFrom: url)
            defer { try? handle.close() }
            // One byte past the core's 64 KiB cap (`adjustment_preset_file::
            // MAX_FILE_BYTES`), so an oversize file reaches the core's refusal.
            bytes = try handle.read(upToCount: 65_537) ?? Data()
        } catch {
            throw DoctoredCoreError(message: "\(name): \(error.localizedDescription)")
        }
        do {
            return cmyk
                ? try RasterAdjustmentPresetFile.decodeCMYK(bytes, extension: url.pathExtension)
                : try RasterAdjustmentPresetFile.decode(bytes, extension: url.pathExtension)
        } catch {
            throw DoctoredCoreError(message: "\(name): \(error.localizedDescription)")
        }
    }

    /// The `Presets` folder of the newest installed Photoshop — the
    /// lexicographically greatest `/Applications/Adobe Photoshop */Presets`
    /// that exists — or nil. Only the open panel's starting folder.
    static func installedPresetsDirectory() -> URL? {
        let applications = URL(fileURLWithPath: "/Applications", isDirectory: true)
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: applications.path)
        else { return nil }
        for name in names.filter({ $0.hasPrefix("Adobe Photoshop ") }).sorted(by: >) {
            let presets = applications.appendingPathComponent(name, isDirectory: true)
                .appendingPathComponent("Presets", isDirectory: true)
            var isDirectory: ObjCBool = false
            if FileManager.default.fileExists(atPath: presets.path, isDirectory: &isDirectory),
               isDirectory.boolValue
            {
                return presets
            }
        }
        return nil
    }
}
