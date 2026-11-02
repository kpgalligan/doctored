import AppKit

// The unit probe of `AppUpdater` (Doctored ▸ Check for Updates…, designs/sparkle-updates.md):
// its pure `availability` decision and `isPublicKey`, the menu item every build carries, and
// this build's own answer. Probes compile app/Sources without `DR_UPDATER` and without a
// Sparkle module, so no updater is ever constructed here: `startIfEligible` must land on
// `.absent`, and `begin`'s transitions (the deferred start above all) run against the stand-in
// `start()` that such a build counts. Nothing is read from or written to the owner's defaults
// domain, and nothing reaches the network. Run by `verify.py`; prints PASS/FAIL per case and exits non-zero on any
// FAIL.

var failures = 0

func report(_ ok: Bool, _ name: String, _ detail: String = "") {
    if ok {
        print("PASS \(name)")
    } else {
        failures += 1
        print("FAIL \(name)" + (detail.isEmpty ? "" : ": \(detail)"))
    }
}

_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
_ = NSApp.setActivationPolicy(.accessory)

let feed = "https://github.com/kpgalligan/doctored/releases/latest/download/appcast.xml"
let key = "v0qZeH0YUq+E1zznO2RjbaKZcIMuKSWM1hfEAuz7otI="
let socketPath = "/probe-home/Library/Application Support/Doctored/Agent/agent.sock"
let executable = "/Applications/Doctored.app/Contents/MacOS/Doctored"
let validInfo: [String: Any] = ["SUFeedURL": feed, "SUPublicEDKey": key, "CFBundleVersion": "42"]

func decide(compiledIn: Bool = true, info: [String: Any] = validInfo,
            environment: [String: String] = [:],
            arguments: [String] = [executable]) -> AppUpdater.Availability {
    AppUpdater.availability(compiledIn: compiledIn, info: info, environment: environment,
                            arguments: arguments, defaultSocketPath: socketPath)
}

func check(_ got: AppUpdater.Availability, _ want: AppUpdater.Availability, _ name: String) {
    report(got == want, "\(name) -> \(want)", "got \(got)")
}

func replacing(_ name: String, with value: Any?) -> [String: Any] {
    var info = validInfo
    info[name] = value
    return info
}

let marker = [executable, AppUpdater.helperLaunchArgument, "YES"]

// 1. The `availability` matrix.
do {
    check(decide(), .started, "an ordinary launch of a published build")
    check(decide(compiledIn: false), .absent, "a build without DR_UPDATER")
    check(decide(compiledIn: false, environment: ["DR_BACKGROUND": "1"], arguments: marker),
          .absent, "no DR_UPDATER wins over automation and the marker")
    let feeds = [("an empty feed", ""), ("an http feed", "http://example.com/appcast.xml"),
                 ("an unexpanded feed", "$(DR_SPARKLE_FEED_URL:default=)")]
    for (label, value) in feeds {
        check(decide(info: replacing("SUFeedURL", with: value)), .absent, label)
    }
    check(decide(info: replacing("SUFeedURL", with: nil)), .absent, "no feed")
    let keys = [("an empty key", ""), ("a 31-byte key", Data(count: 31).base64EncodedString()),
                ("a 33-byte key", Data(count: 33).base64EncodedString()),
                ("a key that is not base64", "not a key")]
    for (label, value) in keys {
        check(decide(info: replacing("SUPublicEDKey", with: value)), .absent, label)
    }
    check(decide(info: replacing("SUPublicEDKey", with: nil)), .absent, "no key")
    for build in ["0", "x", "-3", ""] {
        check(decide(info: replacing("CFBundleVersion", with: build)), .absent,
              "build \"\(build)\"")
    }
    check(decide(info: replacing("CFBundleVersion", with: 42)), .absent,
          "a build that is not a string")
    for (name, value) in [("DR_BACKGROUND", "1"), ("DR_AUTOMATION_ALERTS", "record"),
                          ("DR_AGENT_SOCKET", "/tmp/scratch/agent.sock"), ("RZ_BACKGROUND", "1")] {
        check(decide(environment: [name: value]), .automation, name)
    }
    check(decide(environment: ["DR_AGENT_SOCKET": socketPath]), .started, "the default socket")
    check(decide(environment: ["DR_BACKGROUND": ""]), .started, "an empty DR_BACKGROUND")
    check(decide(arguments: marker), .deferred, "the doctored-mcp marker")
    check(decide(arguments: [executable, "-ApplePersistenceIgnoreState", "YES"]), .started,
          "another argument pair")
    check(decide(environment: ["DR_AGENT_SOCKET": "/tmp/scratch/agent.sock"], arguments: marker),
          .automation, "automation wins over the marker")
    check(decide(environment: ["DR_AGENT_SOCKET": socketPath], arguments: marker), .deferred,
          "the marker on the person's socket")
}

// 2. `isPublicKey`: strict base64 of exactly 32 bytes.
do {
    report(AppUpdater.isPublicKey(key), "the published key is a public key")
    report(AppUpdater.isPublicKey(Data(count: 32).base64EncodedString()),
           "32 zero bytes are a public key")
    let refused: [(String, String)] = [
        ("empty", ""),
        ("31 bytes", Data(count: 31).base64EncodedString()),
        ("33 bytes", Data(count: 33).base64EncodedString()),
        ("64 bytes", Data(count: 64).base64EncodedString()),
        ("no padding", String(key.dropLast())),
        ("non-canonical trailing bits", String(key.dropLast(2)) + "J="),
        ("a trailing newline", key + "\n"),
        ("a leading space", " " + key),
        ("not base64", "v0qZeH0YUq+E1zznO2RjbaKZcIMuKSWM1hfEAuz7ot!="),
    ]
    for (label, text) in refused {
        report(!AppUpdater.isPublicKey(text), "refused: \(label)", text)
    }
}

// 3. Doctored ▸ Check for Updates…: item 1 of the app menu in every build, nil target, no key
// equivalent, disabled in a launch without an updater.
do {
    let delegate = AppDelegate()
    let menu = delegate.buildMainMenu()
    let app = menu.items.first?.submenu
    let items = app?.items ?? []
    if items.count > 2 {
        let item = items[1]
        report(items[0].title == "About Doctored" && items[2].isSeparatorItem,
               "Check for Updates… sits between About and the first separator",
               items.prefix(3).map(\.title).joined(separator: " | "))
        report(item.title == "Check for Updates…", "its title", item.title)
        report(item.action == NSSelectorFromString("checkForUpdates:"), "its action",
               item.action.map(NSStringFromSelector) ?? "nil")
        report(item.target == nil, "a nil target")
        report(item.keyEquivalent.isEmpty, "no key equivalent", item.keyEquivalent)
        report(!delegate.validateMenuItem(item), "disabled without an updater")
    } else {
        report(false, "the app menu has Check for Updates…", "\(items.map(\.title))")
    }
    withExtendedLifetime(delegate) {}
}

// 4. This build: no DR_UPDATER, so `startIfEligible` decides `.absent` and nothing can check.
do {
    report(!AppUpdater.isCompiledIn, "a probe build is not compiled with the updater")
    AppUpdater.startIfEligible()
    report(AppUpdater.current == .absent, "startIfEligible decides absent", "\(AppUpdater.current)")
    report(!AppUpdater.canCheckForUpdates, "nothing can check for updates")
}

// 5. The transition `begin` makes for each decision, through the stand-in `start()` this build
// counts: a deferred updater waits for the first activation or Check for Updates…, starts once,
// and stops listening; `.automation` and `.absent` never start one.
func activate() {
    NotificationCenter.default.post(name: NSApplication.didBecomeActiveNotification, object: NSApp)
    RunLoop.main.run(until: Date().addingTimeInterval(0.1))
}

do {
    if NSApp.isActive {
        report(false, "the probe is inactive, so a deferred updater waits")
    } else {
        var before = AppUpdater.starts
        AppUpdater.begin(.deferred)
        report(AppUpdater.current == .deferred && AppUpdater.starts == before,
               "a deferred updater waits for activation",
               "\(AppUpdater.current), \(AppUpdater.starts - before) starts")
        report(AppUpdater.canCheckForUpdates, "a deferred updater can check for updates")
        activate()
        report(AppUpdater.current == .started && AppUpdater.starts == before + 1,
               "the first activation starts it once",
               "\(AppUpdater.current), \(AppUpdater.starts - before) starts")
        activate()
        report(AppUpdater.starts == before + 1, "a second activation starts nothing",
               "\(AppUpdater.starts - before) starts")

        before = AppUpdater.starts
        AppUpdater.begin(.deferred)
        AppUpdater.checkForUpdates(nil)
        report(AppUpdater.current == .started && AppUpdater.starts == before + 1,
               "Check for Updates… starts a deferred updater once",
               "\(AppUpdater.current), \(AppUpdater.starts - before) starts")
        activate()
        report(AppUpdater.starts == before + 1, "after Check for Updates…, activation starts nothing",
               "\(AppUpdater.starts - before) starts")
    }

    var before = AppUpdater.starts
    AppUpdater.begin(.started)
    report(AppUpdater.current == .started && AppUpdater.starts == before + 1,
           "an ordinary launch starts at once", "\(AppUpdater.starts - before) starts")
    for decided in [AppUpdater.Availability.automation, .absent] {
        before = AppUpdater.starts
        AppUpdater.begin(decided)
        activate()
        report(AppUpdater.current == decided && AppUpdater.starts == before
               && !AppUpdater.canCheckForUpdates,
               "\(decided) starts nothing and cannot check",
               "\(AppUpdater.current), \(AppUpdater.starts - before) starts")
    }
}

exit(failures == 0 ? 0 : 1)
