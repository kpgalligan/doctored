import AppKit

// Bare `{`/`}` step the paint tool's hardness along `BrushPreset.steppedHardness`'s 0/25/50/75/100
// ladder, `,`/`.` and Shift+`,`/Shift+`.` step the built-in presets
// (`BrushPreset.stepped(from:_:)`, the popup's store write), and `/` toggles Lock Transparency
// through that menu item's own validation and `lockTransparency(_:)`.
//
// The rule: Edit ▸ Clear's bare ⌫ stands down on
// `ImageCanvasView.hasPendingSelectionGesture` — an open Polygonal Lasso polygon (⌫ pops its last
// vertex in `keyDown`), a Freehand Lasso / Quick Selection drag in flight (the stroke commits
// against its mouse-down capture) or a Rectangular / Elliptical Marquee drag in flight
// (`dragAnchor`; it commits against its mouse-down selection) or an open Magnetic Lasso outline
// (⌫ removes its last anchor in `handleMagneticLassoKey`) — one predicate, never a second guard.

/// The canvas's brush and lock keys: bare { } step
/// the tip's hardness, , . < > step the built-in presets, and / toggles
/// Lock Transparency. The canvas only reports the key; these do the work.
extension EditorViewController {
    /// Bare { and }: steps the active paint tool's hardness along the
    /// 0/25/50/75/100 ladder, mirrored like `brushSizeKeyChanged`.
    func brushHardnessKeyStepped(up: Bool) {
        guard var paint = ToolOptionsStore.shared.paintOptions(for: currentTool) else { return }
        paint.hardness = BrushPreset.steppedHardness(paint.hardness, up: up)
        ToolOptionsStore.shared.setPaintOptions(paint, for: currentTool)
        syncCanvasPaintState()
        optionsBar.refreshValues()
    }

    /// Bare , . and Shift+, Shift+.: applies the stepped built-in preset
    /// the way the options bar's preset popup does (size and opacity kept).
    func brushPresetKeyStepped(_ step: BrushPreset.Step) {
        guard var paint = ToolOptionsStore.shared.paintOptions(for: currentTool) else { return }
        paint = BrushPreset.stepped(from: paint, step).applied(to: paint)
        ToolOptionsStore.shared.setPaintOptions(paint, for: currentTool)
        syncCanvasPaintState()
        optionsBar.refreshValues()
    }

    /// Bare /: Layer ▸ Lock ▸ Transparency, gated by that menu item's own
    /// validation; a refusal beeps.
    func lockTransparencyKeyPressed() {
        let item = NSMenuItem(
            title: "", action: #selector(lockTransparency(_:)), keyEquivalent: "")
        guard validateUserInterfaceItem(item) else {
            NSSound.beep()
            return
        }
        lockTransparency(nil)
    }
}
