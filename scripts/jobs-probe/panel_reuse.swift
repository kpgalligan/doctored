import AppKit

// `BackgroundWork.wait` checks one pooled progress panel out and back in, so sequential
// waits share one `NSWindow`, retitled per wait, and Cancel and Esc on the reused panel cancel the
// wait that holds it.

private func reuseKeyDown(windowNumber: Int) -> NSEvent? {
    NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                     timestamp: ProcessInfo.processInfo.systemUptime,
                     windowNumber: windowNumber, context: nil, characters: "\u{1b}",
                     charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)
}

/// One wait of `seconds` (ended early by a cancel when `cancellable`); a
/// `.modalPanel` timer captures the modal window and its title, then runs
/// `act` once on it. Returns the window, its title, whether the worker saw a
/// cancel, how long the wait took and whether the window was still visible.
private func pooledWait(_ title: String, seconds: TimeInterval, cancellable: Bool,
                        revealAfter: TimeInterval = 0, act: ((NSWindow) -> Void)? = nil)
    throws -> (window: NSWindow?, title: String?, cancelled: Bool, elapsed: TimeInterval, visibleAfter: Bool)
{
    let control = cancellable ? try RasterJobControl() : nil
    let task = try WorkerTask<Bool> {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline && control?.isCancelled != true {
            Thread.sleep(forTimeInterval: 0.005)
        }
        return control?.isCancelled == true
    }
    var window: NSWindow?
    var seen: String?
    var acted = false
    let timer = Timer(timeInterval: 0.01, repeats: true) { _ in
        guard let modal = NSApp.modalWindow else { return }
        if window == nil {
            window = modal
            seen = modal.title
        }
        if !acted, let act {
            acted = true
            act(modal)
        }
    }
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    let started = Date()
    let cancelled = try BackgroundWork.wait(task, title: title, control: control, revealAfter: revealAfter)
    return (window, seen, cancelled, Date().timeIntervalSince(started), window?.isVisible ?? true)
}

func verifyProgressPanelReuse() throws {
    let clickCancel: (NSWindow) -> Void = { modal in
        let cancel = modal.contentView.map(buttons)?.first {
            $0.action == NSSelectorFromString("cancelClicked:") && !$0.isHidden
        }
        cancel?.performClick(nil)
    }
    let pressEscape: (NSWindow) -> Void = { modal in
        if let event = reuseKeyDown(windowNumber: modal.windowNumber) { NSApp.postEvent(event, atStart: false) }
    }
    let a = try pooledWait("Panel Reuse A", seconds: 0.15, cancellable: false)
    let b = try pooledWait("Panel Reuse B", seconds: 5, cancellable: true, act: clickCancel)
    let c = try pooledWait("Panel Reuse C", seconds: 0.05, cancellable: false, revealAfter: 0.25)
    let d = try pooledWait("Panel Reuse D", seconds: 5, cancellable: true, act: pressEscape)
    guard let panel = a.window else { throw ProbeFailure(description: "No modal panel captured in wait A") }
    for (name, wait) in [("A", a), ("B", b), ("C", c), ("D", d)] {
        try require(wait.window === panel, "Wait \(name) used the same panel: \(String(describing: wait.window))")
        try require(wait.title == "Panel Reuse \(name)",
                    "Wait \(name)'s panel carried its own title: \(String(describing: wait.title))")
        try require(!wait.visibleAfter, "The panel is ordered out after wait \(name)")
    }
    try require(!a.cancelled && !c.cancelled, "Uncancelled waits ran to completion: \(a) \(c)")
    try require(b.cancelled, "Cancel on the reused panel cancelled wait B: \(b)")
    try require(d.cancelled, "Esc on the reused panel cancelled wait D: \(d)")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0, "Worker slot released")
    print("PASS: progress panel reuse — four sequential waits share one panel, retitled per wait and "
        + "ordered out after each; Cancel and Esc on the reused panel cancel their wait")
}
