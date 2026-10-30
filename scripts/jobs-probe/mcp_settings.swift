import AppKit

// Settings ▸ MCP through the pane's own button, on a private pasteboard and a scratch
// helper file, so the case never writes the general pasteboard or reads the bundle it runs in.

/// "mcp settings pane": an executable helper enables Copy MCP Command, whose click puts
/// `AgentServer.mcpCommand` for that path on the pane's pasteboard and shows `copiedStatus`; a
/// helper that is not executable disables the button and shows `missingStatus`; the helper's path
/// is selectable monospace text.
func verifyMCPSettingsPane() throws {
    let tag = "[mcp settings pane]"
    let folder = directory.appendingPathComponent("mcp-pane-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let helper = folder.appendingPathComponent("it's doctored-mcp")
    try Data("#!/bin/sh\n".utf8).write(to: helper)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
    let pasteboard = NSPasteboard(name: NSPasteboard.Name("rz-mcp-pane-\(getpid())"))
    defer { pasteboard.releaseGlobally() }
    pasteboard.clearContents()

    let pane = MCPSettingsPane(helperURL: helper, pasteboard: pasteboard)
    let button = try uiControl(pane, id: "settings.mcp.copy", as: NSButton.self)
    try require(button === pane.copyButton && button.isEnabled && pane.statusLabel.stringValue.isEmpty,
                "\(tag) an executable helper enables the button with no status: "
                + "\(button.isEnabled) \"\(pane.statusLabel.stringValue)\"")
    try require(pane.helperLabel.stringValue == helper.path && pane.helperLabel.isSelectable
                && pane.helperLabel.font?.isFixedPitch == true,
                "\(tag) the helper's path is selectable monospace text: \(pane.helperLabel.stringValue)")
    try uiSend(button, "\(tag) Copy MCP Command")
    let expected = AgentServer.mcpCommand(helperPath: helper.path)
    try require(expected == "claude mcp add --scope user doctored -- '"
                + helper.path.replacingOccurrences(of: "'", with: "'\\''") + "'",
                "\(tag) the command quotes the path for a POSIX shell: \(expected)")
    try require(pasteboard.string(forType: .string) == expected,
                "\(tag) the click puts the command on the pane's pasteboard: "
                + "\(pasteboard.string(forType: .string) ?? "nothing")")
    try require(pane.statusLabel.stringValue == MCPSettingsPane.copiedStatus,
                "\(tag) the status says it was copied: \(pane.statusLabel.stringValue)")

    let missing = folder.appendingPathComponent("not-executable")
    try Data("#!/bin/sh\n".utf8).write(to: missing)
    try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: missing.path)
    let absent = MCPSettingsPane(helperURL: missing, pasteboard: pasteboard)
    try require(!absent.copyButton.isEnabled && absent.statusLabel.stringValue == MCPSettingsPane.missingStatus,
                "\(tag) a helper that is not executable disables the button and says so: "
                + "\(absent.copyButton.isEnabled) \"\(absent.statusLabel.stringValue)\"")
    print("PASS: mcp settings pane — an executable helper enables Copy MCP Command, whose click puts the "
        + "quoted claude mcp add line on the pane's own pasteboard and says Copied; a helper that is not "
        + "executable disables it with the missing sentence; the path is selectable monospace text")
}
