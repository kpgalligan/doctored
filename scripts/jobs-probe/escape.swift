import AppKit

/// One modal wait whose worker polls its control until cancelled, with `event`
/// (built once the modal session exists) posted into the queue at 0.2 s.
/// Returns whether the worker saw cancellation and how long `wait` took.
private func escapeCase(
    _ name: String, returnAfter: Double = 10, event makeEvent: @escaping () -> NSEvent?
) throws -> (cancelled: Bool, elapsed: Double) {
    let control = try RasterJobControl()
    let task = try WorkerTask<Bool> {
        let deadline = Date().addingTimeInterval(returnAfter)
        while !control.isCancelled && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.005)
        }
        return control.isCancelled
    }
    let post = Timer(timeInterval: 0.2, repeats: false) { _ in
        if let event = makeEvent() { NSApp.postEvent(event, atStart: false) }
    }
    RunLoop.main.add(post, forMode: .common)
    defer { post.invalidate() }
    let started = Date()
    let cancelled = try BackgroundWork.wait(task, title: "Escape probe \(name)", control: control)
    let elapsed = Date().timeIntervalSince(started)
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] worker slot released after wait")
    return (cancelled, elapsed)
}

private func keyDown(_ characters: String, keyCode: UInt16, windowNumber: Int) -> NSEvent? {
    NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                     timestamp: ProcessInfo.processInfo.systemUptime,
                     windowNumber: windowNumber, context: nil, characters: characters,
                     charactersIgnoringModifiers: characters, isARepeat: false,
                     keyCode: keyCode)
}

func verifyEscapeCancelsModalWait() throws {
    // (a) Esc addressed to no window.
    let a = try escapeCase("a") { keyDown("\u{1b}", keyCode: 53, windowNumber: 0) }
    try require(a.cancelled && a.elapsed < 5,
                "(a) Esc with windowNumber 0 did not cancel within 5 s: \(a)")

    // (b) Esc addressed to the modal progress panel.
    let b = try escapeCase("b") {
        keyDown("\u{1b}", keyCode: 53, windowNumber: NSApp.modalWindow?.windowNumber ?? 0)
    }
    try require(b.cancelled && b.elapsed < 5,
                "(b) Esc to the modal panel did not cancel within 5 s: \(b)")

    // (c) Any other key leaves the job running to its own completion.
    let c = try escapeCase("c", returnAfter: 0.5) { keyDown("a", keyCode: 0, windowNumber: 0) }
    try require(!c.cancelled, "(c) a non-Esc key cancelled the wait: \(c)")

    // (d) The real press: Esc arrives addressed to the key editor window.
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    guard let window = document.windowControllers.first?.window else {
        throw ProbeFailure(description: "(d) fixture editor window")
    }
    window.makeKey()
    let windowNumber = window.windowNumber
    let d = try escapeCase("d") { keyDown("\u{1b}", keyCode: 53, windowNumber: windowNumber) }
    try require(d.cancelled && d.elapsed < 5,
                "(d) Esc to the key editor window did not cancel within 5 s: \(d)")

    print("PASS: Esc cancels a cancellable modal wait (a: no window, b: modal panel, "
        + "d: key editor window) and another key does not (c)")
}
