import AppKit

/// The per-case registry check. After case `name`, pumps the main run
/// loop until `SelectionWorkBudget.scratchBytes` is back at `entry`, the bytes
/// it found registered when it began, and reports how long that took.
/// Entry-relative, so only a case that leaves preview reservations behind
/// fails; later cases do not cascade. The verdict is load-independent: the
/// 120 s ceiling only catches reservations that are never released (a slow
/// drain on a busy machine is not a failure), and the time is printed, not
/// asserted. Always prints the INFO line; a leftover writes one FAIL line to
/// stderr and answers false.
func previewRegistryDrained(after name: String, entry: UInt64) -> Bool {
    let start = Date()
    let deadline = start.addingTimeInterval(120)
    while SelectionWorkBudget.scratchBytes > entry, Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    let now = SelectionWorkBudget.scratchBytes
    let seconds = String(format: "%.2f", Date().timeIntervalSince(start))
    print("INFO: preview registry after [\(name)]: \(now) B (\(entry) B at entry), drained in \(seconds) s")
    guard now > entry else { return true }
    fputs("FAIL [\(name)]: left \(now - entry) B of preview reservations registered in "
          + "SelectionWorkBudget\n", stderr)
    return false
}
