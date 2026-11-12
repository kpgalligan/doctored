import AppKit

// The screens probe's two capture mechanisms, the cuts, the leak guard and labels, the manifest
// records, and the modal step driver. Only this process's own windows and views are captured:
// `screencapture -l<windowNumber>` of a window in `NSApp.windows` (server, the default), or
// `cacheDisplay` of a view it built (cache, run.py's `--fallback-cache`); never a region, the
// screen or another process's window.

struct ProbeFailure: Error, CustomStringConvertible { let description: String }

func require(_ value: @autoclosure () -> Bool, _ message: String) throws {
    if !value() { throw ProbeFailure(description: message) }
}

/// Runs the main run loop until `predicate` holds or `seconds` pass.
func pumpUntil(_ predicate: () -> Bool, seconds: Double = 30) throws {
    let deadline = Date().addingTimeInterval(seconds)
    while !predicate() {
        if Date() > deadline { throw ProbeFailure(description: "timed out after \(seconds) s") }
        RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
}

/// Lets layout, drawing and Core Animation settle before a capture.
func settle(_ window: NSWindow?, seconds: TimeInterval = 0.3) {
    NSApp.updateWindows()
    RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    window?.layoutIfNeeded()
    window?.displayIfNeeded()
    CATransaction.flush()
}

private func writePNG(_ rep: NSBitmapImageRep, to url: URL) throws {
    guard let png = rep.representation(using: .png, properties: [:]) else {
        throw ProbeFailure(description: "\(url.lastPathComponent): no PNG encoding")
    }
    try png.write(to: url)
}

func descendants(_ view: NSView) -> [NSView] {
    [view] + view.subviews.flatMap(descendants)
}

/// The view's own `cacheDisplay` at scale 2 into an sRGB bitmap of `bounds × 2` pixels whose
/// point size is `bounds.size`, written as PNG.
func cacheCapture(_ view: NSView, to url: URL) throws -> (width: Int, height: Int) {
    let scale: CGFloat = 2
    let bounds = view.bounds
    let width = Int((bounds.width * scale).rounded()), height = Int((bounds.height * scale).rounded())
    guard width > 0, height > 0,
          let blank = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
            samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bytesPerRow: 0, bitsPerPixel: 0),
          let rep = blank.retagging(with: .sRGB) else {
        throw ProbeFailure(description: "\(url.lastPathComponent): no \(width)×\(height) bitmap")
    }
    rep.size = bounds.size
    view.cacheDisplay(in: bounds, to: rep)
    try writePNG(rep, to: url)
    return (width, height)
}

/// `screencapture -l<windowNumber> -o -x` of one of this process's own windows: the whole window
/// frame, title bar included, top-left origin. With `cut` (window base coordinates: bottom-left
/// origin, points), the image is cut to that rect, flipped against the frame height and scaled by
/// the image's own ratio. An image whose height is not the frame height × 2 is an error.
func serverCapture(_ window: NSWindow, cut: CGRect?, to url: URL)
    throws -> (width: Int, height: Int, notes: [String])
{
    try require(NSApp.windows.contains { $0 === window } && window.windowNumber > 0,
                "\(url.lastPathComponent): the window is not one of this process's own")
    // A window past its screen's edge, or a cut past the window image (a sheet hanging below its
    // window), would come out clipped and still look valid.
    if let screen = window.screen {
        try require(screen.frame.contains(window.frame),
                    "the window (\(window.frame)) reaches past its screen (\(screen.frame))")
    }
    var notes: [String] = []
    let raw = cut == nil ? url : url.deletingPathExtension().appendingPathExtension("whole.png")
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    process.arguments = ["-l\(window.windowNumber)", "-o", "-x", raw.path]
    let errors = Pipe()
    process.standardError = errors
    try process.run()
    defer { if cut != nil { try? FileManager.default.removeItem(at: raw) } }
    // Read to EOF first: a full pipe would block screencapture before it exits.
    let said = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        .trimmingCharacters(in: .whitespacesAndNewlines)
    process.waitUntilExit()
    try require(process.terminationStatus == 0,
                "screencapture exited \(process.terminationStatus): \(said)")
    if !said.isEmpty { notes.append("screencapture said: \(said)") }
    guard let data = try? Data(contentsOf: raw), let whole = NSBitmapImageRep(data: data),
          let image = whole.cgImage else {
        throw ProbeFailure(description: "screencapture wrote no readable PNG")
    }
    let frame = window.frame.size
    let expected = Int((frame.height * 2).rounded())
    guard image.height == expected else {
        throw ProbeFailure(description: "the window image is \(image.width)×\(image.height) px; "
            + "the frame × 2 is \(Int((frame.width * 2).rounded()))×\(expected)")
    }
    guard let cut else { return (image.width, image.height, notes) }
    let sx = CGFloat(image.width) / frame.width, sy = CGFloat(image.height) / frame.height
    let pixels = CGRect(x: cut.minX * sx, y: (frame.height - cut.maxY) * sy,
                        width: cut.width * sx, height: cut.height * sy).integral
    let bounds = CGRect(x: 0, y: 0, width: image.width, height: image.height)
    guard bounds.insetBy(dx: -1, dy: -1).contains(pixels),
          let part = image.cropping(to: pixels.intersection(bounds)) else {
        throw ProbeFailure(description:
            "the cut \(pixels) reaches past the \(image.width)×\(image.height) window image")
    }
    notes.append("cut \(Int(pixels.minX)),\(Int(pixels.minY)) \(part.width)×\(part.height) px "
                 + "from the \(image.width)×\(image.height) window image")
    try writePNG(NSBitmapImageRep(cgImage: part), to: url)
    return (part.width, part.height, notes)
}

/// What one capture takes: the window `screencapture` names (a sheet's parent for a sheet), the
/// cut in that window's base coordinates, the view the leak walk and `cacheDisplay` start from,
/// the window whose title the walk reads, and the capture's size in points.
struct CaptureSubject {
    let window: NSWindow
    let cut: CGRect?
    let root: NSView
    let titled: NSWindow?
    let points: CGSize
}

/// The subject of `target` in the running screen. A window, panel, card or alert is its own
/// window, whose frame view the leak walk starts from; a view or sheet is cut from the editor
/// window's image.
func captureSubject(_ target: CaptureTarget, _ context: Context) throws -> CaptureSubject {
    let mechanism = context.recorder.mechanism
    func whole(_ window: NSWindow?, _ what: String) throws -> CaptureSubject {
        guard let window, window.isVisible, let content = window.contentView else {
            throw ProbeFailure(description: "no visible \(what) window")
        }
        let frameView = content.superview ?? content
        return CaptureSubject(window: window, cut: nil, root: frameView, titled: window,
                              points: mechanism == .server ? window.frame.size : frameView.bounds.size)
    }
    switch target {
    case .window:
        return try whole(context.window, "screen")
    case .panel(let name):
        let panel = panels[name]?.window(context)
        // A child window (the options bar's More panel) is cut from its parent's image, which
        // composites it: its own window number captures the parent too, as a sheet's does.
        if let panel, let parent = panel.parent, panel.isVisible, let content = panel.contentView {
            let frame = panel.frame.offsetBy(dx: -parent.frame.minX, dy: -parent.frame.minY)
            return CaptureSubject(window: parent, cut: frame, root: content, titled: nil,
                                  points: mechanism == .server ? frame.size : content.bounds.size)
        }
        return try whole(panel, name)
    case .card(let tab):
        if let tab { try selectTab(tab, in: context.card) }
        return try whole(context.card, "card")
    case .alert:
        return try whole(context.alert, "alert")
    case .view(let name):
        let editor = try context.requireEditor()
        guard let window = context.window, let target = editorViews[name]?(editor), target.window === window,
              !target.isHiddenOrHasHiddenAncestor else {
            throw ProbeFailure(description: "the \(name) view is not shown in the editor window")
        }
        let cut = target.convert(target.bounds, to: nil)
        return CaptureSubject(window: window, cut: cut, root: target, titled: nil, points: target.bounds.size)
    case .sheet:
        guard let window = context.window, let sheet = window.attachedSheet, let content = sheet.contentView else {
            throw ProbeFailure(description: "the window has no sheet attached")
        }
        let frame = sheet.frame.offsetBy(dx: -window.frame.minX, dy: -window.frame.minY)
        return CaptureSubject(window: window, cut: frame, root: content, titled: sheet,
                              points: mechanism == .server ? frame.size : content.bounds.size)
    }
}

/// Selects the tab labelled (or identified) `tab` in the card's one tab view.
private func selectTab(_ tab: String, in card: NSWindow?) throws {
    guard let card, let content = card.contentView else { throw ProbeFailure(description: "no card is running") }
    let tabViews = descendants(content).compactMap { $0 as? NSTabView }
    guard tabViews.count == 1 else { throw ProbeFailure(description: "\(tabViews.count) tab views in the card") }
    let items = tabViews[0].tabViewItems
    guard let item = items.first(where: { $0.label == tab || ($0.identifier as? String) == tab }) else {
        throw ProbeFailure(description: "the card has no tab \(tab); it has \(items.map(\.label))")
    }
    tabViews[0].selectTabViewItem(item)
    settle(card, seconds: 0.5)
}

/// The leak guard and the labels. A leak is any string a capture would show that holds a home
/// path or the user's names or email; `label` rewrites every string the manifest holds.
struct Labeller {
    private let folders: [(path: String, label: String)]
    private let email: String?
    private let fullName = NSFullUserName()
    private let shortName = NSUserName()

    /// `folders` are matched as given and with symlinks resolved, longest first.
    init(folders: [(path: String, label: String)], email: String?) {
        self.folders = folders.flatMap { folder in
            [folder, (URL(fileURLWithPath: folder.path).resolvingSymlinksInPath().path, folder.label)]
        }.map { (path: $0.path.count > 1 && $0.path.hasSuffix("/") ? String($0.path.dropLast()) : $0.path,
                 label: $0.label) }
        .filter { $0.path.count > 1 }
        .sorted { $0.path.count > $1.path.count }
        self.email = email.flatMap { $0.isEmpty ? nil : $0 }
    }

    private var needles: [String] {
        ["/Users/", "-Users-", "/home/", shortName, fullName, email ?? ""].filter { !$0.isEmpty }
    }

    /// `text` with each labelled folder replaced at a path-component boundary (the repository is
    /// a prefix of `doctored-fixtures`), any other `/Users/<name>` or `/home/<name>` prefix as `~`,
    /// the email as `<email>`, the full name as `<user>`, and the short name as `<user>` when it is
    /// at least 4 characters (a shorter one would rewrite ordinary words; `leaks` replaces it
    /// in its own texts at any length).
    func label(_ text: String) -> String {
        label(text, shortNameAnyLength: false)
    }

    private func label(_ text: String, shortNameAnyLength: Bool) -> String {
        var result = text
        for (path, name) in folders {
            let pattern = NSRegularExpression.escapedPattern(for: path) + "(?![A-Za-z0-9_-])"
            result = result.replacingOccurrences(
                of: pattern, with: NSRegularExpression.escapedTemplate(for: name), options: .regularExpression)
        }
        result = result.replacingOccurrences(of: #"/(Users|home)/[^/\s"']+"#, with: "~",
                                             options: .regularExpression)
        if let email { result = result.replacingOccurrences(of: email, with: "<email>") }
        if !fullName.isEmpty { result = result.replacingOccurrences(of: fullName, with: "<user>") }
        if !shortName.isEmpty, shortNameAnyLength || shortName.count >= 4 {
            result = result.replacingOccurrences(of: shortName, with: "<user>")
        }
        return result
    }

    /// Every string under `root` (and `window`'s title) holding a needle: text field values,
    /// button titles, text view strings, popup item titles and tab labels. Each text is labelled.
    func leaks(in root: NSView, window: NSWindow?) -> [[String: String]] {
        var strings: [(kind: String, text: String)] = []
        if let window { strings.append(("window_title", window.title)) }
        for view in descendants(root) {
            if let field = view as? NSTextField { strings.append(("text_field", field.stringValue)) }
            if let popup = view as? NSPopUpButton { strings += popup.itemTitles.map { ("popup_item", $0) } }
            if let button = view as? NSButton { strings.append(("button", button.title)) }
            if let text = view as? NSTextView { strings.append(("text_view", text.string)) }
            if let tabs = view as? NSTabView { strings += tabs.tabViewItems.map { ("tab", $0.label) } }
        }
        let needles = self.needles
        return strings.filter { string in needles.contains { string.text.contains($0) } }
            .map { ["kind": $0.kind, "text": label($0.text, shortNameAnyLength: true)] }
    }
}

/// The captures of one look, written under `out` as `<screen>.<capture>.<look>.png`, and their
/// manifest records. Every string a record holds goes through `labeller`.
final class Recorder {
    let out: URL
    let look: Look
    let mechanism: Mechanism
    let labeller: Labeller
    private(set) var captures: [[String: Any]] = []

    init(out: URL, look: Look, mechanism: Mechanism, labeller: Labeller) {
        self.out = out
        self.look = look
        self.mechanism = mechanism
        self.labeller = labeller
    }

    /// Records `spec` of `screen`. A leak writes no file; a failed capture is an `error` record.
    func capture(_ screen: String, _ spec: CaptureSpec, subject: () throws -> CaptureSubject,
                 notes extra: [String] = []) {
        let file = "\(screen).\(spec.name).\(look.rawValue).png"
        var record: [String: Any] = [
            "screen": screen, "capture": spec.name, "of": spec.of,
            "pixel_width": 0, "pixel_height": 0, "point_width": 0.0, "point_height": 0.0,
            "window_number": 0, "leak": [[String: String]](),
        ]
        var notes = extra
        do {
            let subject = try subject()
            settle(subject.window)
            record["window_number"] = subject.window.windowNumber
            record["point_width"] = Double(subject.points.width)
            record["point_height"] = Double(subject.points.height)
            let hits = labeller.leaks(in: subject.root, window: subject.titled)
            if !hits.isEmpty {
                record["leak"] = hits
            } else {
                let url = out.appendingPathComponent(file)
                switch mechanism {
                case .server:
                    try require(NSScreen.screens.contains { $0.backingScaleFactor == 2 }, "no 2x screen")
                    try require(subject.window.backingScaleFactor == 2,
                                "the window is on a \(subject.window.backingScaleFactor)x screen")
                    let shot = try serverCapture(subject.window, cut: subject.cut, to: url)
                    record["pixel_width"] = shot.width
                    record["pixel_height"] = shot.height
                    notes += shot.notes
                case .cache:
                    // The frame view (window), the view, or the sheet's content view.
                    let root = subject.root
                    if look == .dark, descendants(root).contains(where: { $0 is NSTabView }) {
                        throw ProbeFailure(description: "cacheDisplay draws a dark NSTabView blank")
                    }
                    let pixels = try cacheCapture(root, to: url)
                    record["pixel_width"] = pixels.width
                    record["pixel_height"] = pixels.height
                    notes.append("cacheDisplay at scale 2")
                }
                record["file"] = file
            }
        } catch {
            record["error"] = labeller.label(String(describing: error))
        }
        record["notes"] = notes.map { labeller.label($0) }
        captures.append(record)
        let outcome = record["file"] as? String ?? record["error"] as? String ?? "leak"
        print("capture \(screen).\(spec.name): \(outcome)")
        fflush(stdout)
    }

    /// An `error` record for each of `specs` not yet recorded for `screen`.
    func failed(_ screen: String, _ specs: [CaptureSpec], error unlabelled: String) {
        let reached = Set(captures.filter { $0["screen"] as? String == screen }.compactMap { $0["capture"] as? String })
        let error = labeller.label(unlabelled)
        for spec in specs where !reached.contains(spec.name) {
            captures.append(["screen": screen, "capture": spec.name, "of": spec.of, "error": error,
                             "pixel_width": 0, "pixel_height": 0, "point_width": 0.0,
                             "point_height": 0.0, "window_number": 0, "leak": [[String: String]](),
                             "notes": [String]()])
            print("capture \(screen).\(spec.name): \(error)")
        }
        fflush(stdout)
    }
}

/// Drives one modal window (`NSApp.modalWindow` whose title is `title`) from a timer in `.common`
/// and `.modalPanel`. Each tick calls the current step; true advances, false waits. A throw, or
/// `timeout` seconds since the last advance, records the error and calls `NSApp.abortModal()`.
/// Master copy: `ModalSteps` in `scripts/jobs-probe/ui_support.swift`; this copy adds `busy`, so a
/// tick that fires while a step runs (a capture spins the run loop) returns at once.
final class ModalSteps {
    typealias Step = (name: String, run: (NSWindow) throws -> Bool)

    private let title: String
    private let timeout: TimeInterval
    private let steps: [Step]
    private var index = 0
    private var error: Error?
    private var deadline = Date()
    private var busy = false

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
        guard !busy, index < steps.count, error == nil else { return }
        busy = true
        defer { busy = false }
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
