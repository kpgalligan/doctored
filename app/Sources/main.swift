import AppKit
AgentCatalogSnapshot.writeIfAsked()
DataMigration.runAtLaunch()
let delegate = AppDelegate()
NSApplication.shared.delegate = delegate
_ = NSApplicationMain(CommandLine.argc, CommandLine.unsafeArgv)
