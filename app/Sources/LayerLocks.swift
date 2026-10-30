import Foundation

/// A layer's lock flags, as an `OptionSet` over the core's `DrLockFlags`
/// bits (`doc_lock.rs` is where the meaning lives; this side only names and
/// displays them).
///
/// What each one does, restated here because the refusal sentences below
/// have to be true:
///
/// - **transparency** — the layer's ALPHA CHANNEL is frozen. Every pixel
///   keeps its alpha exactly: a transparent pixel stays empty, and on a
///   semi-transparent one paint mixes in at its own opacity × coverage,
///   not boosted by the kept alpha. So an eraser
///   cannot punch a hole and paint cannot spill outside the existing shape.
///   It does NOT block a transform: a
///   transform resamples the whole buffer including its alpha, so a frozen
///   alpha has no meaning there.
/// - **pixels** — every pixel edit is refused. Mask edits are not: a
///   pixel-locked layer's mask can be painted.
/// - **position** — moves, offsets, transforms and perspective are refused.
///   Moving a GROUP checks every descendant's position bit too, because
///   moving the group moves them.
/// - **all three together** — additionally freezes the MASK. That is the one
///   extra meaning "Lock All" carries beyond the union of the three.
///
/// A GROUP's locks cover its contents: an entry's effective locks are
/// its own OR every enclosing group's, and a refusal names the group whose
/// lock did the blocking.
///
/// Nothing here blocks delete, duplicate, reorder, group, rename, opacity,
/// blend mode, visibility or styles: blocking them would make a locked
/// layer unmanageable.
struct LockFlags: OptionSet, Equatable {
    let rawValue: UInt32

    init(rawValue: UInt32) {
        // Bits 3..31 are reserved; mask them off wherever a value enters, the
        // same way the core's setter and its `.rz` reader do.
        self.rawValue = rawValue & LockFlags.allBits
    }

    static let transparency = LockFlags(rawValue: DR_LOCK_TRANSPARENCY.rawValue)
    static let pixels = LockFlags(rawValue: DR_LOCK_PIXELS.rawValue)
    static let position = LockFlags(rawValue: DR_LOCK_POSITION.rawValue)

    /// "Lock All" — the three bits together rather than a fourth bit of its
    /// own. Bits 0..2 share PSD `lspf`'s positions, but `lspf` spells Lock All
    /// as bit 31 (the core's `psd_locks`).
    static let all: LockFlags = [.transparency, .pixels, .position]

    private static let allBits = DR_LOCK_ALL.rawValue

    /// The agent's vocabulary, in the order `set_layer_lock` publishes it.
    /// One table, so the tool's schema, its reply and the menu can never
    /// disagree about a name.
    static let named: [(name: String, flag: LockFlags)] = [
        ("transparency", .transparency), ("pixels", .pixels), ("position", .position),
    ]

    /// The set's names, ascending by bit — `["transparency", "pixels"]`.
    var names: [String] { Self.named.filter { contains($0.flag) }.map { $0.name } }

    /// Menu- and alert-facing wording for ONE flag.
    var displayName: String {
        if self == .all { return "Lock All" }
        switch self {
        case .transparency: return "Transparency"
        case .pixels: return "Pixels"
        case .position: return "Position"
        default: return names.joined(separator: ", ")
        }
    }

    /// The ONE flag `refusal` names out of a blocking set: `.all`, else
    /// `.pixels`, else `.position`, else `.transparency`. It is also what
    /// `RasterDocument.lockRefusal` asks the core's owner query about, so the
    /// entry a sentence blames is the one holding the flag it names.
    var reported: LockFlags {
        if self == .all { return .all }
        if contains(.pixels) { return .pixels }
        if contains(.position) { return .position }
        if contains(.transparency) { return .transparency }
        return self
    }

    /// Why an edit was refused, naming the lock that stopped it — the whole
    /// point of asking the core which bits blocked rather than just seeing a
    /// NULL. `blocking` is what `lockBlockingFlags` answered. `group` names
    /// the enclosing group whose own lock did the blocking (a group's
    /// locks cover its contents); nil when the layer's own lock did.
    static func refusal(layerName: String, blocking: LockFlags, group: String? = nil) -> String {
        if let group {
            let inside = "Layer “\(layerName)” is inside group “\(group)”, which"
            switch blocking.reported {
            case .all: return "\(inside) is locked (Lock All)."
            case .pixels: return "\(inside) has its pixels locked."
            case .position: return "\(inside) has its position locked."
            case .transparency: return "\(inside) has its transparency locked."
            default: return "\(inside) is locked."
            }
        }
        if blocking == .all {
            return "Layer “\(layerName)” is locked (Lock All)."
        }
        if blocking.contains(.pixels) {
            return "Layer “\(layerName)” has its pixels locked."
        }
        if blocking.contains(.position) {
            return "Layer “\(layerName)” has its position locked."
        }
        if blocking.contains(.transparency) {
            return "Layer “\(layerName)” has its transparency locked."
        }
        return "Layer “\(layerName)” is locked."
    }
}

/// One lock an enclosing group imposes on an entry whose own locks lack it,
/// derived from `lockOwner` — the Layers panel's dimmed badge and
/// `get_document`'s `inherited_locks` / `lock_owner` read it.
struct InheritedLock: Equatable {
    /// Exactly one of `.transparency` / `.pixels` / `.position`.
    let flag: LockFlags
    /// The nearest enclosing group whose OWN bits hold `flag`.
    let owner: Int
    /// The owner's name, for the panel's tooltip.
    let group: String
}

extension RasterDocument {
    /// The locks entry `idx` inherits from its enclosing groups, in
    /// `LockFlags.named` order — empty at the top level and for every flag
    /// the entry already holds itself.
    func inheritedLocks(_ idx: Int) -> [InheritedLock] {
        guard layerDepth(idx) != 0 else { return [] }
        let own = lockFlags(idx)
        var inherited: [InheritedLock] = []
        for (_, flag) in LockFlags.named where !own.contains(flag) {
            guard let owner = lockOwner(idx, locks: flag.rawValue), owner != idx else { continue }
            inherited.append(InheritedLock(
                flag: flag, owner: owner, group: layerInfo(owner)?.name ?? "this group"))
        }
        return inherited
    }

    /// Entry `idx`'s locks as an `OptionSet`.
    func lockFlags(_ idx: Int) -> LockFlags { LockFlags(rawValue: layerLocks(idx)) }

    /// Which of entry `idx`'s locks would block an edit of `kind` — empty
    /// when the edit is allowed. The core refuses regardless of whether
    /// anyone asks; this exists so a refusal can NAME the lock.
    func lockBlockingFlags(_ idx: Int, kind: DrEditKind) -> LockFlags {
        LockFlags(rawValue: lockBlocking(idx, kind: kind))
    }

    /// The refusal sentence for an edit of `kind` on entry `idx`, and the
    /// entry that owns the blocking lock — `idx` itself, or the enclosing
    /// group whose own bit did it. nil when nothing blocks. The owner
    /// is `lockOwner(idx, reported) ?? idx`: a group's move blocked only by a
    /// descendant's position bit blames the group being moved.
    func lockRefusal(_ idx: Int, kind: DrEditKind) -> (message: String, owner: Int)? {
        let blocking = lockBlockingFlags(idx, kind: kind)
        guard !blocking.isEmpty else { return nil }
        let owner = lockOwner(idx, locks: blocking.reported.rawValue) ?? idx
        let name = layerInfo(idx)?.name ?? "this layer"
        let group = owner == idx ? nil : layerInfo(owner)?.name ?? "this group"
        return (LockFlags.refusal(layerName: name, blocking: blocking, group: group), owner)
    }

    /// The entry whose own Transparency lock freezes entry `idx`'s alpha —
    /// `idx` itself or an enclosing group — or nil when its alpha is
    /// not frozen.
    func transparencyLockOwner(_ idx: Int) -> Int? {
        lockOwner(idx, locks: LockFlags.transparency.rawValue)
    }

    /// Sets entry `idx`'s locks. Pure, like every other core op: nil when
    /// the entry already carries exactly these flags.
    func withLockFlags(_ idx: Int, _ locks: LockFlags) throws -> RasterDocument? {
        try withLayerLocks(idx, locks.rawValue)
    }
}
