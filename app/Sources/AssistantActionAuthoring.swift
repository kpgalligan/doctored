import Foundation

/// The assistant's half of Actions: one paragraph spliced into the panel's
/// system prompt.
///
/// Wired into the EXISTING assistant surface rather than a second one — the
/// panel already sees the catalog through `AssistantHelperSession.open` (every
/// `catalogJSON()` entry but close_document), so the Actions and Batch tools
/// join its toolset automatically and nothing here needs a second key gate, a
/// second session or a second view. All this adds is what the model needs
/// before it searches: that Actions exist, which tools to search for, and that
/// a recording captures the user's own edits too. The step shape, the symbols
/// and the tools a step may not name are `save_action`'s catalog description.
///
/// It lives in its own file so the assistant panel's owner and this feature's
/// owner never edit the same lines: the panel's `systemPrompt()` splices this
/// one constant and knows nothing else about Actions.
enum AssistantActionAuthoring {
    /// Spliced into `AssistantPanelViewController.systemPrompt()`.
    ///
    /// Written as one continued line — every line ends in a `\` — because it
    /// is interpolated into the middle of a prompt paragraph, where a stray
    /// newline would read as a section break.
    static let systemPromptSection = """
        You can also build reusable Actions: named lists of tool calls that the user can \
        edit in the Actions window and replay on other documents. save_action creates one \
        from a sentence, list_actions reads one back, run_action plays it on a document as \
        one undo step, run_batch plays it over every image in a folder and delete_action \
        removes it; start_recording and stop_recording capture what the user does at the \
        keyboard as well as your own calls, so "save what I just did" is a recording rather \
        than a guess. Search for save_action before writing steps: its description gives \
        the step shape, the {"$layer": …} symbols, the geometry modes that remap \
        coordinates onto another canvas size, and the tools a step may not name.
        """
}
