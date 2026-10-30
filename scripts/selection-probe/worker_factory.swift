import AppKit

func verifyCutoutWorkerFactory() throws {
    let baseline = Set(NSApp.windows.filter(\.isVisible).map(ObjectIdentifier.init))
    var refused = false
    do {
        let _: Int = try BackgroundWork.wait(title: "Throwing Cutout Factory") { () throws -> WorkerTask<Int> in
            try require(BackgroundWork.isWaiting && !BackgroundWork.occupied
                && NSApp.windows.contains(where: { $0.isVisible && $0.title == "Throwing Cutout Factory" }),
                "The progress panel and modal session exist before the factory starts")
            throw DoctoredCoreError(message: "Deliberate factory refusal")
        }
    } catch let failure as DoctoredCoreError { refused = failure.message == "Deliberate factory refusal" }
    try require(refused && !BackgroundWork.isWaiting && !BackgroundWork.occupied
        && BackgroundWork.scratchEstimate == 0, "Factory refusal leaves no worker/modal/scratch state")
    try require(Set(NSApp.windows.filter(\.isVisible).map(ObjectIdentifier.init)) == baseline,
                "Factory refusal closes only its progress panel")
    print("PASS: factory failure occurs after real panel creation and cleans modal/waiting/worker/scratch state")

    // Hold a non-pixel worker until the nested busy-refusal check finishes.
    // This is an ownership-state test, not a delayed image-cancellation witness.
    let gate = DispatchSemaphore(value: 0)
    let outer = try WorkerTask(scratchEstimate: 12345) {
        guard gate.wait(timeout: .now() + 5) == .success else {
            throw DoctoredCoreError(message: "Nested factory test did not release its outer worker")
        }
        return 23
    }
    var nestedFailure: Error?, checked = false
    let timer = Timer(timeInterval: 0.001, repeats: true) { timer in
        guard !checked, BackgroundWork.isWaiting,
              NSApp.windows.contains(where: { $0.isVisible && $0.title == "Outer Existing Worker" }) else { return }
        checked = true; timer.invalidate()
        defer { gate.signal() }
        do {
            var busy = false
            do {
                let _: Int = try BackgroundWork.wait(title: "Busy Cutout Factory") {
                    try WorkerTask(scratchEstimate: 99999) { 99 }
                }
            } catch let failure as DoctoredCoreError { busy = failure.message.contains("Another background operation") }
            try require(busy && BackgroundWork.isWaiting && BackgroundWork.occupied
                && BackgroundWork.scratchEstimate == 12345,
                "Nested busy refusal preserves the outer waiting/occupied/scratch ownership")
            try require(!NSApp.windows.contains(where: { $0.isVisible && $0.title == "Busy Cutout Factory" })
                && NSApp.windows.contains(where: { $0.isVisible && $0.title == "Outer Existing Worker" }),
                "Nested refusal closes its own modal and retains the outer one")
        } catch { nestedFailure = error }
    }
    RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate(); gate.signal() }
    let value = try BackgroundWork.wait(outer, title: "Outer Existing Worker")
    if let nestedFailure { throw nestedFailure }
    try require(checked && value == 23 && !BackgroundWork.isWaiting && !BackgroundWork.occupied
        && BackgroundWork.scratchEstimate == 0, "Existing-task wait still drains the original mailbox after nested refusal")
    print("PASS: nested busy factory restores outer waiting, occupied, scratch and modal state; existing worker drains normally")

    let result: Int = try BackgroundWork.wait(title: "Successful Cutout Factory") {
        try require(NSApp.windows.contains(where: { $0.isVisible && $0.title == "Successful Cutout Factory" }),
                    "Successful factory also sees its progress UI before dispatch")
        return try WorkerTask(scratchEstimate: 4567) { 37 }
    }
    try require(result == 37 && !BackgroundWork.occupied && !BackgroundWork.isWaiting
        && BackgroundWork.scratchEstimate == 0
        && Set(NSApp.windows.filter(\.isVisible).map(ObjectIdentifier.init)) == baseline,
        "Successful factory drains the worker and closes its panel")
    print("PASS: successful task factory starts after progress UI and releases its complete lifecycle")
}
