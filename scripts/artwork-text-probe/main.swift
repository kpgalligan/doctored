import AppKit
import Foundation
_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
NSApp.setActivationPolicy(.prohibited)
do {
    guard let path = ProcessInfo.processInfo.environment["DR_ARTWORK_TEXT_PROBE_DIR"] else {
        throw DoctoredCoreError(message: "Missing private text probe directory")
    }
    let directory = URL(fileURLWithPath: path)
    var report = try runArtworkTextCaptureProbe(directory: directory)
    #if ARTWORK_WORKFLOW_UI
    report["workflow_ui"] = try runArtworkUIWorkflowProbe(directory: directory)
    #endif
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("host-report.json"))
    FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]))
} catch {
    FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8))
    exit(1)
}
