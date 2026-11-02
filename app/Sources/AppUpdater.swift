import AppKit
#if DR_UPDATER
import Sparkle
#endif

/// Doctored ▸ Check for Updates… and Sparkle's scheduled check: the published build's
/// updater (designs/sparkle-updates.md).
/// Distribution's alone: Sparkle is named only inside `#if DR_UPDATER`, which project.yml
/// sets in Distribution and nowhere else, because `make typecheck` and every probe compile
/// app/Sources with no Sparkle module and Debug/Release must keep no Sparkle load command
/// (an ad-hoc hardened build cannot load the framework). `availability` is compiled in every
/// build so a probe can test it.
/// Who gets one: a published build's ordinary launch. Never under `AutomationLaunch` (Sparkle's
/// windows bypass AutomationAlerts, and its keys would land in the owner's defaults domain);
/// deferred to the first activation in an instance doctored-mcp opened (`helperLaunchArgument`).
/// Distribution writes one `[AppUpdater] <case>` stderr line per decision; updater_check.py D9
/// reads it. No MCP mirror: app-wide, no undo step, nothing a save carries, and an install
/// quits the app. Static state, not an AppDelegate property: probes construct AppDelegate().
enum AppUpdater {
    enum Availability: Equatable { case absent, automation, deferred, started }

    /// What doctored-mcp appends to every launch (mcp-proxy/src/launch.rs HELPER_LAUNCH_ARGUMENT).
    static let helperLaunchArgument = "-DRMCPHelperLaunch"

    static var isCompiledIn: Bool {
        #if DR_UPDATER
        return true
        #else
        return false
        #endif
    }

    /// Pure. `.absent` unless compiled in with an https feed, a 32-byte EdDSA public key and
    /// a positive integer CFBundleVersion (local builds read "0", which sees every release as
    /// newer).
    static func availability(compiledIn: Bool, info: [String: Any], environment: [String: String],
                             arguments: [String], defaultSocketPath: String?) -> Availability {
        guard compiledIn,
              let feed = info["SUFeedURL"] as? String, feed.hasPrefix("https://"),
              let key = info["SUPublicEDKey"] as? String, isPublicKey(key),
              let build = info["CFBundleVersion"] as? String, let number = Int(build), number > 0
        else { return .absent }
        if AutomationLaunch.isAutomation(environment, defaultSocketPath: defaultSocketPath) {
            return .automation
        }
        return arguments.contains(helperLaunchArgument) ? .deferred : .started
    }

    /// Strict base64 of exactly 32 bytes.
    static func isPublicKey(_ text: String) -> Bool {
        guard let data = Data(base64Encoded: text) else { return false }
        return data.count == 32 && data.base64EncodedString() == text
    }

    private(set) static var current: Availability = .absent
    private static var activationObserver: NSObjectProtocol?

    /// From applicationDidFinishLaunching, after bindSocket(). Main only.
    static func startIfEligible() {
        begin(availability(compiledIn: isCompiledIn, info: Bundle.main.infoDictionary ?? [:],
                           environment: ProcessInfo.processInfo.environment,
                           arguments: CommandLine.arguments,
                           defaultSocketPath: AgentServer.defaultSocketPath))
    }

    /// The transition for a decided launch, apart from the decision so the updater probe can
    /// drive `.deferred` in a build without `DR_UPDATER`. Main only.
    static func begin(_ decided: Availability) {
        assert(Thread.isMainThread)
        current = decided
        note(current)
        switch current {
        case .absent, .automation: return
        case .started: start()
        case .deferred:
            // A person may already have activated the instance while it was launching.
            if NSApp.isActive { ensureStarted(); return }
            activationObserver = NotificationCenter.default.addObserver(
                forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { _ in AppUpdater.ensureStarted() }
        }
    }

    static var canCheckForUpdates: Bool {
        switch current {
        case .absent, .automation: return false
        case .deferred: return true
        case .started: return controllerCanCheck
        }
    }

    static func checkForUpdates(_ sender: Any?) {
        guard canCheckForUpdates else { NSSound.beep(); return }
        ensureStarted()
        #if DR_UPDATER
        controller?.checkForUpdates(sender)
        #endif
    }

    private static func ensureStarted() {
        if let observer = activationObserver {
            NotificationCenter.default.removeObserver(observer)
            activationObserver = nil
        }
        guard current == .deferred || current == .started else { return }
        if current == .deferred { current = .started; note(.started) }
        start()
    }

    #if DR_UPDATER
    private static var controller: SPUStandardUpdaterController?
    private static var controllerCanCheck: Bool { controller?.updater.canCheckForUpdates ?? false }
    private static func start() {
        guard controller == nil else { return }
        controller = SPUStandardUpdaterController(
            startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    }
    /// One stderr line per decision, AutomationAlerts's style; updater_check.py D9 reads it.
    private static func note(_ availability: Availability) {
        try? FileHandle.standardError.write(contentsOf: Data("[AppUpdater] \(availability)\n".utf8))
    }
    #else
    private static var controllerCanCheck: Bool { false }
    /// How often the stand-in `start()` ran; the updater probe holds the deferred path to one.
    private(set) static var starts = 0
    private static func start() { starts += 1 }
    private static func note(_ availability: Availability) {}
    #endif
}
