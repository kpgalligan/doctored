import Foundation

// The unit probe of `AgentPathGuard`, against a fake home in a
// temporary folder (every home root created except `.kube`, left absent for
// the dangling-link case). Run by `verify.py`; prints PASS/FAIL per case and
// `ENV_FOLDERS: <names>`, and exits non-zero on any FAIL.

var failures = 0

func report(_ ok: Bool, _ name: String, _ detail: String = "") {
    if ok {
        print("PASS \(name)")
    } else {
        failures += 1
        print("FAIL \(name)" + (detail.isEmpty ? "" : ": \(detail)"))
    }
}

let fm = FileManager.default
let scratch = NSTemporaryDirectory() + "path-guard-probe-" + UUID().uuidString
let home = scratch + "/home"
let support = home + "/Library/Application Support/Doctored"
let actions = support + "/actions"
let payloads = support + "/Clipboard Payloads"
let masks = support + "/RAW Local Masks"
let pictures = home + "/Pictures"
let profiles = "/System/Library/ColorSync/Profiles"
let fileRoots: Set<String> = ["~/.netrc", "~/.npmrc", "~/.pypirc", "~/.git-credentials"]

func expand(_ root: String) -> String {
    root.hasPrefix("~/") ? home + root.dropFirst() : root
}

do {
    for root in AgentPathGuard.blockedRoots where root.path.hasPrefix("~/") {
        guard root.path != "~/.kube" else { continue }
        let path = expand(root.path)
        if fileRoots.contains(root.path) {
            try fm.createDirectory(atPath: (path as NSString).deletingLastPathComponent,
                                   withIntermediateDirectories: true)
            fm.createFile(atPath: path, contents: Data("secret".utf8))
        } else {
            try fm.createDirectory(atPath: path, withIntermediateDirectories: true)
        }
    }
    for folder in [actions + "/Logo.assets", payloads, masks, pictures, home + "/Pictures/Foo.app"] {
        try fm.createDirectory(atPath: folder, withIntermediateDirectories: true)
    }
    fm.createFile(atPath: support + "/anthropic_api_key", contents: Data("sk-fake".utf8))
    try fm.createSymbolicLink(atPath: pictures + "/to-ssh", withDestinationPath: home + "/.ssh")
    try fm.createSymbolicLink(atPath: pictures + "/to-kube", withDestinationPath: "../.kube")
    try fm.createSymbolicLink(atPath: actions + "/Logo.assets/key.png",
                              withDestinationPath: support + "/anthropic_api_key")
} catch {
    print("FAIL setup: \(error)")
    exit(1)
}

func makeGuard(environment: [String: String] = [:], cwd: String = "/") -> AgentPathGuard {
    AgentPathGuard(home: home, currentDirectory: cwd, environment: environment,
                   actionsDirectory: actions, clipboardPayloadsDirectory: payloads,
                   profileFolders: [profiles], rawLocalMasksDirectory: masks)
}

let guardian = makeGuard()

func refused(_ path: String, _ access: AgentPathGuard.Access, label: String? = nil,
             guard g: AgentPathGuard = guardian) -> (Bool, String) {
    guard let message = g.refusal(path: path, access: access, argument: "path") else {
        return (false, "allowed")
    }
    if let label, !message.contains(label) { return (false, message) }
    return (true, message)
}

func allowed(_ path: String, _ access: AgentPathGuard.Access,
             guard g: AgentPathGuard = guardian) -> (Bool, String) {
    let message = g.refusal(path: path, access: access, argument: "path")
    return (message == nil, message ?? "")
}

// Every blocked root, read and write, refused with its label.
for root in AgentPathGuard.blockedRoots {
    let base = expand(root.path)
    let path = fileRoots.contains(root.path) ? base : base + "/probe-file"
    for access in [AgentPathGuard.Access.read, .write] {
        let (ok, detail) = refused(path, access, label: root.label)
        report(ok, "blocked root \(root.path) \(access)", detail)
    }
    let (ok, detail) = refused(root.path, .read, label: root.label)
    report(ok, "blocked root \(root.path) as written", detail)
}

// The exact sentence.
let sentence = guardian.refusal(path: "~/.ssh/id_rsa", access: .read, argument: "path")
report(sentence == "Refused: path points into SSH keys (~/.ssh), which agent tools may not "
       + "read or write.", "refusal sentence", sentence ?? "nil")
let writeSentence = guardian.refusal(path: "/Applications/x.png", access: .write,
                                     argument: "destination")
report(writeSentence == "Refused: destination points into the Applications folder "
       + "(/Applications), which agent tools may not write into.", "write-only sentence",
       writeSentence ?? "nil")

// Environment folders: refused when set, allowed when not.
for name in AgentPathGuard.environmentFolders {
    let folder = scratch + "/env/" + name.lowercased()
    let set = makeGuard(environment: [name: folder])
    var (ok, detail) = refused(folder + "/file.json", .read, label: name, guard: set)
    report(ok, "env \(name) set refused", detail)
    (ok, detail) = refused(folder + "/file.json", .write, label: name, guard: set)
    report(ok, "env \(name) set refused write", detail)
    (ok, detail) = allowed(folder + "/file.json", .read,
                           guard: makeGuard(environment: [name: ""]))
    report(ok, "env \(name) empty allowed", detail)
    (ok, detail) = allowed(folder + "/file.json", .write)
    report(ok, "env \(name) unset allowed", detail)
}

// Symlinks, `..` and case.
var (ok, detail) = refused(pictures + "/to-ssh/id_rsa", .read, label: "SSH keys")
report(ok, "symlink into .ssh", detail)
(ok, detail) = refused(pictures + "/to-kube/config", .read, label: "Kubernetes")
report(ok, "dangling symlink into .kube", detail)
(ok, detail) = refused(pictures + "/../.ssh/x", .read, label: "SSH keys")
report(ok, "dotdot into .ssh", detail)
(ok, detail) = allowed(pictures + "/x/../y.png", .write)
report(ok, "dotdot inside Pictures allowed", detail)
(ok, detail) = refused(home + "/.SSH/id_rsa", .read, label: "SSH keys")
report(ok, "upper-case .SSH", detail)
(ok, detail) = refused(home.uppercased() + "/.ssh/id_rsa", .read, label: "SSH keys")
report(ok, "upper-case home prefix", detail)
(ok, detail) = refused(home.uppercased() + "/LIBRARY/APPLICATION SUPPORT/DOCTORED/x", .write,
                       label: "Doctored's Application Support")
report(ok, "upper-case support folder", detail)
// The folder under the name before the rename stays blocked until it is moved.
(ok, detail) = refused(home + "/Library/Application Support/Rasterize/anthropic_api_key", .read,
                       label: "the former Rasterize Application Support folder")
report(ok, "former support folder", detail)

// An ordinary picture.
(ok, detail) = allowed(pictures + "/photo.jpg", .read)
report(ok, "Pictures read allowed", detail)
(ok, detail) = allowed(pictures + "/photo.jpg", .write)
report(ok, "Pictures write allowed", detail)
(ok, detail) = allowed("~/Pictures/photo.jpg", .write)
report(ok, "~/Pictures write allowed", detail)

// Real system paths.
for path in ["/etc/hosts", "/private/etc/hosts", "/System/Library/CoreServices/SystemVersion.plist",
             "/Library/Keychains/x"] {
    (ok, detail) = refused(path, .read)
    report(ok, "system \(path)", detail)
}

// Write-only rules.
for path in [pictures + "/Foo.app/Contents/Resources/x.png", pictures + "/New.APP/x.png",
             "/Applications/x.png", "/applications/Foo/x.png"] {
    (ok, detail) = refused(path, .write)
    report(ok, "write-only refused \(path)", detail)
    (ok, detail) = allowed(path, .read)
    report(ok, "write-only read allowed \(path)", detail)
}

// Read carve-outs: allowed to read, refused to write.
// The side files carve out under their current and their legacy (pre-rename) extensions.
let carveOuts = [actions + "/Logo.assets/logo.png", payloads + "/A1B2.drcb",
                 payloads + "/A1B2.rzcb", masks + "/C3D4.drmask", masks + "/C3D4.rzmask",
                 profiles + "/sRGB Profile.icc"]
for path in carveOuts {
    (ok, detail) = allowed(path, .read)
    report(ok, "carve-out read \(path)", detail)
    (ok, detail) = refused(path, .write)
    report(ok, "carve-out write refused \(path)", detail)
}
for path in [actions + "/logo.json", actions + "/.assets/x.png", actions + "/Logo.assets",
             payloads + "/note.txt", payloads + "/sub/A1B2.drcb", payloads + "/sub/A1B2.rzcb",
             payloads + "/.drcb", payloads + "/.rzcb", masks + "/note.txt",
             masks + "/sub/C3D4.drmask", masks + "/.rzmask", profiles] {
    (ok, detail) = refused(path, .read)
    report(ok, "outside carve-out refused \(path)", detail)
}
// SideFileExtension: writers name `current`, readers take `current` or `legacy`.
let sideExtensions: [(SideFileExtension, String, String)] = [
    (.snapshot, "drsnapshot", "rzsnapshot"), (.actionBundle, "draction", "rzaction"),
    (.clipboardPayload, "drcb", "rzcb"), (.rawLocalMask, "drmask", "rzmask"),
]
report(SideFileExtension.allCases.count == sideExtensions.count, "side extensions: four cases")
for (side, current, legacy) in sideExtensions {
    report(side.current == current && side.legacy == legacy, "side extension \(current) spelling",
           "\(side.current) \(side.legacy)")
    report(side.matches(URL(fileURLWithPath: "/x/a." + current))
           && side.matches(URL(fileURLWithPath: "/x/a." + legacy.uppercased()))
           && side.matches(path: "/x/a." + legacy) && !side.matches(path: "/x/a.png")
           && !side.matches(path: "/x/a." + current + "x"), "side extension \(current) matches")
    report(side.isFileName("a." + current) && side.isFileName("a." + legacy)
           && !side.isFileName("." + current) && !side.isFileName("." + legacy)
           && !side.isFileName("a.png"), "side extension \(current) file names")
}
(ok, detail) = refused(actions + "/Logo.assets/key.png", .read,
                       label: "Doctored's Application Support")
report(ok, "assets symlink to the API key", detail)

// Relative paths resolve against the current directory.
(ok, detail) = refused("id_rsa", .read, label: "SSH keys", guard: makeGuard(cwd: home + "/.ssh"))
report(ok, "relative into .ssh", detail)
(ok, detail) = refused("../.aws/credentials", .read, label: "AWS",
                       guard: makeGuard(cwd: pictures))
report(ok, "relative dotdot into .aws", detail)
(ok, detail) = allowed("photo.png", .write, guard: makeGuard(cwd: pictures))
report(ok, "relative inside Pictures allowed", detail)

// check(tool:arguments:).
func toolRefusal(_ tool: String, _ arguments: [String: Any]) -> String? {
    do {
        try guardian.check(tool: tool, arguments: arguments)
        return nil
    } catch let error as AgentServer.ToolError {
        return error.message
    } catch {
        return "\(error)"
    }
}
var message = toolRefusal("open_document", ["path": "~/.ssh/id_rsa"])
report(message?.hasPrefix("Refused: path points into SSH keys") == true, "tool open_document",
       message ?? "nil")
message = toolRefusal("load_files_into_stack",
                      ["paths": [pictures + "/a.png", pictures + "/b.png", "/etc/hosts"]])
report(message?.hasPrefix("Refused: paths[2] points into") == true, "tool array element",
       message ?? "nil")
message = toolRefusal("export_image", ["path": pictures + "/a.png",
                                       "settings": ["profile_path": "~/.gnupg/x.icc"]])
report(message?.hasPrefix("Refused: settings.profile_path points into GnuPG") == true,
       "tool nested argument", message ?? "nil")
message = toolRefusal("contact_sheet", ["output": ["kind": "export", "destination": "/System/x"]])
report(message?.hasPrefix("Refused: output.destination points into") == true,
       "tool nested destination", message ?? "nil")
message = toolRefusal("new_document", ["profile": "srgb"])
report(message == nil, "tool builtin profile name", message ?? "")
message = toolRefusal("new_document", ["profile": home + "/.aws/x.icc"])
report(message != nil, "tool profile path", "nil")
message = toolRefusal("set_path", ["path": "/etc/hosts"])
report(message == nil, "tool undeclared argument ignored", message ?? "")
message = toolRefusal("open_document", ["path": 3])
report(message == nil, "tool non-string ignored", message ?? "")

// catalogFailure on synthetic catalogs.
func catalog(_ tools: [[String: Any]]) -> String {
    let data = (try? JSONSerialization.data(withJSONObject: tools)) ?? Data()
    return String(decoding: data, as: UTF8.self)
}
func schema(_ properties: [String: Any]) -> [String: Any] {
    ["type": "object", "properties": properties]
}
var failure = AgentPathGuard.catalogFailure(catalog([
    ["name": "frobnicate", "inputSchema": schema(["path": ["type": "string"]])],
]))
report(failure?.contains("frobnicate.path") == true, "catalog undeclared path fails",
       failure ?? "nil")
failure = AgentPathGuard.catalogFailure(catalog([
    ["name": "frobnicate", "inputSchema": schema([
        "output": ["type": "object", "properties": ["destination": ["type": "string"]]],
        "items": ["type": "array", "items": schema(["mask_path": ["type": "string"]])],
    ])],
]))
report(failure?.contains("frobnicate.output.destination") == true
       && failure?.contains("frobnicate.items.mask_path") == true,
       "catalog undeclared nested fails", failure ?? "nil")
failure = AgentPathGuard.catalogFailure(catalog([
    ["name": "open_document", "inputSchema": schema(["path": ["type": "string"]])],
    ["name": "contact_sheet", "inputSchema": schema([
        "output": ["type": "object", "properties": [
            "destination": ["type": "string"],
            "settings": ["type": "object", "properties": ["profile_path": ["type": "string"]]],
        ]],
    ])],
]))
report(failure == nil, "catalog declared passes", failure ?? "")
failure = AgentPathGuard.catalogFailure(catalog([
    ["name": "rename_path", "inputSchema": schema(["path": ["type": "string"]])],
]))
report(failure == nil, "catalog Paths family exempt", failure ?? "")
failure = AgentPathGuard.catalogFailure(catalog([
    ["name": "open_document", "inputSchema": schema(["file": ["type": "string"]])],
]))
report(failure?.contains("open_document.path") == true, "catalog stale declaration fails",
       failure ?? "nil")

print("ENV_FOLDERS: " + AgentPathGuard.environmentFolders.joined(separator: " "))
try? fm.removeItem(atPath: scratch)
exit(failures == 0 ? 0 : 1)
