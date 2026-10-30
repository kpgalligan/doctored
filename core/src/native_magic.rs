//! The native document format's four-byte magic, the one home of both
//! spellings. Every writer emits [`NATIVE_MAGIC`]; every reader sniffs with
//! [`is_native_magic`], which accepts the new magic and the legacy one.
//!
//! The format kept its layout when the product was renamed from Rasterize to
//! Doctored: only the magic changed, from `RZDC` to `DRDC`. Files written
//! before the rename start `RZDC` and stay readable forever, so a reader that
//! compared against one spelling would orphan every existing document. A
//! build older than the rename refuses a `DRDC` file with its own
//! "not an RZDC document", which is the refusal the format change needs: the
//! body version cannot be raised past 89 because words 90 and up are the
//! conditional words read ahead of the body version (see `DRDC_VERSION`).

/// The magic every native file this build writes starts with.
pub(crate) const NATIVE_MAGIC: &[u8; 4] = b"DRDC";

/// The magic of native files written before the rename; read, never written.
pub(crate) const LEGACY_NATIVE_MAGIC: &[u8; 4] = b"RZDC";

/// True when `head` starts with either native magic.
pub(crate) fn is_native_magic(head: &[u8]) -> bool {
    head.starts_with(NATIVE_MAGIC) || head.starts_with(LEGACY_NATIVE_MAGIC)
}
