import AppKit

// Clouds have no dialog (swatches, a fresh seed, ⌥ high contrast); ⌃F draws a fresh seed for both
// Clouds and Difference Clouds (`repeatLastFilter`), an Action keeps its recorded one.

/// Filters ▸ Render ▸ Clouds and Difference Clouds. No dialog:
/// the clouds mix the current foreground and background swatches with a fresh
/// seed, and holding Option renders high-contrast clouds. One core kernel at
/// both depths (`clouds` / `difference_clouds`); the recorded step carries
/// the seed and both colours, so an Action replays identical clouds while ⌃F
/// draws a new seed (`repeatLastFilter`).
extension EditorViewController {
    @objc func applyClouds(_ sender: Any?) {
        renderClouds("clouds", actionName: "Clouds")
    }

    @objc func applyDifferenceClouds(_ sender: Any?) {
        renderClouds("difference_clouds", actionName: "Difference Clouds")
    }

    private func renderClouds(_ name: String, actionName: String) {
        // The seed crosses as the Int64 BIT PATTERN, as Add Grain's does.
        let seed = UInt64.random(in: UInt64.min ... UInt64.max)
        let params: [String: Any] = [
            "seed": Int(bitPattern: UInt(seed)),
            "foreground_color": ActionArgs.hex(paintColor),
            "background_color": ActionArgs.hex(backgroundColor),
            "high_contrast": NSEvent.modifierFlags.contains(.option),
        ]
        if applyPrecisionFilter(name, params: params) { return }
        guard !refuseOperationScope(.filter) else { return }
        guard let document, let capture = document.captureOperation() else { NSSound.beep(); return }
        let request = ByteFilterRequest(op: name, params: params)
        document.applyCapturedByteFilter(capture, actionName, record: filterRecord(name, params),
                                         request: request)
    }
}
