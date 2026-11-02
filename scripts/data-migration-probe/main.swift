import Foundation

// The unit probe of `DataMigration` (the Rasterize-to-Doctored rename's
// one-time move, and the Develop Filter preset folder's) and of the Develop
// Filter preset's earlier key. Every case runs on folders under a scratch directory and on
// two scratch defaults suites named after this process; nothing here reads or
// writes the owner's Application Support folders or their defaults domains,
// and `runAtLaunch` is never called. Run by `verify.py`; prints PASS/FAIL per
// case and exits non-zero on any FAIL.

var failures = 0

func report(_ ok: Bool, _ name: String, _ detail: String = "") {
    if ok {
        print("PASS \(name)")
    } else {
        failures += 1
        print("FAIL \(name)" + (detail.isEmpty ? "" : ": \(detail)"))
    }
}

let fm = FileManager.default
let scratch = URL(fileURLWithPath: NSTemporaryDirectory())
    .appendingPathComponent("data-migration-probe-" + UUID().uuidString, isDirectory: true)
let oldSuite = "com.kgalligan.doctored.migration-probe.\(getpid()).old"
let newSuite = "com.kgalligan.doctored.migration-probe.\(getpid()).new"
var caseNumber = 0

/// A fresh (old, new) folder pair under the scratch directory; neither exists.
func folders() -> (URL, URL) {
    caseNumber += 1
    let root = scratch.appendingPathComponent("case\(caseNumber)", isDirectory: true)
    try? fm.createDirectory(at: root, withIntermediateDirectories: true)
    return (root.appendingPathComponent("Rasterize", isDirectory: true),
            root.appendingPathComponent("Doctored", isDirectory: true))
}

func write(_ text: String, to url: URL) {
    try? fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    fm.createFile(atPath: url.path, contents: Data(text.utf8))
}

func read(_ url: URL) -> String? {
    fm.contents(atPath: url.path).flatMap { String(data: $0, encoding: .utf8) }
}

/// Every relative path under `folder`, sorted, with each file's contents.
func listing(_ folder: URL) -> [String] {
    guard let walker = fm.enumerator(atPath: folder.path) else { return [] }
    var out: [String] = []
    while let relative = walker.nextObject() as? String {
        out.append(relative + "=" + (read(folder.appendingPathComponent(relative)) ?? "<dir>"))
    }
    return out.sorted()
}

/// Migrates a pair with the scratch suites, which the folder cases leave
/// marked after the first call (the defaults half has its own cases below).
func migrate(_ old: URL, _ new: URL) -> DataMigration.Outcome {
    DataMigration.migrate(oldFolder: old, newFolder: new, oldDomain: oldSuite, newDomain: newSuite)
}

func seed(_ old: URL) {
    write("sk-fake", to: old.appendingPathComponent("anthropic_api_key"))
    write("{}", to: old.appendingPathComponent("actions/Logo.json"))
    write("preset", to: old.appendingPathComponent("RAW Presets/Warm.json"))
}

// 1. The new folder absent: one move, the old contents now under the new name.
do {
    let (old, new) = folders()
    seed(old)
    let before = listing(old)
    let outcome = migrate(old, new)
    report(outcome.movedFolder && !fm.fileExists(atPath: old.path) && listing(new) == before,
           "moves when the new folder is absent", "\(outcome) \(listing(new))")
}

// 2. The new folder holding only the helper's pre-launch layout: replaced.
do {
    let (old, new) = folders()
    seed(old)
    let before = listing(old)
    write("", to: new.appendingPathComponent("Agent/agent.launch.lock"))
    write("", to: new.appendingPathComponent("Agent/agent.sock"))
    let outcome = migrate(old, new)
    report(outcome.movedFolder && !fm.fileExists(atPath: old.path) && listing(new) == before,
           "moves over the helper's Agent stub", "\(outcome) \(listing(new))")
}

// 3. The new folder holding anything else: nothing moves, both untouched.
do {
    let (old, new) = folders()
    seed(old)
    write("", to: new.appendingPathComponent("Agent/agent.launch.lock"))
    write("{}", to: new.appendingPathComponent("actions/Mine.json"))
    let (oldBefore, newBefore) = (listing(old), listing(new))
    let outcome = migrate(old, new)
    report(!outcome.movedFolder && listing(old) == oldBefore && listing(new) == newBefore,
           "no-op when the new folder holds other data", "\(outcome)")
}

// 4. The old folder absent: nothing moves, nothing is made.
do {
    let (old, new) = folders()
    let outcome = migrate(old, new)
    report(!outcome.movedFolder && !fm.fileExists(atPath: new.path) && !fm.fileExists(atPath: old.path),
           "no-op when the old folder is absent", "\(outcome)")
}

// 5. The default paths sit under Doctored (path computation only, no file I/O).
do {
    unsetenv("DR_ACTIONS_DIR")
    let actions = ActionLibrary.directory.path
    let key = APIKeyStore.keyFileURL?.path ?? ""
    let socket = AgentServer.defaultSocketPath ?? ""
    report(actions.hasSuffix("Application Support/Doctored/actions"), "actions folder default", actions)
    report(key.hasSuffix("Application Support/Doctored/anthropic_api_key"), "API key file default", key)
    report(socket.hasSuffix("Application Support/Doctored/Agent/agent.sock"), "agent socket default", socket)
}

// 6. The defaults: copied once, the new domain's own keys win, the old untouched.
do {
    let defaults = UserDefaults.standard
    defaults.removePersistentDomain(forName: oldSuite)
    defaults.removePersistentDomain(forName: newSuite)
    let old: [String: Any] = ["WorkingLimitBytes": 123, "Shared": "old", "OnlyOld": true]
    defaults.setPersistentDomain(old, forName: oldSuite)
    defaults.setPersistentDomain(["Shared": "new"], forName: newSuite)
    let (oldFolder, newFolder) = folders()
    let first = migrate(oldFolder, newFolder)
    let copied = defaults.persistentDomain(forName: newSuite) ?? [:]
    report(first.copiedDefaults && copied["WorkingLimitBytes"] as? Int == 123
           && copied["OnlyOld"] as? Bool == true && copied["Shared"] as? String == "new"
           && copied[DataMigration.markerKey] as? Bool == true,
           "defaults copied once, new keys win, marker set", "\(first) \(copied)")
    defaults.setPersistentDomain(old.merging(["Later": 1]) { $1 }, forName: oldSuite)
    let second = migrate(oldFolder, newFolder)
    let after = defaults.persistentDomain(forName: newSuite) ?? [:]
    report(!second.copiedDefaults && after["Later"] == nil, "a second call copies nothing", "\(second)")
    let oldAfter = defaults.persistentDomain(forName: oldSuite) ?? [:]
    report(oldAfter["Later"] as? Int == 1 && oldAfter[DataMigration.markerKey] == nil
           && oldAfter.count == 4, "the old domain is never written", "\(oldAfter)")
    defaults.removePersistentDomain(forName: oldSuite)
    defaults.removePersistentDomain(forName: newSuite)
}

// 7. Automation launches skip the migration: `AutomationLaunch.isAutomation`, the broad test
// `runAtLaunch` asks first (and `AppUpdater` too).
do {
    let path = "/probe-home/Library/Application Support/Doctored/Agent/agent.sock"
    for key in ["DR_BACKGROUND", "DR_AUTOMATION_ALERTS"] {
        report(AutomationLaunch.isAutomation([key: "1"], defaultSocketPath: path), "automation: \(key)")
    }
    report(AutomationLaunch.isAutomation(["DR_AGENT_SOCKET": "/tmp/s/agent.sock"], defaultSocketPath: path),
           "automation: a scratch DR_AGENT_SOCKET")
    report(!AutomationLaunch.isAutomation([:], defaultSocketPath: path), "an ordinary launch migrates")
    report(!AutomationLaunch.isAutomation(["DR_AGENT_SOCKET": path], defaultSocketPath: path),
           "the default socket is not automation")
    report(!AutomationLaunch.isAutomation(["DR_BACKGROUND": ""], defaultSocketPath: path),
           "an empty variable is not automation")
    // The pre-rename spellings: an old script's RZ_* still marks automation,
    // whichever variable it is, so it can never migrate the owner's folder.
    for key in ["RZ_BACKGROUND", "RZ_AUTOMATION_ALERTS", "RZ_AGENT_SOCKET", "RZ_ACTIONS_DIR"] {
        report(AutomationLaunch.isAutomation([key: "1"], defaultSocketPath: path),
               "automation: legacy \(key)")
    }
    report(AutomationLaunch.isAutomation(["RZ_AGENT_SOCKET": path], defaultSocketPath: path),
           "automation: a legacy RZ_AGENT_SOCKET even at the default path")
    report(!AutomationLaunch.isAutomation(["RZ_BACKGROUND": ""], defaultSocketPath: path),
           "an empty legacy variable is not automation")
}

/// A fresh (Camera Raw Filter Presets, Develop Filter Presets) pair under the scratch
/// directory; neither exists.
func renamedFolders() -> (URL, URL) {
    caseNumber += 1
    let root = scratch.appendingPathComponent("case\(caseNumber)", isDirectory: true)
    try? fm.createDirectory(at: root, withIntermediateDirectories: true)
    return (root.appendingPathComponent("Camera Raw Filter Presets", isDirectory: true),
            root.appendingPathComponent("Develop Filter Presets", isDirectory: true))
}

// 8. The renamed preset folder (`moveRenamedFolder`): moved once, a second call moves nothing,
// and a new folder that exists (holding presets or empty) wins with both left as they were.
do {
    let (old, new) = renamedFolders()
    write("{\"exposure\":1}", to: old.appendingPathComponent("Warm.json"))
    let before = listing(old)
    let moved = DataMigration.moveRenamedFolder(from: old, to: new)
    report(moved && !fm.fileExists(atPath: old.path) && listing(new) == before,
           "the renamed preset folder moves", "\(moved) \(listing(new))")
    let again = DataMigration.moveRenamedFolder(from: old, to: new)
    report(!again && !fm.fileExists(atPath: old.path) && listing(new) == before,
           "a second move call moves nothing", "\(again) \(listing(new))")
    write("{\"contrast\":20}", to: old.appendingPathComponent("Cool.json"))
    let (oldBefore, newBefore) = (listing(old), listing(new))
    let blocked = DataMigration.moveRenamedFolder(from: old, to: new)
    report(!blocked && listing(old) == oldBefore && listing(new) == newBefore,
           "an existing new preset folder wins, both untouched", "\(blocked) \(listing(old)) \(listing(new))")
}
do {
    let (old, new) = renamedFolders()
    write("{\"exposure\":1}", to: old.appendingPathComponent("Warm.json"))
    try? fm.createDirectory(at: new, withIntermediateDirectories: true)
    let oldBefore = listing(old)
    let moved = DataMigration.moveRenamedFolder(from: old, to: new)
    report(!moved && listing(old) == oldBefore && fm.fileExists(atPath: new.path) && listing(new).isEmpty,
           "an empty new preset folder wins, both untouched", "\(moved) \(listing(new))")
}
do {
    let (old, new) = renamedFolders()
    let moved = DataMigration.moveRenamedFolder(from: old, to: new)
    report(!moved && !fm.fileExists(atPath: old.path) && !fm.fileExists(atPath: new.path),
           "no old preset folder: nothing moves, nothing is made", "\(moved)")
}

// 9. A Develop Filter preset saved under the earlier key (`LegacyNames.developFilterPresetKey`)
// decodes to the same values as its `develop_filter` twin and re-encodes with `develop_filter`;
// a preset holding both keys is refused.
do {
    let controls: [String: Any] = ["exposure": 0.5, "contrast": 20, "vibrance": -15]
    let header: [String: Any] = ["schema_version": 1, "name": "Warm"]
    let current = try? DevelopFilterPreset.decode(header.merging(["develop_filter": controls]) { $1 })
    let legacy = try? DevelopFilterPreset.decode(header.merging(["camera_raw": controls]) { $1 })
    report(current != nil && legacy != nil && legacy?.name == current?.name
           && legacy?.controls == current?.controls,
           "a camera_raw preset decodes as its develop_filter twin",
           "\(String(describing: legacy?.controls.values)) vs \(String(describing: current?.controls.values))")
    let resaved = legacy?.json ?? [:]
    let stored = resaved["develop_filter"] as? [String: Any] ?? [:]
    report(resaved["camera_raw"] == nil && Set(resaved.keys) == ["schema_version", "name", "develop_filter"]
           && NSDictionary(dictionary: stored).isEqual(to: controls),
           "a camera_raw preset re-encodes with develop_filter", "\(resaved)")
    let both = try? DevelopFilterPreset.decode(
        header.merging(["camera_raw": controls, "develop_filter": controls]) { $1 })
    report(both == nil, "a preset holding both keys is refused")
}

try? fm.removeItem(at: scratch)
exit(failures == 0 ? 0 : 1)
