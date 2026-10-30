import AppKit

// Edit ▸ Convert to Profile… on a Lab Color document (#547 Lab phase 5b): a Lab document has no
// document profile to convert from,
// so it is Image ▸ Mode's conversion into the chosen profile: CMYK Color into a CMYK profile (the
// core's `lab_cmyk`), RGB Color into an RGB one (`convertLabToRGB`) and (phase 6a-ii) Grayscale
// into a Gray one (5a-ii's route, `lab_gray`: each pixel's L* through its tone curve; a Gray
// working space by name, an installed Gray profile by bytes), at the document's declared depth,
// as ONE undo step recorded as `convert_profile` (`commitCmykToRGBProfile`'s shape). The sheet is
// `CmykProfileSheetController`'s Lab arm; the MCP twin is `AgentServer+LabColour.swift`'s.
// Assign Profile… stays disabled (`LabDocument.disabledTools`).

extension EditorViewController {
    /// Convert to Profile… on a Lab document: the CMYK profile sheet's Lab arm. False for every
    /// other document.
    func presentLabProfileSheet() -> Bool {
        guard let document, document.doc?.isLab == true else { return false }
        presentAsSheet(CmykProfileSheetController(editor: self, document: document, kind: .convert))
        return true
    }

    /// The Lab document converted to `target` (CMYK, RGB Color or Grayscale) in `profile` (toward
    /// Grayscale a Gray ICC, else the Gray working space `graySpace`), recorded as the
    /// `convert_profile` that names `destination`.
    func commitLabProfileConversion(_ target: ColorModeTarget, profile: Data?, graySpace: String? = nil,
                                    destination: [String: Any], options: CmykConversionOptions,
                                    document: ImageDocument) {
        do {
            let conversion = try CmykModeConversion(document, target: target, profile: profile,
                                                    options: options, graySpace: graySpace)
            var arguments: [String: Any] = ["intent": options.intent.rawValue,
                                            "black_point_compensation": options.blackPointCompensation,
                                            "flatten": options.flatten]
            arguments.merge(destination) { _, chosen in chosen }
            let prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: [
                ActionStep(tool: "convert_profile", arguments: arguments, note: "Convert to Profile"),
            ])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }
}
