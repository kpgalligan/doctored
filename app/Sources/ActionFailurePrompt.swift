import AppKit

// The prompt an INTERACTIVE Play puts up when a step fails — the
// Actions window's Play and File ▸ Automate ▸ Play Last Action, the two callers
// that pass `prompt: ActionFailurePrompt.ask` to `ActionPlayer.run`. Batch,
// `run_action` and ⌃F never ask: a failed step there follows its own `on_error`
// (or Batch's `stop_on_error`).
//
// The rule: the prompt asks only about a failed step whose `on_error`
// is `stop`. Continue is offered when the step's command could not run ("The
// step “Paste” could not run.") or names an object the document does not
// have ("The step names layer “AF_missing”, which this document does not
// have."); Continue skips that step and plays on, and Stop ends the Play with
// the steps already applied kept. An error INSIDE a command (a `ToolError`
// typed `.commandFailed`) offers OK alone and stops the Play at that step
// ("Gaussian Blur stopped: the selection holds no pixels."). Doctored's own
// reason is the secondary text. Select Inverse with nothing selected is not a
// failure at all (`modify_selection`'s `invert` is a no-op success). The
// buttons are the standard Mac arrangement: Stop then Continue left to
// right, Continue the default.
//
// Test record: the attended takes `doctored-fixtures/photoshop/
// action-failure-472/take2-continue` and `take3-watched` (identical), with
// take1's `paste_empty`, are the record of this behaviour: `paste_empty` and
// `select_missing` prompted and Continue played on, `inverse_none` did not
// fail, `blur_empty` stopped at its step.

/// What kind of failure a step had, for the sentence the prompt shows.
enum ActionStepFailureKind: String {
    /// The command could not run: a tool error, an `$asset` or `$param` that
    /// did not resolve, a geometry remap refusal, a placeholder, an encoding
    /// failure.
    case unavailable
    /// A `$layer`, `$channel`, `$guide` or `$layers` symbol named nothing on
    /// this document.
    case missingObject = "missing_object"
    /// A tool error thrown as `AgentServer.ToolErrorKind.commandFailed`:
    /// the inside-a-command sentence.
    case commandFailed = "command_failed"

    /// Whether the prompt offers Continue; false for `.commandFailed` only.
    var offersContinue: Bool {
        switch self {
        case .unavailable, .missingObject: return true
        case .commandFailed: return false
        }
    }
}

extension AgentServer {
    /// A tool error's typed kind, read by `ActionPlayer` for the
    /// prompt. nil on a `ToolError` reads as an unavailable command. Set only
    /// at the throw sites worded for it: Move and Transform Selected
    /// Pixels on an empty area.
    enum ToolErrorKind: Equatable {
        /// An error inside a command; `cause` is the clause after the colon,
        /// without the final period.
        case commandFailed(cause: String)
    }
}

extension AgentServer.ToolError {
    /// The selected-pixels refusals that are errors inside a command, by
    /// the exact sentence the app or the core throws, with their cause.
    /// The empty-area sentences are `MoveSelectedPixels.emptyRefusal`
    /// and `TransformSelectedPixels.emptyRefusal` (the core's `EMPTY_AREA`
    /// in `selected_pixels.rs` and `selected_pixels_transform.rs`) and a
    /// colour plane's `MoveSelectedPixels.colorPlaneEmptyRefusal`
    /// (`COLOR_EMPTY_AREA` in `selected_color_plane.rs`). The transform has no
    /// pixel-work refusal (it runs as a cancellable job), so its
    /// sentence left this table with it.
    private static let selectedPixelsFailures: [String: String] = [
        MoveSelectedPixels.emptyRefusal: "the selection holds no pixels",
        TransformSelectedPixels.emptyRefusal: "the selection holds no pixels",
        MoveSelectedPixels.colorPlaneEmptyRefusal: "the selection holds no pixels",
    ]

    /// `message` as a ToolError, kinded `.commandFailed` when it is one of
    /// the selected-pixels refusals above; for the Move / Transform Selected
    /// Pixels throw and catch sites.
    static func selectedPixels(_ message: String) -> AgentServer.ToolError {
        AgentServer.ToolError(
            message: message,
            kind: selectedPixelsFailures[message].map { .commandFailed(cause: $0) })
    }
}

/// One failed step, as the prompt shows it.
struct ActionStepFailure {
    /// 1-based.
    let step: Int
    /// The action's step count.
    let steps: Int
    let tool: String
    /// `ActionCatalogFacts.displayName(for:arguments:)`; "Not recorded —
    /// <name>" for a placeholder.
    let title: String
    /// The same string appended to `ActionRunReport.failures`.
    let reason: String
    let kind: ActionStepFailureKind
    /// The object phrase for a missing object, e.g. `layer “AF_missing”`.
    let object: String?
    /// The clause after the colon (no final period) for `.commandFailed`;
    /// nil otherwise.
    var cause: String? = nil
}

enum ActionFailureChoice {
    case continueRun
    case stop
}

enum ActionFailurePrompt {
    /// Button titles in NSAlert order; the first is the rightmost, the
    /// Return default and what `AutomationAlerts` answers. The standard Mac
    /// arrangement: Stop, Continue left to right, Continue the default. A
    /// command failure offers only OK, which stops.
    static func buttonTitles(for kind: ActionStepFailureKind) -> [String] {
        kind.offersContinue ? ["Continue", "Stop"] : ["OK"]
    }

    /// The sentence for a step whose command could not run, its slot `{}`
    /// the step's title.
    private static let unavailableTemplate = "The step “{}” could not run."
    /// The sentence for a step naming an object the document does not have,
    /// its slot the object phrase (`layer “X”`).
    private static let missingObjectTemplate = "The step names {}, which this document does not have."
    /// The sentence for an error inside a command: the step's title, then
    /// the cause.
    private static let commandFailedTemplate = "{} stopped: {}."

    /// The sentence for `failure.kind` with its slots filled.
    static func message(for failure: ActionStepFailure) -> String {
        switch failure.kind {
        case .unavailable:
            return fill(unavailableTemplate, with: [failure.title])
        case .missingObject:
            // A `$layers` miss names no single object, so it reads as an
            // unavailable command.
            guard let object = failure.object else {
                return fill(unavailableTemplate, with: [failure.title])
            }
            return fill(missingObjectTemplate, with: [object])
        case .commandFailed:
            return fill(commandFailedTemplate, with: [failure.title, failure.cause ?? "it could not finish"])
        }
    }

    /// messageText = `message(for:)`; informativeText = the reason; buttons =
    /// `buttonTitles(for:)`.
    static func alert(for failure: ActionStepFailure) -> NSAlert {
        let alert = NSAlert()
        alert.messageText = message(for: failure)
        alert.informativeText = failure.reason
        for title in buttonTitles(for: failure.kind) {
            alert.addButton(withTitle: title)
        }
        return alert
    }

    /// `runModal` of `alert(for:)`, mapped through `buttonTitles(for:)` to a
    /// choice; an unrecognized response, and any answer to an alert that
    /// offers no Continue, stops.
    static func ask(_ failure: ActionStepFailure) -> ActionFailureChoice {
        let response = alert(for: failure).runModal()
        let titles = buttonTitles(for: failure.kind)
        let index = response.rawValue
            - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
        guard failure.kind.offersContinue, titles.indices.contains(index) else { return .stop }
        return titles[index] == "Continue" ? .continueRun : .stop
    }

    /// `template` with each `{}` replaced, in order, by the next slot.
    private static func fill(_ template: String, with slots: [String]) -> String {
        var parts = template.components(separatedBy: "{}")
        var out = parts.removeFirst()
        for (index, part) in parts.enumerated() {
            out += (slots.indices.contains(index) ? slots[index] : "") + part
        }
        return out
    }
}
