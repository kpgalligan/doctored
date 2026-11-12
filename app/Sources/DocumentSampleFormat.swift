import AppKit

/// The document's colour mode and declared sample format, as the status bar's mode segment
/// names them ("Grayscale · 16-bit", "RGB · 32-bit float", "Bitmap · 1-bit") and MCP's precision
/// name does ("gray16", "rgba32_float", "bitmap1"). Each fact is read
/// through the property its mode already has (`colorMode`, `grayDepth`, `labDepth`,
/// `CmykAdjustFilter.depth(of:)`, `isFloat`): from the captured descriptor, except the RGB arm's
/// float test and the CMYK fallback when the descriptor names no depth, which ask the core.
extension RasterDocument {
    /// The bits per channel the document declares: 1 for Bitmap, 8 for Indexed, the declared
    /// depth of a Grayscale, Duotone, Lab or CMYK document (16 for a CMYK one whose depth cannot
    /// be read, `CmykAdjustFilter.depth(of:)`'s rule), and 32 (float), 16 or 8 for an RGB one.
    /// This is the mode's depth, not the storage's: an 8-bit Grayscale document is held in
    /// 16-bit planes. nil only when a Grayscale or Lab descriptor names no depth.
    var bitsPerChannel: Int? {
        switch colorMode {
        case .bitmap: return 1
        case .indexed: return 8
        case .gray, .duotone: return grayDepth
        case .lab: return labDepth
        case .cmyk: return CmykAdjustFilter.depth(of: self)
        case .rgb: return isFloat ? 32 : isHighDepth ? 16 : 8
        }
    }

    /// The document's mode and depth as MCP names them: the summary's `precision`
    /// (`AgentServer.summary`) and `get_precision`'s `mode` on a colour-mode document. An RGB
    /// document answers its storage (`rgba8`, `rgba16`, `rgba32_float`); every other mode
    /// `<mode><bitsPerChannel>`, which is that mode's `convert_precision` name where it has one
    /// (`gray8`…`gray32`, `lab8`, `lab16`, `cmyk8`, `cmyk16`), and `indexed8` and `bitmap1`. A
    /// Duotone document answers its Gray storage's name, the one `convert_precision` takes there
    /// (`color_mode` says duotone). A Grayscale or Lab descriptor that names no depth answers
    /// `gray0` or `lab0`.
    var precisionName: String {
        let bits = bitsPerChannel ?? 0
        switch colorMode {
        case .rgb: return isFloat ? "rgba32_float" : isHighDepth ? "rgba16" : "rgba8"
        case .gray, .duotone: return "gray\(bits)"
        case .lab: return "lab\(bits)"
        case .cmyk: return "cmyk\(bits)"
        case .indexed: return "indexed\(bits)"
        case .bitmap: return "bitmap\(bits)"
        }
    }

    /// The mode segment before the profile: the mode's status name and the sample format,
    /// "32-bit float" at 32 bits and "<n>-bit" below (`LayerPastePlan.depthPhrase`).
    var modeStatusText: String {
        guard let bits = bitsPerChannel else { return colorMode.statusName }
        return "\(colorMode.statusName) · \(LayerPastePlan.depthPhrase(bits))"
    }
}
