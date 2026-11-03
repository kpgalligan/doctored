// swift-tools-version: 6.0
// The assistant helper, Doctored.app/Contents/Helpers/doctored-agent: the agent loop
// in its own process, on Slaude (the Claude API in Swift). agent/CLAUDE.md has the
// rules; the app builds it through scripts/build_agent_helper.sh (SwiftPM release on the
// build/agent-swift scratch path, without whole-module optimization).
import PackageDescription

/// The helper's own targets build warning-free in the loop (`make agent-test` sets
/// DR_AGENT_STRICT=1, so the gate's suite holds them to it); Slaude's are never held to it.
/// The shipping build does not set it: scripts/build_agent_helper.sh runs SwiftPM in a
/// scrubbed environment, so a newer Xcode on the publish runner that adds a diagnostic
/// cannot fail the archive.
let strict: [SwiftSetting] =
    Context.environment["DR_AGENT_STRICT"] == "1" ? [.unsafeFlags(["-warnings-as-errors"])] : []

let package = Package(
    name: "DoctoredAgent",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "doctored-agent", targets: ["doctored-agent"]),
    ],
    dependencies: [
        // Pinned by exact revision; Package.resolved is committed beside this file.
        .package(
            url: "https://github.com/kpgalligan/Slaude",
            revision: "4c6c2ca435350741796c3d461720a61e7149b378"),
    ],
    targets: [
        .target(
            name: "DoctoredAgentKit",
            dependencies: [.product(name: "Anthropic", package: "Slaude")],
            swiftSettings: strict),
        .executableTarget(
            name: "doctored-agent",
            dependencies: ["DoctoredAgentKit", .product(name: "Anthropic", package: "Slaude")],
            swiftSettings: strict),
        .testTarget(
            name: "DoctoredAgentKitTests",
            dependencies: ["DoctoredAgentKit", .product(name: "Anthropic", package: "Slaude")],
            swiftSettings: strict),
    ]
)
