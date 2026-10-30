import AppKit

// On an existing layer the core's `calibratedRefusal` refuses, Adjustment Options… offers Switch to
// Legacy sRGB Evaluation…, which reopens the sheet in `legacy_srgb` (previewed; Apply = one undo
// step).

/// The persisted evaluation choice for Exposure and White Balance. Absent
/// means the shipped sRGB numeric operation; every new RGB edit writes v2.
/// Coverage/individual planes remain profile-independent grayscale operations.
enum CalibratedAdjustment {
    static let legacy = "legacy_srgb"
    static let document = "document_v2"

    static func supports(_ op: String) -> Bool { op == "exposure" || op == "white_balance" }

    static func initialParams(
        op: AdjustmentLayerOp, mode: AdjustmentSheetMode, target: PaintTarget
    ) -> [String: Any] {
        var params = AdjustmentSchema.defaults(for: op)
        guard supports(op.rawValue) else { return params }
        if case .destructive = mode, target != .layer {
            params["evaluation"] = legacy
        } else {
            params["evaluation"] = document
        }
        return params
    }

    static func sheetRefusal(
        op: AdjustmentLayerOp, mode: AdjustmentSheetMode, imageDocument: ImageDocument
    ) -> String? {
        guard supports(op.rawValue), let doc = imageDocument.doc else { return nil }
        if case .edit(_, let original) = mode {
            guard original.string("evaluation", default: legacy) == document else { return nil }
        } else if case .destructive = mode, imageDocument.planeEditTarget != .layer {
            return nil
        }
        return doc.calibratedRefusal(op: op.rawValue)
    }

    /// The re-edit refusal's way forward. The ellipsis says it opens
    /// the sheet rather than committing anything.
    static let legacySwitchTitle = "Switch to Legacy sRGB Evaluation\u{2026}"
    static let legacySwitchDisclosure = "Switch to Legacy sRGB Evaluation\u{2026} reopens this "
        + "layer's settings with the original sRGB processing, which does not use the document "
        + "profile. The layer's appearance changes; the canvas previews it, and nothing changes "
        + "until you click Apply, which records the switch as one undo step."
    static let legacySwitchCaption = "Legacy sRGB processing replaces this layer's "
        + "document-profile processing, so its appearance changes \u{2014} the canvas shows the "
        + "result. Apply records the switch as one undo step; Cancel leaves the layer as it was."

    /// Runs the refusal alert. `reason` is the core's sentence, verbatim. In
    /// `.edit` the alert also discloses the legacy switch and offers it as
    /// its first button beside Cancel; true only when that button was
    /// chosen. Every other mode keeps the one-button alert and answers false.
    static func presentRefusal(_ reason: String, mode: AdjustmentSheetMode) -> Bool {
        let alert = NSAlert()
        alert.messageText = "This adjustment cannot use the document profile."
        guard case .edit = mode else {
            alert.informativeText = reason
            alert.runModal()
            return false
        }
        alert.informativeText = reason + "\n\n" + legacySwitchDisclosure
        alert.addButton(withTitle: legacySwitchTitle)
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
}

extension OperationCapture {
    /// The target has already decided whether evaluation is calibrated or
    /// scalar legacy; only the RGB path needs document colour context.
    func adjustmentImage(_ image: RasterImage, op: String, params: [String: Any]) -> RasterImage? {
        doc.applyingAdjustment(to: image, op: op, params: params)
    }
}
