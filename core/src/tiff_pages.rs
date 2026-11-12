//! Multipage TIFF: which images in a TIFF's directory chain are pages,
//! which pages open as layers, the sentences that say what was left out, and
//! the patch that makes ONE page "the file" for every one-page reader.
//!
//! # The mechanism
//!
//! Every offset in a TIFF is absolute, so a file whose header points at IFD
//! *n* and whose IFD *n* links to nothing is a valid one-page TIFF of page
//! *n*. [`with_page`] writes exactly those two fields into the buffer the open
//! already owns, runs the caller's ordinary one-page open over it, and writes
//! the original bytes back, error paths included. So the flat decoder, the
//! 16-bit codec, the float seam, the metadata scan, orientation, the profile
//! and the spot reader each read one page under their own unchanged rules,
//! and no decoder gains a page parameter. (`tiff_directory::Overlay` patches
//! a STREAM the `tiff` decoder reads; every reader here takes a byte slice,
//! so the owned buffer itself is patched.)
//!
//! The byte open, the typed open (RGB, float and CMYK) and the 8/16-bit
//! Grayscale and Lab Color opens take pages this way (`doc_tiff_pages`).
//! The Indexed Color and Bitmap opens read the first image alone, as each
//! of those documents holds one layer.
//!
//! # What is a page
//!
//! An image in the main chain (SubIFDs are never walked) that has strip or
//! tile offsets, whose NewSubfileType (254) has neither bit 0 (a
//! reduced-resolution copy) nor bit 2 (a transparency mask) set, and whose
//! SubfileType (255) is not 2 (reduced resolution). One more image is not a
//! page: the uncompressed Exif thumbnail, the second and last image of a file
//! whose first page carries an Exif IFD (34665), with no NewSubfileType of
//! its own and smaller than that page both ways. Images that are not pages
//! are neither opened nor mentioned. When no image qualifies, the first
//! image is page 1, which is how the file opened before.
//!
//! # Which pages open
//!
//! Page 1 always does, under the open's own rules, and sets the canvas,
//! profile, metadata, resolution and depth. A later page opens as a layer
//! when its header matches page 1's in everything a layer cannot carry for
//! itself: the bit depth and sample format of its samples, its photometric
//! interpretation, and its ICC profile (both absent, or byte-equal), and it
//! carries no extra channel besides alpha. Each other page is named with its
//! reason ([`Pages::notes`]). The decision reads headers only, so the typed
//! open's import warnings and the byte open's import report say the same.
//! At most [`MAX_PAGES`] pages, holding at most
//! `drdc::MAX_DRDC_TOTAL_LAYER_PIXELS` pixels together, open; the rest are
//! named. A walk that meets a loop, an unreadable directory or more than
//! [`MAX_IFDS`] images stops there and says so; the pages before it open.

use std::collections::HashSet;
use std::ops::Range;

use crate::tiff_view::{Entry, Tiff, TAG_EXIF_IFD, TAG_IMAGE_LENGTH, TAG_IMAGE_WIDTH};

/// Images walked at most. A real multipage file has tens; this bounds the
/// header work a crafted chain can ask for.
pub(crate) const MAX_IFDS: usize = 4096;

/// Pages opened at most: one layer each, the native layer limit
/// (`drdc::MAX_DRDC_LAYERS`, the typed `precision_render::MAX_RECORDS`).
pub(crate) const MAX_PAGES: usize = 1024;

const TAG_NEW_SUBFILE_TYPE: u16 = 254;
const TAG_SUBFILE_TYPE: u16 = 255;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_EXTRA_SAMPLES: u16 = 338;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_ICC_PROFILE: u16 = 34675;
/// BigTIFF's 8-byte unsigned type, which a BigTIFF may give ImageWidth and
/// ImageLength.
const TYPE_LONG8: u16 = 16;

/// One image of the chain, read from its directory alone.
struct Image {
    /// Where the directory starts, and where its next-image link lives.
    offset: usize,
    link: usize,
    width: u32,
    height: u32,
    bits: Vec<u16>,
    format: u16,
    photometric: Option<u16>,
    /// An ExtraSamples value of 0: a channel that is neither alpha.
    extra_channel: bool,
    icc: Option<Range<usize>>,
    /// Strips or tiles present, and no reduced-resolution or mask bit.
    page: bool,
    /// NewSubfileType present at all (the Exif thumbnail rule).
    subfile_tag: bool,
    exif: bool,
}

/// The pages of one TIFF and what opens of them (module doc).
pub(crate) struct Pages {
    big: bool,
    images: Vec<Image>,
    /// Indexes into `images`: page *n* is `pages[n - 1]`.
    pages: Vec<usize>,
    /// Page numbers that open as layers, page 1 first.
    admitted: Vec<usize>,
    /// (page number, reason) for each page that does not.
    skipped: Vec<(usize, String)>,
    /// The page the walk stopped after, and why, when it stopped early.
    stopped: Option<(usize, &'static str)>,
}

/// The chain of `bytes` when there is anything to do with it: `None` for a
/// file that is not a TIFF, whose first directory is unreadable, or that
/// holds exactly one image and ends cleanly — each of which opens exactly
/// as it did before this module.
pub(crate) fn read(bytes: &[u8]) -> Option<Pages> {
    let tiff = Tiff::file(bytes)?;
    let mut images = Vec::new();
    let mut seen = HashSet::new();
    let mut at = tiff.ifd0;
    let mut stopped = None;
    loop {
        if images.len() == MAX_IFDS {
            stopped = Some("it lists more images than are read");
            break;
        }
        if !seen.insert(at) {
            stopped = Some("its list of images loops back on itself");
            break;
        }
        let Some((entries, link)) = tiff.ifd(at) else {
            if images.is_empty() {
                return None;
            }
            stopped = Some("its list of images ends in a damaged entry");
            break;
        };
        images.push(image(&tiff, at, &entries, link));
        match tiff.offset_at(link) {
            Some(0) => break,
            Some(next) if next >= 8 && next < bytes.len() => at = next,
            _ => {
                stopped = Some("its list of images ends in a damaged entry");
                break;
            }
        }
    }
    if images.len() == 1 && stopped.is_none() {
        return None;
    }
    // The Exif thumbnail (module doc).
    if images.len() == 2 && images[0].page && images[0].exif && !images[1].subfile_tag {
        let (first, second) = (&images[0], &images[1]);
        if second.width < first.width && second.height < first.height {
            images[1].page = false;
        }
    }
    let mut pages: Vec<usize> = (0..images.len()).filter(|&i| images[i].page).collect();
    if pages.is_empty() {
        pages.push(0);
    }
    let stopped = stopped.map(|why| (pages.len(), why));
    let mut plan = Pages {
        big: tiff.big,
        images,
        pages,
        admitted: vec![1],
        skipped: Vec::new(),
        stopped,
    };
    plan.decide(bytes);
    Some(plan)
}

/// One directory's facts. A tag that is absent takes the TIFF default
/// (one 1-bit sample, unsigned integers).
fn image(tiff: &Tiff<'_>, offset: usize, entries: &[Entry], link: usize) -> Image {
    let find = |tag: u16| entries.iter().find(|e| e.tag == tag);
    let scalar = |tag: u16| find(tag).and_then(|e| tiff.scalar(e));
    // `Tiff::scalar` reads SHORT and LONG; a BigTIFF's size may be LONG8,
    // which would otherwise read as 0 and escape the pixel cap and the
    // size note.
    let size = |tag: u16| {
        find(tag).and_then(|e| match e.kind {
            TYPE_LONG8 if tiff.big => u32::try_from(tiff.u64_at(e.field)?).ok(),
            _ => tiff.scalar(e),
        })
    };
    let shorts = |tag: u16| -> Vec<u16> {
        let Some(e) = find(tag).filter(|e| e.kind == crate::tiff_view::TYPE_SHORT) else {
            return Vec::new();
        };
        let Some(range) = tiff.data_range(e) else {
            return Vec::new();
        };
        let count = usize::try_from(e.count).unwrap_or(0).min(range.len() / 2);
        (0..count)
            .filter_map(|i| tiff.u16_at(range.start + 2 * i))
            .collect()
    };
    let new_subfile = scalar(TAG_NEW_SUBFILE_TYPE).unwrap_or(0);
    let data = find(TAG_STRIP_OFFSETS).is_some() || find(TAG_TILE_OFFSETS).is_some();
    let bits = match shorts(TAG_BITS_PER_SAMPLE) {
        bits if bits.is_empty() => vec![1],
        bits => bits,
    };
    let icc = find(TAG_ICC_PROFILE).and_then(|e| tiff.data_range(e));
    Image {
        offset,
        link,
        width: size(TAG_IMAGE_WIDTH).unwrap_or(0),
        height: size(TAG_IMAGE_LENGTH).unwrap_or(0),
        bits,
        format: shorts(TAG_SAMPLE_FORMAT).first().copied().unwrap_or(1),
        photometric: scalar(TAG_PHOTOMETRIC).and_then(|v| u16::try_from(v).ok()),
        extra_channel: shorts(TAG_EXTRA_SAMPLES).contains(&0),
        icc,
        page: data && new_subfile & 0b101 == 0 && scalar(TAG_SUBFILE_TYPE) != Some(2),
        subfile_tag: find(TAG_NEW_SUBFILE_TYPE).is_some(),
        exif: find(TAG_EXIF_IFD).is_some(),
    }
}

impl Pages {
    /// Every page's verdict against page 1 (module doc), then the layer and
    /// pixel capacity over the pages that passed, in page order.
    fn decide(&mut self, bytes: &[u8]) {
        let first = &self.images[self.pages[0]];
        let mut pixels = u64::from(first.width) * u64::from(first.height);
        let mut admitted = vec![1];
        let mut skipped = Vec::new();
        for (n, &index) in self.pages.iter().enumerate().skip(1) {
            let number = n + 1;
            let page = &self.images[index];
            if let Some(reason) = mismatch(first, page, bytes) {
                skipped.push((number, reason));
                continue;
            }
            let size = u64::from(page.width) * u64::from(page.height);
            if admitted.len() == MAX_PAGES {
                skipped.push((
                    number,
                    format!("a document holds at most {MAX_PAGES} layers"),
                ));
                continue;
            }
            if pixels.saturating_add(size) > crate::drdc::MAX_DRDC_TOTAL_LAYER_PIXELS {
                skipped.push((
                    number,
                    "the document's layers would hold more pixels than a document can".into(),
                ));
                continue;
            }
            pixels += size;
            admitted.push(number);
        }
        self.admitted = admitted;
        self.skipped = skipped;
    }

    /// How many pages the file has, by the module's rule.
    pub(crate) fn count(&self) -> usize {
        self.pages.len()
    }

    /// The page numbers that open as layers, page 1 first.
    pub(crate) fn admitted(&self) -> &[usize] {
        &self.admitted
    }

    /// Whether page 1 is the file's first image. The Grayscale and Lab
    /// Color opens, and the app's source tests that choose them, read the
    /// first image, so they take pages only when it is page 1.
    pub(crate) fn first_image_is_page_one(&self) -> bool {
        self.pages[0] == 0
    }

    /// Page `number`'s size.
    pub(crate) fn size(&self, number: usize) -> (u32, u32) {
        let image = &self.images[self.pages[number - 1]];
        (image.width, image.height)
    }

    /// The sentences an open that takes every admitted page reports: the
    /// skipped pages, grouped by reason; the pages whose size differs from
    /// the canvas; a walk that stopped early; and, when more than one page
    /// opened, what the layers are.
    pub(crate) fn notes(&self) -> Vec<String> {
        let mut notes = self.skipped_notes();
        let canvas = self.size(1);
        let other: Vec<usize> = self.admitted[1..]
            .iter()
            .copied()
            .filter(|&n| self.size(n) != canvas)
            .collect();
        if !other.is_empty() {
            let (w, h) = canvas;
            notes.push(format!(
                "{} {} a different size from page 1 ({w} × {h}, the canvas) and {} at the canvas's top-left corner.",
                pages_phrase(&other),
                if other.len() == 1 { "is" } else { "are" },
                if other.len() == 1 { "sits" } else { "sit" },
            ));
        }
        if self.admitted.len() > 1 {
            let opened = if self.admitted.len() == self.count() {
                format!("The file's {} pages", self.count())
            } else {
                format!(
                    "{} of the file's {} pages",
                    self.admitted.len(),
                    self.count()
                )
            };
            notes.push(format!(
                "{opened} opened as layers named by page number, page 1 at the bottom; only Page 1 is visible."
            ));
        }
        notes
    }

    fn skipped_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut reasons: Vec<&String> = Vec::new();
        for (_, reason) in &self.skipped {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        for reason in reasons {
            let numbers: Vec<usize> = self
                .skipped
                .iter()
                .filter(|(_, r)| r == reason)
                .map(|(n, _)| *n)
                .collect();
            notes.push(format!(
                "{} {} not opened: {reason}.",
                pages_phrase(&numbers),
                if numbers.len() == 1 { "was" } else { "were" },
            ));
        }
        if let Some(stop) = self.stop_note() {
            notes.push(stop);
        }
        notes
    }

    fn stop_note(&self) -> Option<String> {
        let (after, why) = self.stopped?;
        Some(format!(
            "The file is damaged after page {after}: {why}, so no image after it was read."
        ))
    }

    /// The two fields [`with_page`] writes for page `number`, little-endian
    /// (`with_page` reverses them for an `MM` file): the header's
    /// first-image offset (4 bytes at 4, or 8 at 8 in a BigTIFF) and that
    /// page's next-image link, zeroed.
    fn patches(&self, number: usize) -> [(usize, Vec<u8>); 2] {
        let image = &self.images[self.pages[number - 1]];
        let (header, width) = if self.big { (8, 8) } else { (4, 4) };
        let first = (image.offset as u64).to_le_bytes()[..width].to_vec();
        [(header, first), (image.link, vec![0; width])]
    }
}

/// `first` against a later `page`: the reason it cannot be a layer of
/// page 1's document, or `None`.
fn mismatch(first: &Image, page: &Image, bytes: &[u8]) -> Option<String> {
    let depth = |image: &Image| {
        let width = image.bits[0];
        image.bits.iter().all(|&b| b == width).then_some(width)
    };
    match (depth(page), depth(first)) {
        (Some(a), Some(b)) if a == b => {}
        (Some(a), Some(b)) => {
            return Some(format!("its samples are {a}-bit and page 1's are {b}-bit"));
        }
        _ => return Some("its samples are not all one bit depth".into()),
    }
    if page.format != first.format {
        return Some(format!(
            "its samples are {} and page 1's are {}",
            format_name(page.format),
            format_name(first.format)
        ));
    }
    if page.photometric != first.photometric {
        return Some(format!(
            "it is {} and page 1 is {}",
            photometric_name(page.photometric),
            photometric_name(first.photometric)
        ));
    }
    let profile = |image: &Image| image.icc.clone().and_then(|r| bytes.get(r));
    if profile(page) != profile(first) {
        return Some(match (profile(page), profile(first)) {
            (None, _) => "it has no colour profile and page 1 has one".into(),
            (_, None) => "it has a colour profile and page 1 has none".into(),
            _ => "its colour profile differs from page 1's".into(),
        });
    }
    if page.extra_channel {
        return Some("it carries an extra channel besides alpha".into());
    }
    None
}

fn format_name(format: u16) -> String {
    match format {
        1 => "unsigned integers".into(),
        2 => "signed integers".into(),
        3 => "floating point".into(),
        other => format!("of sample format {other}"),
    }
}

fn photometric_name(photometric: Option<u16>) -> String {
    match photometric {
        Some(0) => "grayscale with white at zero".into(),
        Some(1) => "grayscale".into(),
        Some(2) => "RGB".into(),
        Some(3) => "indexed colour".into(),
        Some(4) => "a transparency mask".into(),
        Some(5) => "CMYK".into(),
        Some(6) => "YCbCr".into(),
        Some(8) => "CIE Lab".into(),
        Some(9) => "ICC Lab".into(),
        Some(10) => "ITU Lab".into(),
        Some(other) => format!("of photometric interpretation {other}"),
        None => "of no stated photometric interpretation".into(),
    }
}

/// "Page 3", "Pages 2 and 5", "Pages 2–4, 7 and 9".
fn pages_phrase(numbers: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &n in numbers {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == n => *end = n,
            _ => runs.push((n, n)),
        }
    }
    let parts: Vec<String> = runs
        .iter()
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}–{b}")
            }
        })
        .collect();
    let single = numbers.len() == 1;
    let list = match parts.as_slice() {
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    format!("{} {list}", if single { "Page" } else { "Pages" })
}

/// Runs `open` over `bytes` with page `number` made the file's only page
/// (module doc), then restores the bytes it changed, whatever `open`
/// returned.
pub(crate) fn with_page<R>(
    bytes: &mut [u8],
    pages: &Pages,
    number: usize,
    open: impl FnOnce(&[u8]) -> R,
) -> R {
    let big_endian = bytes.starts_with(b"MM");
    let mut saved = Vec::new();
    for (at, mut value) in pages.patches(number) {
        if big_endian {
            value.reverse();
        }
        let end = at + value.len();
        saved.push((at, bytes[at..end].to_vec()));
        bytes[at..end].copy_from_slice(&value);
    }
    let result = open(bytes);
    for (at, value) in saved.into_iter().rev() {
        bytes[at..at + value.len()].copy_from_slice(&value);
    }
    result
}
