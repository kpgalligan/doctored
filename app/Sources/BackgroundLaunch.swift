import AppKit

// The `DR_BACKGROUND=1` switch sets the `.accessory` activation policy at launch, `present`/`show`
// order windows behind while the app is inactive, and the two `NSApp.activate` sites
// (`ModalCardWindowController.runModal`, `ActionsWindowController.show`) are guarded by
// `keepsWindowsBehind`.

/// The `DR_BACKGROUND=1` test-launch switch: an instance started with it runs
/// as an `.accessory` app (no Dock icon, no menu bar), never activates itself,
/// and orders every window it presents behind other apps' windows, so a test
/// launch never takes over the desktop.
///
/// The "behind" half applies only while the app is inactive: an `.accessory`
/// app that the user (or a probe) activates — by clicking one of its windows —
/// must then behave exactly as it does without the flag.
enum BackgroundLaunch {
    static let isEnabled: Bool = ProcessInfo.processInfo.environment["DR_BACKGROUND"] == "1"

    static var keepsWindowsBehind: Bool { isEnabled && !NSApp.isActive }

    static func applyActivationPolicy() {
        guard isEnabled else { return }
        NSApp.setActivationPolicy(.accessory)
    }

    static func present(_ window: NSWindow, makeKey: Bool = true) {
        if keepsWindowsBehind {
            window.orderBack(nil)
        } else if makeKey {
            window.makeKeyAndOrderFront(nil)
        } else {
            window.orderFront(nil)
        }
    }

    static func show(_ controller: NSWindowController) {
        if keepsWindowsBehind {
            controller.window?.orderBack(nil)
        } else {
            controller.showWindow(nil)
            controller.window?.makeKeyAndOrderFront(nil)
        }
    }
}
