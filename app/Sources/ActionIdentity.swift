import Foundation

// Identity records both id and name (`ActionIdentity.session`, a UUID per launch): a `{"$layer":
// {name, id, session}}` symbol honours its id alone within the launch that minted it, in another
// session only when the recorded NAME also matches (layer ids are persisted in the `.rz` `LIDS`
// table since 12E-1; channel ids stay session-only), and otherwise falls back to the NAME
// (`ActionPlayer.resolve`) — which is why a batch of different files still works by construction.

/// Stable layer and channel identity as an Action records and resolves it:
/// the two reference values, and the one-per-launch session token that says
/// whether a stored id can mean anything at all.
///
/// **The problem.** A recorded step has to name a layer. An INDEX is
/// renumbered by every insert, delete, group, ungroup, merge, reorder and
/// duplicate, so it points at a different entry seconds later. A NAME is not
/// unique — two layers may be called "Sky" — and a rename breaks it. Phase 12A
/// recorded names alone, which meant that renaming the layer an action was
/// built around silently aimed every step at whatever else matched, or at
/// nothing.
///
/// **The design, in one sentence: record BOTH, resolve by id when it can mean
/// something and by name when it cannot.** [`ActionIdentity.session`] is
/// stamped into the symbol at record time and compared at replay, and it is
/// what tells "this id names the layer it was recorded against" from "this id
/// is a number that happens to exist".
///
/// **LAYER ids are PERSISTED since 12E-1** (`identity_native`'s `LIDS`
/// table): a `.rz` carries every layer's id and a reopen hands the same ids
/// back, so `$layer.id` survives save → quit → reopen. CHANNEL ids are still
/// session-only, which is the guide precedent (`doc_guide.rs`: "deliberately
/// NOT persisted"), so `$channel.id` keeps the 12A rule exactly.
///
/// **What "can mean something" is, for a layer, in three steps**
/// (`ActionPlayer.resolve`):
///
/// 1. SAME session — the id alone. Within one launch ids are unique across
///    every open document, so nothing can be confused with anything.
/// 2. ANOTHER session — the id, but only when the entry carrying it ALSO
///    carries the recorded NAME. Uniqueness is per DOCUMENT, not per file
///    set: a batch's second file may legitimately hold the same id on a
///    completely different layer, and an uncorroborated id would then aim the
///    step at the wrong picture. Two agreeing facts are the bar.
/// 3. Otherwise the NAME, which is what 12A always did.
///
/// **A batch of DIFFERENT files is still the name path by construction**, for
/// the reason step 2 states; re-running an action over the same reopened
/// document is now the id path, which is the case the persistence was for.
enum ActionIdentity {
    /// A token minted once per LAUNCH. An Action symbol carries the session it
    /// was recorded in, and its `id` is honoured only when that token matches
    /// this one — so an id from a previous run of the app, or from another
    /// machine, is ignored rather than gambled on.
    ///
    /// A UUID rather than a counter or a launch time: two instances of the app
    /// can run at once (verification launches its own beside the user's), and
    /// a token they could both produce would let one instance's ids be
    /// honoured against the other's documents.
    static let session: String = UUID().uuidString
}

/// A layer as a recorded step names it: the display name it had, and the
/// stable id it had. Neither half is authoritative on its own — see
/// `ActionIdentity`.
struct LayerRef {
    let name: String
    /// `dr_doc_layer_id`'s value. 0 only for a reference built where the core
    /// could not be asked, which resolves by name like any other.
    let id: UInt64
}

/// A channel as a recorded step names it, the same pair for the same reason.
/// Channels have carried a stable id since the channels phase; this is only
/// the spelling an Action symbol takes.
struct ChannelRef {
    let name: String
    let id: UInt64
}
