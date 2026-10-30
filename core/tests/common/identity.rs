//! The 12E-1 identity wrapper, for the format tests that predate it.
//!
//! Phase 12E-1 made the OUTERMOST `.rz` version word the ALWAYS-written
//! identity wrapper — 45 at byte depth, 46 at typed depth — and appended one
//! `LIDS` table (tag, u32 schema, u32 count, one u64 per record) after every
//! prior tail. A test whose subject is an EARLIER wrapper says so by peeling
//! this one off first: [`without_identity`] hands back byte-for-byte what the
//! writer that test was written against produced, so its own arithmetic is
//! unchanged.
//!
//! `tests/common/mod.rs` holds a TWIN of [`identity_tail`],
//! [`without_identity`] and [`drdc_word`], and both copies exist on purpose:
//! that one serves the files that use `common` and take the whole
//! helper module, while THIS one is a standalone `#[path]` module a test file
//! can pull in ALONE — which is what a format test that wants nothing else
//! from `common` needs, since `mod common;` drags in every fixture helper and
//! its dependencies. Change one, change the other.
#![allow(dead_code)]

pub const IDENTITY_BYTE: u32 = 45;
pub const IDENTITY_TYPED: u32 = 46;

/// The `LIDS` tail's size for a document of `records` records.
pub fn identity_tail(records: usize) -> usize {
    12 + 8 * records
}

/// `bytes` with the identity wrapper removed: its outermost word and its
/// `LIDS` tail cut away.
pub fn without_identity(bytes: &[u8], records: usize) -> Vec<u8> {
    let tail = identity_tail(records);
    assert!(
        bytes.len() > 8 + tail && &bytes[bytes.len() - tail..bytes.len() - tail + 4] == b"LIDS",
        "the identity table closes every file this build writes"
    );
    let mut out = bytes[..4].to_vec();
    out.extend_from_slice(&bytes[8..bytes.len() - tail]);
    out
}

/// The `.rz` version word at `index`, counting the outermost one as 0.
pub fn drdc_word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

/// How many records the file's own `LIDS` table declares — so a test that
/// does not want to count a fixture's records by hand need not.
pub fn identity_records(bytes: &[u8]) -> usize {
    let at = bytes
        .windows(4)
        .rposition(|w| w == b"LIDS")
        .expect("the identity table");
    u32::from_le_bytes(bytes[at + 8..at + 12].try_into().expect("4 bytes")) as usize
}

/// `bytes` with the identity wrapper removed, its record count read from the
/// file's own table.
pub fn prior_body(bytes: &[u8]) -> Vec<u8> {
    without_identity(bytes, identity_records(bytes))
}

/// `prior_body`, asserting first that the TYPED identity word is outermost —
/// so a file that lost the wrapper entirely fails here rather than silently
/// passing every offset below.
pub fn typed_prior_body(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(
        drdc_word(bytes, 0),
        IDENTITY_TYPED,
        "the identity wrapper is the outermost typed word"
    );
    prior_body(bytes)
}

/// The byte twin.
pub fn byte_prior_body(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(
        drdc_word(bytes, 0),
        IDENTITY_BYTE,
        "the identity wrapper is the outermost byte word"
    );
    prior_body(bytes)
}
