import AppKit

// Shared by the v1 UI coverage cases (`ui_float_view.swift`, `ui_presence.swift`,
// `ui_stacks.swift`, `ui_merge_hdr.swift`, `ui_develop.swift`,
// `ui_float_sheets.swift`, and round 2's `ui_new_document_float.swift`,
// `ui_convert_precision.swift`, `ui_people.swift`, `ui_lens_correction.swift`,
// `ui_develop_lens.swift`, `ui_float_tools.swift`, `ui_select_mask_float.swift`,
// `ui_presence_float.swift`): a driver for the app-modal `ModalCardWindowController`
// dialogs, a control lookup by identifier, menu validation by selector name, a PNG
// writer for the file fixtures, a recorder bracket, the shared photo fixtures and a
// document-free MCP call.

/// Drives one modal window (`NSApp.modalWindow` whose title is `title`) from a
/// timer in `.common` and `.modalPanel`. Each tick calls the current step; true
/// advances, false waits. A throw, or `timeout` seconds since the last advance,
/// records the error and calls `NSApp.abortModal()`.
final class ModalSteps {
    typealias Step = (name: String, run: (NSWindow) throws -> Bool)

    private let title: String
    private let timeout: TimeInterval
    private let steps: [Step]
    private var index = 0
    private var error: Error?
    private var deadline = Date()

    init(title: String, timeout: TimeInterval = 90, steps: [Step]) {
        self.title = title
        self.timeout = timeout
        self.steps = steps
    }

    /// Runs `body`, which presents the modal and returns once it has ended.
    /// Throws the recorded error, or when the modal ended before every step ran.
    func run(_ body: () throws -> Void) throws {
        index = 0
        error = nil
        deadline = Date().addingTimeInterval(timeout)
        let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        defer { timer.invalidate() }
        try autoreleasepool { try body() }
        if let error { throw error }
        try require(index == steps.count,
                    "[\(title)] the modal ended before step \(index + 1) of \(steps.count)"
                    + (index < steps.count ? " (\(steps[index].name))" : ""))
    }

    private func fail(_ message: String) {
        error = ProbeFailure(description: "[\(title)] \(message)")
        if NSApp.modalWindow != nil { NSApp.abortModal() }
    }

    private func tick() {
        guard index < steps.count, error == nil else { return }
        let step = steps[index]
        guard Date() <= deadline else {
            fail("timed out at step \(index + 1) of \(steps.count) (\(step.name)); the modal "
                 + "window is \"\(NSApp.modalWindow?.title ?? "none")\"")
            return
        }
        guard let window = NSApp.modalWindow, window.title == title else { return }
        do {
            if try autoreleasepool(invoking: { try step.run(window) }) {
                index += 1
                deadline = Date().addingTimeInterval(timeout)
            }
        } catch {
            fail("step \(index + 1) (\(step.name)): \(error)")
        }
    }
}

private func uiDescendants(_ view: NSView) -> [NSView] {
    [view] + view.subviews.flatMap(uiDescendants)
}

/// Every `T` under `root`, depth-first.
func uiViews<T: NSView>(_ root: NSView, as type: T.Type) -> [T] {
    uiDescendants(root).compactMap { $0 as? T }
}

/// The one control under `root` whose `accessibilityIdentifier()` or
/// `identifier?.rawValue` is `id`.
func uiControl<T: NSView>(_ root: NSView, id: String, as type: T.Type) throws -> T {
    let named = uiDescendants(root).filter {
        $0.accessibilityIdentifier() == id || $0.identifier?.rawValue == id
    }
    guard let found = named.compactMap({ $0 as? T }).first else {
        throw ProbeFailure(description: "no \(T.self) with the identifier \"\(id)\" "
            + "(\(named.count) other views carry it)")
    }
    return found
}

/// Sends `control`'s own action to its own target, as a click or an edit does.
func uiSend(_ control: NSControl, _ what: String) throws {
    guard let action = control.action else {
        throw ProbeFailure(description: "\(what): the control has no action")
    }
    try require(control.isEnabled, "\(what): the control is enabled")
    try require(NSApp.sendAction(action, to: control.target, from: control),
                "\(what): the action was delivered")
}

/// Whether the editor validates the menu item whose action is `selector`.
func uiValidates(_ controller: EditorViewController, _ selector: String, tag: Int = 0) -> Bool {
    let item = NSMenuItem(title: "", action: NSSelectorFromString(selector), keyEquivalent: "")
    item.tag = tag
    return controller.validateUserInterfaceItem(item)
}

/// Writes straight RGBA8 (row 0 = top) as an sRGB PNG.
func uiWritePNG(_ rgba: [UInt8], width: Int, height: Int, to url: URL) throws {
    guard width > 0, height > 0, rgba.count == width * height * 4,
          let space = CGColorSpace(name: CGColorSpace.sRGB),
          let provider = CGDataProvider(data: Data(rgba) as CFData),
          let image = CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    else {
        throw ProbeFailure(description: "PNG fixture \(url.lastPathComponent): no bitmap")
    }
    guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    else {
        throw ProbeFailure(description: "PNG fixture \(url.lastPathComponent): no encoding")
    }
    try png.write(to: url)
}

/// The steps `body` records: `ActionRecorder.shared.start()` … `stop()`.
func uiRecorded(_ body: () throws -> Void) rethrows -> [ActionStep] {
    ActionRecorder.shared.start()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() } }
    try body()
    return ActionRecorder.shared.stop()
}

/// The last `count` recorded alerts, for a failure message.
func uiAlerts(_ count: Int = 2) -> String {
    AutomationAlerts.recorded.suffix(count)
        .map { "\($0.messageText) | \($0.informativeText)" }.joined(separator: " ;; ")
}

/// One photo of the shared fixture folder: `$DR_JOBS_PROBE_FIXTURES/<relative>`
/// (`verify.py` defaults it to ~/devel/doctored-fixtures). An unset variable or
/// a missing file fails only the case that asked, naming the full path.
func uiFixture(_ relative: String) throws -> URL {
    guard let folder = ProcessInfo.processInfo.environment["DR_JOBS_PROBE_FIXTURES"],
          !folder.isEmpty else {
        throw ProbeFailure(description: "DR_JOBS_PROBE_FIXTURES is not set, so \(relative) cannot "
            + "be found (scripts/jobs-probe/verify.py sets it)")
    }
    let url = URL(fileURLWithPath: folder).appendingPathComponent(relative)
    guard FileManager.default.fileExists(atPath: url.path) else {
        throw ProbeFailure(description: "the fixture \(url.path) is missing")
    }
    return url
}

/// `AgentServer.shared.execute` with no `document_id` added (for
/// `new_document`), decoded as `agentCall` in `heal_jobs.swift` decodes it.
func uiExecute(_ tool: String, _ arguments: [String: Any])
    throws -> (isError: Bool, result: [String: Any], text: String)
{
    let encoded = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: tool,
        argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool): \(raw)")
    }
    if reply["isError"] as? Bool == true { return (true, [:], text) }
    guard let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Missing MCP JSON text from \(tool): \(raw)")
    }
    return (false, result, text)
}
