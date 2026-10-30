import AppKit
import ObjectiveC.runtime

// The hook uses `class_replaceMethod` on `NSAlert` (`runModal`,
// `beginSheetModal(for:completionHandler:)`: first button), `NSApplication` (both `presentError`
// forms: not recovered, no UI) and `NSDocument.canClose(withDelegate:shouldClose:contextInfo:)` (an
// edited document's close is cancelled, never saved or discarded; a clean one passes through). Each
// event is one `[AutomationAlerts]` stderr line and an entry in `AutomationAlerts.recorded`. File
// panels, `NSApp.runModal(for:)` and modal sessions are untouched.

/// Automated runs never block on a modal alert. When enabled, one runtime hook
/// answers every `NSAlert` with its first button (`runModal` synchronously,
/// `beginSheetModal(for:completionHandler:)` on the next main run-loop turn
/// with no sheet attached), answers every error presentation as "not
/// recovered" without UI, and cancels NSDocument's unsaved-changes review, so
/// a close never saves and never discards. Each event is written to stderr as
/// one `[AutomationAlerts]` line and appended to `recorded` for probes.
///
/// Two switches turn it on: `DR_BACKGROUND=1` (`BackgroundLaunch`) and
/// `DR_AUTOMATION_ALERTS=record`, which every `scripts/` probe runner and
/// launcher sets. With neither, `installIfEnabled()` returns before touching
/// the runtime, so a normal launch is unaffected.
///
/// `NSApplication` is the only error hook: the NSResponder, NSWindow,
/// NSDocument and NSDocumentController `presentError` forms all forward to
/// NSApp. Like `NSApplication` itself, both error hooks present and record
/// nothing for `NSCocoaErrorDomain`/`NSUserCancelledError` — the code a
/// user cancel (`JobCancelled`) bridges to — and answer "not recovered".
/// `canCloseDocumentWithDelegate:shouldCloseSelector:contextInfo:` is
/// the only NSDocument hook: every unsaved-changes review runs inside it,
/// however AppKit draws it, and a first-button answer there would be "Save",
/// which overwrites a titled source in place. A clean document passes through
/// to the original implementation.
///
/// Each replacement is installed on the class that owns the selector, so a
/// probe's temporary interceptor that swaps the CURRENT implementation of the
/// same Method stacks on the hook and restores it. File panels,
/// `NSApp.runModal(for:)` and modal sessions (`BackgroundWork`,
/// `ActionRunProgress`) are left alone.
struct AutomationAlertRecord: Equatable {
    enum Kind: String {
        case alertModal = "NSAlert.runModal"
        case alertSheet = "NSAlert.beginSheetModal"
        case errorModal = "NSApplication.presentError"
        case errorSheet = "NSApplication.presentError(modalFor:)"
        case documentCloseReview = "NSDocument.canClose"
    }

    let kind: Kind
    let messageText: String
    let informativeText: String
    let buttonTitles: [String]
    /// The first button title ("OK" when none); "not recovered" for errors;
    /// "cancel" for a close review.
    let answer: String
}

enum AutomationAlerts {
    static let environmentKey = "DR_AUTOMATION_ALERTS"
    static let recordValue = "record"
    static let isEnabled: Bool = BackgroundLaunch.isEnabled
        || ProcessInfo.processInfo.environment[environmentKey] == recordValue

    private(set) static var isInstalled = false
    private(set) static var recorded: [AutomationAlertRecord] = []

    static func installIfEnabled() {
        guard isEnabled, !isInstalled else { return }
        isInstalled = true
        installAlertModal()
        installAlertSheet()
        installErrorModal()
        installErrorSheet()
        installDocumentCloseReview()
    }

    static func reset() {
        recorded = []
    }

    private static let firstButton = NSApplication.ModalResponse.alertFirstButtonReturn.rawValue

    private static func record(_ record: AutomationAlertRecord) {
        recorded.append(record)
        func escaped(_ text: String) -> String { text.replacingOccurrences(of: "\n", with: "\\n") }
        let line = "[AutomationAlerts] \(record.kind.rawValue): \(escaped(record.messageText))"
            + " | \(escaped(record.informativeText)) | answered: \(escaped(record.answer))\n"
        try? FileHandle.standardError.write(contentsOf: Data(line.utf8))
    }

    private static func recordAlert(_ alert: NSAlert, kind: AutomationAlertRecord.Kind) {
        let titles = alert.buttons.map(\.title)
        record(AutomationAlertRecord(kind: kind, messageText: alert.messageText,
                                     informativeText: alert.informativeText,
                                     buttonTitles: titles, answer: titles.first ?? "OK"))
    }

    private static func recordError(_ error: NSError, kind: AutomationAlertRecord.Kind) {
        let alert = NSAlert(error: error)
        record(AutomationAlertRecord(kind: kind, messageText: alert.messageText,
                                     informativeText: alert.informativeText,
                                     buttonTitles: alert.buttons.map(\.title),
                                     answer: "not recovered"))
    }

    /// AppKit's own rule: a user cancel is never presented.
    private static func isUserCancel(_ error: NSError) -> Bool {
        error.domain == NSCocoaErrorDomain && error.code == NSUserCancelledError
    }

    private static func replace(_ cls: AnyClass, _ selector: Selector, with block: Any) {
        guard let method = class_getInstanceMethod(cls, selector) else { return }
        class_replaceMethod(cls, selector, imp_implementationWithBlock(block),
                            method_getTypeEncoding(method))
    }

    private static func installAlertModal() {
        let block: @convention(block) (NSAlert) -> Int = { alert in
            recordAlert(alert, kind: .alertModal)
            return firstButton
        }
        replace(NSAlert.self, #selector(NSAlert.runModal), with: block)
    }

    private static func installAlertSheet() {
        let block: @convention(block) (NSAlert, NSWindow, (@convention(block) (Int) -> Void)?)
            -> Void = { alert, _, handler in
                recordAlert(alert, kind: .alertSheet)
                guard let handler else { return }
                RunLoop.main.perform(inModes: [.common]) { handler(firstButton) }
            }
        replace(NSAlert.self, #selector(NSAlert.beginSheetModal(for:completionHandler:)),
                with: block)
    }

    private static func installErrorModal() {
        let block: @convention(block) (NSApplication, NSError) -> Bool = { _, error in
            if !isUserCancel(error) { recordError(error, kind: .errorModal) }
            return false
        }
        replace(NSApplication.self, #selector(NSApplication.presentError(_:)), with: block)
    }

    private static func installErrorSheet() {
        typealias DidPresent = @convention(c) (AnyObject, Selector, ObjCBool, UnsafeMutableRawPointer?)
            -> Void
        let block: @convention(block) (NSApplication, NSError, NSWindow, AnyObject?, Selector?,
                                       UnsafeMutableRawPointer?) -> Void = {
            _, error, _, delegate, selector, contextInfo in
            if !isUserCancel(error) { recordError(error, kind: .errorSheet) }
            guard let delegate, let selector else { return }
            RunLoop.main.perform(inModes: [.common]) {
                guard let cls = object_getClass(delegate),
                      let imp = class_getMethodImplementation(cls, selector) else { return }
                let didPresent = unsafeBitCast(imp, to: DidPresent.self)
                didPresent(delegate, selector, ObjCBool(false), contextInfo)
            }
        }
        replace(NSApplication.self,
                #selector(NSApplication.presentError(_:modalFor:delegate:didPresent:contextInfo:)),
                with: block)
    }

    private static func installDocumentCloseReview() {
        typealias CanClose = @convention(c) (NSDocument, Selector, AnyObject?, Selector?,
                                             UnsafeMutableRawPointer?) -> Void
        typealias ShouldClose = @convention(c) (AnyObject, Selector, NSDocument, ObjCBool,
                                                UnsafeMutableRawPointer?) -> Void
        let selector = #selector(NSDocument.canClose(withDelegate:shouldClose:contextInfo:))
        guard let method = class_getInstanceMethod(NSDocument.self, selector) else { return }
        let original = unsafeBitCast(method_getImplementation(method), to: CanClose.self)
        let block: @convention(block) (NSDocument, AnyObject?, Selector?, UnsafeMutableRawPointer?)
            -> Void = { document, delegate, shouldClose, contextInfo in
                guard document.isDocumentEdited else {
                    original(document, selector, delegate, shouldClose, contextInfo)
                    return
                }
                record(AutomationAlertRecord(kind: .documentCloseReview,
                                             messageText: document.displayName,
                                             informativeText: "Unsaved changes kept; close cancelled.",
                                             buttonTitles: [], answer: "cancel"))
                guard let delegate, let shouldClose else { return }
                RunLoop.main.perform(inModes: [.common]) {
                    guard let cls = object_getClass(delegate),
                          let imp = class_getMethodImplementation(cls, shouldClose) else { return }
                    let answer = unsafeBitCast(imp, to: ShouldClose.self)
                    answer(delegate, shouldClose, document, ObjCBool(false), contextInfo)
                }
            }
        replace(NSDocument.self, selector, with: block)
    }
}
