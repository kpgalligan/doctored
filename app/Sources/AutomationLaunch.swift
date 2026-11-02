import Foundation

/// The broad test of "a script or probe owns this launch" (any `RZ_*`, `DR_BACKGROUND`,
/// `DR_AUTOMATION_ALERTS`, a scratch agent socket). `DataMigration` and `AppUpdater` skip their
/// work under it. `AutomationAlerts.isEnabled` is the narrower test (`DR_BACKGROUND` or
/// `DR_AUTOMATION_ALERTS` only) that alert answering and `EditorWindowMemory` use.
enum AutomationLaunch {
    /// True for a launch a script or probe owns: `DR_BACKGROUND` or
    /// `DR_AUTOMATION_ALERTS` set, or an agent socket other than the default.
    /// Any non-empty variable with the pre-rename `RZ_` prefix counts too: the
    /// app reads none of them, but a script written before the rename still
    /// sets them, and such a launch must never migrate the owner's folders or
    /// start the updater.
    static func isAutomation(_ environment: [String: String],
                             defaultSocketPath: String?) -> Bool {
        if environment.contains(where: { $0.key.hasPrefix("RZ_") && !$0.value.isEmpty }) {
            return true
        }
        if environment["DR_BACKGROUND"]?.isEmpty == false { return true }
        if environment["DR_AUTOMATION_ALERTS"]?.isEmpty == false { return true }
        if let socket = environment[AgentServer.socketEnvironmentKey], !socket.isEmpty,
           socket != defaultSocketPath {
            return true
        }
        return false
    }

    /// This process's answer: its own environment against the default agent socket.
    static var isCurrent: Bool {
        isAutomation(ProcessInfo.processInfo.environment,
                     defaultSocketPath: AgentServer.defaultSocketPath)
    }
}
