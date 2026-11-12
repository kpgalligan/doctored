import AppKit
import CryptoKit

// The screens probe: builds the app's real windows in this process, in one look
// (`DR_SCREENS_LOOK`, set on NSApp before any window exists), runs each screen of the plan
// (`DR_SCREENS_PLAN`, plan.swift; scenes.swift runs it over the tables of windows.swift,
// editor_tables.swift and actions.swift) and writes its captures and `manifest.json` under
// `DR_SCREENS_OUT` (capture.swift). `scripts/screens-probe/run.py` runs it once per look.

let environment = ProcessInfo.processInfo.environment

func fail(_ reason: String) -> Never {
    print("FAIL: \(reason)")
    fflush(stdout)
    exit(1)
}

guard let outPath = environment["DR_SCREENS_OUT"], !outPath.isEmpty else {
    fail("DR_SCREENS_OUT is not set (run.py sets it)")
}
guard let look = Look(rawValue: environment["DR_SCREENS_LOOK"] ?? "") else {
    fail("DR_SCREENS_LOOK is not light or dark")
}
guard let mechanism = Mechanism(rawValue: environment["DR_SCREENS_MECHANISM"] ?? "") else {
    fail("DR_SCREENS_MECHANISM is not server or cache")
}
guard let planPath = environment["DR_SCREENS_PLAN"], !planPath.isEmpty else {
    fail("DR_SCREENS_PLAN is not set (run.py sets it)")
}
let out = URL(fileURLWithPath: outPath, isDirectory: true)
// run.py starts the probe in the repository.
let labeller = Labeller(folders: [
    (outPath, "<out>"),
    (FileManager.default.currentDirectoryPath, "<repo>"),
    (environment["DR_SCREENS_FIXTURES_ROOT"] ?? "", "<fixtures>"),
    (NSHomeDirectory(), "<home>"),
    (NSTemporaryDirectory(), "<tmp>"),
], email: environment["DR_SCREENS_EMAIL"])

_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
_ = NSApp.setActivationPolicy(.accessory)
NSApp.appearance = NSAppearance(named: look.appearanceName)
// AppDelegate does this at launch, and a probe has no launch.
WorkingCMYK.install()
do { try installAppIcon() } catch { fail(labeller.label("\(error)")) }
// Never CGRequestScreenCaptureAccess(): that would put up a system prompt.
let granted = CGPreflightScreenCaptureAccess()
if mechanism == .server && !granted {
    fail("screen recording is not granted to this terminal; grant it, or run with --fallback-cache "
         + "(cacheDisplay, no title bar or window shape)")
}

let plan: (screens: [Screen], skipped: [[String: String]])
do {
    plan = try parsePlan(try Data(contentsOf: URL(fileURLWithPath: planPath)))
} catch {
    fail(labeller.label("the plan: \(error)"))
}

do {
    try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
} catch {
    fail(labeller.label("the output folder: \(error)"))
}

func sha256(_ url: URL) -> String? {
    guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
    return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

let recorder = Recorder(out: out, look: look, mechanism: mechanism, labeller: labeller)
var failures: [String] = []
var fixtures: [[String: String]] = []
for fixture in plan.screens.compactMap(\.fixture) where !fixtures.contains(where: { $0["id"] == fixture.id }) {
    fixtures.append(["id": fixture.id, "sha256": sha256(fixture.path) ?? "unreadable"])
}
print(labeller.label("look \(look.rawValue), mechanism \(mechanism.rawValue), appearance "
      + "\(NSApp.effectiveAppearance.name.rawValue), screen recording \(granted ? "granted" : "not granted"), "
      + "\(placement), \(plan.screens.count) screens, \(plan.skipped.count) skipped"))
fflush(stdout)

for screen in plan.screens {
    print("screen \(screen.id)")
    fflush(stdout)
    do {
        try runScreen(screen, recorder)
    } catch {
        let reason = labeller.label("\(screen.id): \(error)")
        failures.append(reason)
        print("screen \(screen.id) failed: \(reason)")
        recorder.failed(screen.id, screen.captures, error: String(describing: error))
    }
    settle(nil)
}

let manifest: [String: Any] = [
    "look": look.rawValue,
    "mechanism": mechanism.rawValue,
    "scale": 2,
    "effective_appearance": NSApp.effectiveAppearance.name.rawValue,
    "screen_capture_granted": granted,
    "displays": NSScreen.screens.map { screen -> [String: Any] in
        let f = screen.frame
        return ["backing_scale": Double(screen.backingScaleFactor),
                "frame": [f.minX, f.minY, f.width, f.height].map(Double.init)]
    },
    "fixtures": fixtures,
    "captures": recorder.captures,
    "skipped": plan.skipped.map { $0.mapValues { labeller.label($0) } },
    "failures": failures,
]
do {
    let data = try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: out.appendingPathComponent("manifest.json"))
    print("wrote manifest.json")
} catch {
    fail(labeller.label("manifest.json: \(error)"))
}

let unwritten = recorder.captures.filter { $0["file"] == nil }
if !failures.isEmpty || !unwritten.isEmpty {
    fail("\(failures.count) screen failures, \(unwritten.count) captures without a file")
}
print("PASS: \(recorder.captures.count) captures, \(plan.skipped.count) skipped")
fflush(stdout)
exit(0)
