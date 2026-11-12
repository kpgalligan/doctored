import AppKit

// Image ▸ Mode ▸ Assign Profile… / Convert to Profile… on a CMYK document and Image ▸ Mode ▸
// 8 Bits/Channel and 16 Bits/Channel (#1 phase 5): the commits (one undo step each, recorded as
// `assign_profile`, `convert_profile` and `convert_precision`) and the validation the precision
// gate asks first. On a 32-bit RGB document 8/16 Bits/Channel open HDR Toning at that depth
// (`EditorViewController+HdrToning.swift`); on an 8- or 16-bit RGB one Convert Precision
// on that mode; on a Lab Color one its own depth edit (#1 Lab phase 5a,
// `EditorViewController+LabMode.swift`'s `commitLabDepth`); the CMYK edit itself is
// `CmykProfileEdit.swift`, the sheet `CmykProfileSheetController`.

extension EditorViewController {
    /// Image ▸ Mode ▸ 8 Bits/Channel.
    @objc func convertToEightBits(_ sender: Any?) {
        convertToBits(8)
    }

    /// Image ▸ Mode ▸ 16 Bits/Channel.
    @objc func convertToSixteenBits(_ sender: Any?) {
        convertToBits(16)
    }

    private func convertToBits(_ bits: Int) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if doc.isFloat, !doc.isCmyk {
            presentHdrToning(bitDepth: bits)
            return
        }
        // #1 phase 5a: an 8- or 16-bit Grayscale document's own depth edit (a 32-bit one opened
        // HDR Toning above).
        if doc.isGray {
            commitGrayProfileEdit(.depth(bits), profile: nil, space: nil, destination: [:],
                                  options: CmykConversionOptions(), document: document)
            return
        }
        // #1 Lab phase 5a: a Lab Color document's own depth edit (`LabDepthEdit`).
        if doc.isLab {
            commitLabDepth(bits, document: document)
            return
        }
        guard doc.isCmyk else {
            presentAsSheet(ConvertPrecisionSheetController(editor: self, document: document,
                                                           target: bits == 8 ? .rgba8 : .rgba16))
            return
        }
        commitCmykProfileEdit(.depth(bits), profile: nil, destination: [:], options: CmykConversionOptions(),
                              document: document)
    }

    /// Assign Profile… / Convert to Profile… on a CMYK document: its own sheet. False for an RGB
    /// document, whose sheets are `EditorViewController+ColorManagement.swift`'s.
    func presentCmykProfileSheet(_ kind: CmykProfileEdit.Kind) -> Bool {
        guard let document, document.doc?.isCmyk == true else { return false }
        presentAsSheet(CmykProfileSheetController(editor: self, document: document, kind: kind))
        return true
    }

    /// The CMYK profile edit as one undo step, recorded as its MCP twin.
    func commitCmykProfileEdit(_ kind: CmykProfileEdit.Kind, profile: Data?, destination: [String: Any],
                               options: CmykConversionOptions, document: ImageDocument) {
        do {
            let edit = try CmykProfileEdit(document, kind: kind, profile: profile, destination: destination,
                                           options: options)
            guard edit.changes else { NSSound.beep(); return }
            let prepared = try edit.compute()
            try edit.publish(prepared, to: document, record: [edit.step()])
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }

    /// Convert to Profile into an RGB profile: Image ▸ Mode ▸ RGB Color's conversion with that
    /// destination, recorded as the `convert_profile` that names it.
    func commitCmykToRGBProfile(_ profile: Data?, destination: [String: Any], options: CmykConversionOptions,
                                document: ImageDocument) {
        do {
            let conversion = try CmykModeConversion(document, target: .rgb, profile: profile, options: options)
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

    /// Image ▸ Mode ▸ 8/16 Bits/Channel at every depth (the current one checked and disabled; a
    /// float document converts through Convert Precision), and Assign / Convert to Profile on a
    /// CMYK document; on a Grayscale document (#1 phase 5a) also 32 Bits/Channel, its declared
    /// depth checked; on a Lab one (Lab phase 5a) its declared depth checked, 32 left to the Lab
    /// gate. nil for every other item.
    func validateBitDepthItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let action = item.action else { return nil }
        let eight = action == #selector(convertToEightBits(_:))
        let sixteen = action == #selector(convertToSixteenBits(_:))
        let float = action == #selector(convertToFloatPrecision(_:))
        guard eight || sixteen || float else { return nil }
        guard let document, let doc = document.doc else { return false }
        if float, !doc.isGray { return nil }
        let isCurrent = doc.bitsPerChannel == (eight ? 8 : sixteen ? 16 : 32)
        (item as? NSMenuItem)?.state = isCurrent ? .on : .off
        return !isCurrent && !document.isLiveEditing && !BackgroundWork.occupied
    }
}
