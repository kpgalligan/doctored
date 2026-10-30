//! Finite, presence-only disclosure of unsupported PSD layer metadata.
//! Keys and the simple range/protection/knockout fields follow Adobe's
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/ . GIMP's
//! GIMP_3_2_6/plug-ins/file-psd/psd-layer-res-load.c additionally identifies
//! the PlLd/lnkE/Txt2 aliases. Descriptor payloads are never interpreted here.
//! Google's platform/tools/base studio-2.2 pixelprobe PsdFile.java identifies
//! lmfx (stacked effects) and iOpa (fill opacity), absent from Adobe's table.
//! Bounded record framing is owned by psd_group_records; this module neither
//! allocates per block nor changes pixel admission. It carries exactly ONE
//! value into the imported document: the record's `lspf` protection word,
//! decoded through `psd_locks` into Doctored lock bits ([`FeatureMarkers::locks`]).
//!
//! A fixed marker set discloses known style/live/blending records by verified layer/folder
//! identity, distinguishing documented defaults from discarded settings without interpreting
//! descriptors.

const EFFECTS: u16 = 1;
const ADJUSTMENT: u16 = 1 << 1;
const TYPE: u16 = 1 << 2;
const SMART: u16 = 1 << 3;
const VECTOR: u16 = 1 << 4;
const RANGES: u16 = 1 << 5;
const KNOCKOUT: u16 = 1 << 6;
const PROTECTION: u16 = 1 << 7;
const BLEND_OPTIONS: u16 = 1 << 8;
const FILL_OPACITY: u16 = 1 << 9;
const IRRELEVANT_PIXELS: u16 = 1 << 10;
const FILL_OPACITY_DEFAULT: u16 = 1 << 11;
/// The record carries a `clbl` block. Tracked BESIDE `BLEND_OPTIONS` so a
/// carried `infx` can withdraw the shared sentence's claim without silencing
/// the clipping key it never interpreted (12E-3c, E9).
const CLBL: u16 = 1 << 12;
/// Photoshop's Background record had implicit locks that were masked off
/// (`psd_locks::background`).
const BACKGROUND_LOCKS: u16 = 1 << 13;

#[derive(Clone, Copy, Default)]
pub(crate) struct FeatureMarkers {
    losses: u16,
    observations: u16,
    /// The FIRST adjustment/fill key this record carried, kept verbatim so the
    /// import report can name the construct it omits.
    adjustment: Option<[u8; 4]>,
    /// The FIRST admitted `lspf` word of this record (`psd_locks::word`),
    /// Background-masked by [`FeatureMarkers::background`].
    lspf: Option<u32>,
    /// Some `lspf` body failed `psd_locks::word`, or a later word disagreed
    /// with the first: nothing is carried and PROTECTION is a loss.
    lspf_unrecognized: bool,
}

impl FeatureMarkers {
    /// Header flag bit 0 is never read as a lock (ImageMagick's
    /// `samples/layered.psd` sets it with no `lspf`); it is a PROTECTION loss
    /// unless the record's carried `lspf` already holds a transparency or
    /// pixels lock. Must run after every [`FeatureMarkers::block`] of the
    /// record, which `psd_group_records` does.
    pub fn flags(&mut self, flags: u8, raster: bool) {
        if flags & 1 != 0
            && (self.lspf.is_none()
                || self.lspf_unrecognized
                || self.locks()
                    & (crate::doc_lock::LOCK_TRANSPARENCY | crate::doc_lock::LOCK_PIXELS)
                    == 0)
        {
            self.losses |= PROTECTION;
        }
        // Adobe makes bit4 meaningful only when bit3 is set. Folder pixel
        // irrelevance is ordinary structural metadata, not an import loss.
        if raster && flags & 24 == 24 {
            self.losses |= IRRELEVANT_PIXELS;
        }
    }

    pub fn ranges(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // Each source/destination range is two black and two white values.
        // A complete set of unrestricted pairs is ordinary default metadata,
        // not evidence that Photoshop applied a restrictive Blend If effect.
        if bytes.len().is_multiple_of(8) && bytes.chunks_exact(4).all(|v| v == [0, 0, 255, 255]) {
            self.observations |= RANGES;
        } else {
            self.losses |= RANGES;
        }
    }

    pub fn block(&mut self, key: &[u8], body: &[u8]) {
        let marker = match key {
            b"lrFX" | b"lfx2" | b"lfxs" | b"lmfx" => EFFECTS,
            b"SoCo" | b"GdFl" | b"PtFl" | b"brit" | b"levl" | b"curv" | b"expA" | b"vibA"
            | b"hue " | b"hue2" | b"blnc" | b"blwh" | b"phfl" | b"mixr" | b"clrL" | b"nvrt"
            | b"post" | b"thrs" | b"grdm" | b"selc" => {
                if self.adjustment.is_none() {
                    if let Ok(key) = <[u8; 4]>::try_from(key) {
                        self.adjustment = Some(key);
                    }
                }
                ADJUSTMENT
            }
            b"tySh" | b"TySh" | b"Txt2" => TYPE,
            b"plLd" | b"PlLd" | b"SoLd" | b"SoLE" | b"lnkD" | b"lnk2" | b"lnk3" | b"lnkE" => SMART,
            b"vstk" | b"vscg" | b"vogk" => VECTOR,
            b"knko" => {
                // Adobe specifies one boolean followed by three padding bytes.
                if body.len() == 4 && body[0] == 0 {
                    self.observations |= KNOCKOUT;
                    return;
                }
                KNOCKOUT
            }
            b"lspf" => {
                let Some(word) = crate::psd_locks::word(body) else {
                    self.lspf_unrecognized = true;
                    self.losses |= PROTECTION;
                    return;
                };
                match self.lspf {
                    None => self.lspf = Some(word),
                    Some(first) if first != word => {
                        self.lspf_unrecognized = true;
                        self.losses |= PROTECTION;
                        return;
                    }
                    Some(_) => {}
                }
                // A decodable nonzero word is carried (`locks`), so it is
                // neither a loss nor an observation.
                if body == [0; 4] {
                    self.observations |= PROTECTION;
                }
                return;
            }
            b"clbl" | b"infx" => {
                // Presence does not establish a nondefault or active effect.
                // Their interaction with clipping/styles is not qualified.
                // The two keys are tracked SEPARATELY since 12E-3c, because
                // `infx` can now be carried and `clbl` never is.
                self.observations |= BLEND_OPTIONS;
                if key == b"clbl" {
                    self.observations |= CLBL;
                }
                return;
            }
            b"iOpa" => {
                // This fill-opacity key is absent from Adobe's public table.
                // Photoshop's own framing is one opacity byte and three pad
                // bytes; 255 is the 100 % default (`psd_layer_effects`).
                if body == [255, 0, 0, 0] {
                    self.observations |= FILL_OPACITY_DEFAULT;
                } else {
                    self.observations |= FILL_OPACITY;
                }
                return;
            }
            _ => return,
        };
        self.losses |= marker;
    }

    /// Folds a closing divider's markers into the file's divider summary.
    /// `lspf`/`lspf_unrecognized` are deliberately NOT merged: a divider's
    /// decodable word is attached to nothing, so it is neither carried nor a
    /// loss, and its unrecognized state already lives in `losses`.
    pub fn merge(&mut self, other: Self) {
        self.losses |= other.losses;
        self.observations |= other.observations;
        self.adjustment = self.adjustment.or(other.adjustment);
    }

    /// The Doctored lock bits this record carries (`psd_locks::locks`), 0
    /// when it has no `lspf` or an unrecognized one.
    pub fn locks(&self) -> u32 {
        if self.lspf_unrecognized {
            return 0;
        }
        self.lspf.map_or(0, crate::psd_locks::locks)
    }

    /// This record is Photoshop's Background: its implicit locks are masked
    /// off (`psd_locks::background`) and, when that changed the carried
    /// locks, the report says so. A no-op without an `lspf`.
    pub fn background(&mut self) {
        let before = self.locks();
        self.lspf = self.lspf.map(crate::psd_locks::background);
        if self.locks() != before {
            self.observations |= BACKGROUND_LOCKS;
        }
    }

    /// The record's first adjustment/fill key, or `None` when it carries no
    /// ADJUSTMENT-category block at all.
    pub fn adjustment_key(&self) -> Option<[u8; 4]> {
        self.adjustment
    }

    /// The record's adjustment body is carried as a Doctored adjustment
    /// layer (`psd_adjustment`), so its ADJUSTMENT metadata is no longer
    /// discarded and must not be reported as a loss.
    pub fn carry_adjustment(&mut self) {
        self.losses &= !ADJUSTMENT;
    }

    /// The record's `PtFl`, `SoCo` or `GdFl` is carried as a Doctored
    /// pattern, solid color or gradient fill layer (`psd_fill_layer`):
    /// its ADJUSTMENT
    /// metadata is carried, and pixels it
    /// declares irrelevant (bit 4) are replaced by the re-rendered fill on
    /// purpose, so neither is a loss.
    pub fn carry_fill(&mut self) {
        self.losses &= !(ADJUSTMENT | IRRELEVANT_PIXELS);
    }

    /// The record's effects body, fill opacity and/or blending ranges are
    /// carried as a Doctored layer style (`psd_layer_effects`), so exactly
    /// those markers are no longer discarded and must not be reported.
    pub fn carry_style(&mut self, effects: bool, fill: bool, ranges: bool) {
        if effects {
            self.losses &= !EFFECTS;
        }
        if fill {
            self.observations &= !FILL_OPACITY;
        }
        if ranges {
            self.losses &= !RANGES;
            self.observations &= !RANGES;
        }
    }

    /// The record's `knko` and/or `infx` are carried as part of its Doctored
    /// layer style (`psd_layer_effects`), so exactly those markers are no
    /// longer discarded and must not be reported. A carried `infx` withdraws
    /// the SHARED blending-options sentence's "not interpreted or retained"
    /// claim; a `clbl` on that same record then discloses itself alone.
    pub fn carry_knockout(&mut self, knockout: bool, interior: bool) {
        if knockout {
            self.losses &= !KNOCKOUT;
            self.observations &= !KNOCKOUT;
        }
        if interior {
            self.observations &= !BLEND_OPTIONS;
        }
    }

    pub fn report(self, target: &str, losses: &mut Vec<String>, observations: &mut Vec<String>) {
        for (marker, text) in [
            (EFFECTS, "has layer-style metadata; layered import discards these records without interpreting whether effects are active"),
            (ADJUSTMENT, "has adjustment/fill metadata; layered import discards these records without interpreting their settings"),
            (TYPE, "has text/type metadata; layered import discards these records and does not retain editable text"),
            (SMART, "has placed/smart-object or linked-layer metadata; layered import discards these records and does not retain editable object contents or links"),
            (VECTOR, "has vector stroke/origination metadata; layered import discards these records and does not retain editable vector settings"),
            (RANGES, "has nondefault or unrecognized Blend If range data; layered import discards these settings without applying them"),
            (KNOCKOUT, "has nonzero or unrecognized knockout metadata; layered import discards these settings without applying them"),
            (PROTECTION, "has nonzero or unrecognized layer-protection metadata; layered import discards these settings without applying them as locks"),
            (IRRELEVANT_PIXELS, "declares its raster pixels irrelevant to document appearance; layered import retains the available pixels without interpreting the alternative appearance semantics"),
        ] {
            if self.losses & marker != 0 {
                losses.push(format!("{target} {text}."));
            }
        }
        for (marker, text) in [
            (RANGES, "has unrestricted default Blend If ranges; no restrictive range settings were found in that record"),
            (KNOCKOUT, "has a disabled knockout flag"),
            (PROTECTION, "has zero layer-protection flags"),
            (BLEND_OPTIONS, "has additional clipping/interior blending metadata; its payload is not interpreted or retained, and its presence alone does not establish an active effect"),
            (FILL_OPACITY, "has fill-opacity metadata; its payload is not interpreted or retained, and its presence alone does not establish nondefault opacity"),
            (FILL_OPACITY_DEFAULT, "has fill-opacity metadata at its 100 % default; its presence alone does not establish nondefault opacity"),
            (BACKGROUND_LOCKS, "is Photoshop's Background layer; its implicit transparency and position locks are not carried — Doctored has no Background layer kind"),
        ] {
            // Conflicting duplicate simple fields must not produce a benign
            // default observation alongside a discarded-setting disclosure.
            if self.observations & marker != 0 && self.losses & marker == 0 {
                observations.push(format!("{target} {text}."));
            }
        }
        // E9: a carried `infx` withdrew the shared sentence above, so a
        // `clbl` on the same record states its own — and only its own — case.
        // While the shared sentence stands it already covers this key, and
        // the two must never claim the same block twice.
        if self.observations & CLBL != 0 && self.observations & BLEND_OPTIONS == 0 {
            observations.push(format!("{target} has additional clipping blending metadata (clbl); its payload is not interpreted or retained, and its presence alone does not establish an active effect."));
        }
    }
}
