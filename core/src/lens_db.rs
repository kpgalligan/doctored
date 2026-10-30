//! The bundled Lensfun lens database (v1-lensfun): camera and lens matching
//! from an EXIF identity, and the resolution of one lens's calibration rows to
//! the `lens_model::LensProfile` JSON at a focal length, f-number and
//! distance. No pixel work: `sampling_lens` applies the profile.
//!
//! The data is `core/data/lensfun/lensfun-db.json`, written by
//! `scripts/fetch_lensfun.py` from Lensfun v0.3.4's `data/db/*.xml` (CC BY-SA
//! 3.0, the Lensfun project and its contributors; original database: Tom
//! Niemann's PTLens database; the JSON is an adaptation under the same
//! licence), embedded with `include_bytes!` and parsed once.
//!
//! Matching (designs/v1-lensfun.md, "Matching"):
//! - Names are normalised (trimmed, spaces collapsed, lowercased, `N.0`
//!   written `N`) and split into Lensfun's documented words: runs of digits,
//!   of punctuation and of other characters. Single punctuation characters
//!   and the word `f` are dropped; words compare as sets.
//! - The camera is the case-insensitive exact maker and model, the first
//!   entry without a variant. Its crop factor is the image's; without a
//!   camera, `focal_length_35mm / focal_length`; without either, no profile.
//! - A lens is a candidate when the EXIF lens words are a subset of the
//!   lens's maker and model words, or the lens's model words (less its
//!   maker's) are a subset of the EXIF words; the EXIF focal length is within
//!   its range ± 0.5 mm; and, with a known camera, one of its mounts is the
//!   camera's or one the camera's mount lists as compatible (one level). A
//!   fixed-lens camera (a mount named in lower case) with exactly one lens on
//!   that mount matches without a lens name.
//! - Candidates rank by the Jaccard index of the two word sets, then the
//!   nearer calibration crop (`|ln(Cc/Ci)|`), then database order; `auto`
//!   takes the first. With the target plane's `dimensions` and the EXIF pixel
//!   dimensions both known, `auto` gives no profile when their long/short
//!   aspects differ by more than 1% (the picture is not the camera frame).
//!
//! Interpolation between calibration rows is `lens_interp`'s (its module doc
//! states the rules). No f-number: no vignetting. No distance: 1000 m.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::lens_interp::{interpolate_rows, pa_at, Row};

static DATA: &[u8] = include_bytes!("../data/lensfun/lensfun-db.json");

/// The subject distance assumed when the EXIF states none: far focus.
const DEFAULT_DISTANCE_M: f64 = 1000.0;
/// How far, in mm, the EXIF focal length may sit outside a lens's range:
/// EXIF rounds focal lengths, and a zoom's ends are nominal.
const FOCAL_SLACK_MM: f64 = 0.5;
/// The largest `|ln(a_plane / a_exif)|` auto accepts: 1%, which a uniformly
/// downscaled frame's rounding never reaches and a crop to a different
/// aspect does.
const FRAME_TOLERANCE: f64 = 0.01;

/// A lens identity read from EXIF (or given by the host).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LensIdentity {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub focal_length: Option<f64>,
    pub f_number: Option<f64>,
    pub subject_distance: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub pixel_width: Option<u32>,
    pub pixel_height: Option<u32>,
}

const STRING_FIELDS: [&str; 4] = ["make", "model", "lens_make", "lens_model"];
const NUMBER_FIELDS: [&str; 6] = [
    "focal_length",
    "f_number",
    "subject_distance",
    "focal_length_35mm",
    "pixel_width",
    "pixel_height",
];

impl LensIdentity {
    /// The `dr_lens_exif_identity` JSON: every field, `null` when unknown.
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "make": self.make, "model": self.model,
            "lens_make": self.lens_make, "lens_model": self.lens_model,
            "focal_length": self.focal_length, "f_number": self.f_number,
            "subject_distance": self.subject_distance,
            "focal_length_35mm": self.focal_length_35mm,
            "pixel_width": self.pixel_width, "pixel_height": self.pixel_height,
        })
    }

    /// The same object back, strict: an unknown key or a value of the wrong
    /// type is refused; an absent field is unknown. A nonpositive or
    /// nonfinite number is treated as unknown (EXIF writes 0 for "not
    /// recorded").
    pub(crate) fn from_json(v: &Value) -> Result<Self, String> {
        let object = v.as_object().ok_or("identity must be an object")?;
        if let Some(key) = object
            .keys()
            .find(|k| !STRING_FIELDS.contains(&k.as_str()) && !NUMBER_FIELDS.contains(&k.as_str()))
        {
            return Err(format!("identity has unknown key {key}"));
        }
        let text = |name: &str| -> Result<Option<String>, String> {
            match object.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone()).filter(|s| !s.trim().is_empty())),
                Some(_) => Err(format!("identity {name} must be a string or null")),
            }
        };
        let number = |name: &str| -> Result<Option<f64>, String> {
            match object.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_f64()
                    .map(|n| Some(n).filter(|n| n.is_finite() && *n > 0.0))
                    .ok_or_else(|| format!("identity {name} must be a number or null")),
            }
        };
        let pixels = |name: &str| -> Result<Option<u32>, String> {
            Ok(number(name)?
                .filter(|n| *n == n.round() && *n <= f64::from(u32::MAX))
                .map(|n| n as u32))
        };
        Ok(Self {
            make: text("make")?,
            model: text("model")?,
            lens_make: text("lens_make")?,
            lens_model: text("lens_model")?,
            focal_length: number("focal_length")?,
            f_number: number("f_number")?,
            subject_distance: number("subject_distance")?,
            focal_length_35mm: number("focal_length_35mm")?,
            pixel_width: pixels("pixel_width")?,
            pixel_height: pixels("pixel_height")?,
        })
    }
}

#[derive(Deserialize)]
struct RawDb {
    database: Map<String, Value>,
    mounts: BTreeMap<String, Vec<String>>,
    cameras: Vec<Camera>,
    lenses: Vec<RawLens>,
}

#[derive(Deserialize)]
struct Camera {
    maker: String,
    model: String,
    mount: String,
    crop: f64,
    #[serde(default)]
    variant: Option<String>,
}

fn default_aspect() -> f64 {
    1.5
}

#[derive(Deserialize)]
struct RawLens {
    id: String,
    maker: String,
    model: String,
    mounts: Vec<String>,
    crop: f64,
    #[serde(default = "default_aspect")]
    aspect: f64,
    #[serde(default)]
    center: [f64; 2],
    focal: [f64; 2],
    #[serde(default)]
    poly3: Vec<[f64; 2]>,
    #[serde(default)]
    poly5: Vec<[f64; 3]>,
    #[serde(default)]
    ptlens: Vec<[f64; 4]>,
    #[serde(default)]
    tca_linear: Vec<[f64; 3]>,
    #[serde(default)]
    tca_poly3: Vec<[f64; 7]>,
    /// `[focal, aperture, distance, k1, k2, k3]`.
    #[serde(default)]
    pa: Vec<[f64; 6]>,
}

struct Lens {
    raw: RawLens,
    words: BTreeSet<String>,
    model_words: BTreeSet<String>,
    distortion: Vec<Row>,
    tca: Vec<Row>,
}

struct Database {
    header: Map<String, Value>,
    mounts: BTreeMap<String, Vec<String>>,
    cameras: Vec<Camera>,
    lenses: Vec<Lens>,
}

fn database() -> Result<&'static Database, String> {
    static DB: OnceLock<Result<Database, String>> = OnceLock::new();
    DB.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<Database, String> {
    let raw: RawDb = serde_json::from_slice(DATA)
        .map_err(|e| format!("failed to parse the bundled lens database: {e}"))?;
    let lenses = raw
        .lenses
        .into_iter()
        .map(|raw| {
            let all_words = words(&format!("{} {}", raw.maker, raw.model));
            let maker = words(&raw.maker);
            let model_words = words(&raw.model).difference(&maker).cloned().collect();
            let mut distortion: Vec<Row> = Vec::new();
            for r in &raw.poly3 {
                distortion.push(row(r[0], "poly3", &["k1"], &r[1..]));
            }
            for r in &raw.poly5 {
                distortion.push(row(r[0], "poly5", &["k1", "k2"], &r[1..]));
            }
            for r in &raw.ptlens {
                distortion.push(row(r[0], "ptlens", &["a", "b", "c"], &r[1..]));
            }
            let mut tca: Vec<Row> = Vec::new();
            for r in &raw.tca_linear {
                tca.push(row(r[0], "linear", &["kr", "kb"], &r[1..]));
            }
            for r in &raw.tca_poly3 {
                tca.push(row(
                    r[0],
                    "poly3",
                    &["vr", "cr", "br", "vb", "cb", "bb"],
                    &r[1..],
                ));
            }
            distortion.sort_by(|a, b| a.focal.total_cmp(&b.focal));
            tca.sort_by(|a, b| a.focal.total_cmp(&b.focal));
            Lens {
                raw,
                words: all_words,
                model_words,
                distortion,
                tca,
            }
        })
        .collect();
    Ok(Database {
        header: raw.database,
        mounts: raw.mounts,
        cameras: raw.cameras,
        lenses,
    })
}

fn row(focal: f64, model: &'static str, keys: &'static [&'static str], terms: &[f64]) -> Row {
    Row {
        focal,
        model,
        keys,
        terms: terms.to_vec(),
    }
}

/// Trimmed, spaces collapsed, lowercased, and a `.0` after a number dropped
/// (`f/4.0` is `f/4`).
fn normalised(text: &str) -> String {
    let lower = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = String::with_capacity(lower.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.'
            && i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1) == Some(&'0')
            && !chars.get(i + 2).is_some_and(char::is_ascii_digit)
        {
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Lensfun's words of `text` (module doc), as a set.
fn words(text: &str) -> BTreeSet<String> {
    #[derive(PartialEq, Clone, Copy)]
    enum Kind {
        Digit,
        Punct,
        Other,
    }
    let kind = |c: char| {
        if c.is_ascii_digit() {
            Kind::Digit
        } else if c.is_ascii_punctuation() {
            Kind::Punct
        } else {
            Kind::Other
        }
    };
    let mut out = BTreeSet::new();
    for word in normalised(text).split(' ') {
        let mut current = String::new();
        let mut last: Option<Kind> = None;
        let mut flush = |current: &mut String, last: Option<Kind>| {
            let single_punct = last == Some(Kind::Punct) && current.chars().count() == 1;
            if !current.is_empty() && !single_punct && current != "f" {
                out.insert(std::mem::take(current));
            }
            current.clear();
        };
        for c in word.chars() {
            let k = kind(c);
            if last.is_some_and(|l| l != k) {
                flush(&mut current, last);
            }
            current.push(c);
            last = Some(k);
        }
        flush(&mut current, last);
    }
    out
}

fn lens_json(lens: &Lens) -> Value {
    let raw = &lens.raw;
    json!({
        "id": raw.id, "maker": raw.maker, "model": raw.model, "crop_factor": raw.crop,
        "focal_min": raw.focal[0], "focal_max": raw.focal[1],
        "distortion": !lens.distortion.is_empty(), "tca": !lens.tca.is_empty(),
        "vignetting": !raw.pa.is_empty(),
    })
}

fn same(a: &str, b: &str) -> bool {
    normalised(a) == normalised(b)
}

/// The request's `dimensions`: `[w, h]`, both positive integers.
fn dimensions(v: Option<&Value>) -> Result<Option<(f64, f64)>, String> {
    let Some(v) = v else {
        return Ok(None);
    };
    let pair = v
        .as_array()
        .filter(|a| a.len() == 2)
        .and_then(|a| Some((a[0].as_u64()?, a[1].as_u64()?)))
        .filter(|(w, h)| *w > 0 && *h > 0)
        .ok_or("dimensions must be two positive integers")?;
    Ok(Some((pair.0 as f64, pair.1 as f64)))
}

/// `dr_lens_match` (the header's contract): the camera, the image's crop
/// factor, the candidate lenses, the selected one and its resolved profile,
/// or a reason there is none. Refuses an unknown key, an unknown lens id or
/// a malformed request.
pub(crate) fn match_request(request: &Value) -> Result<Value, String> {
    let object = request
        .as_object()
        .ok_or("lens match request must be an object")?;
    if let Some(key) = object
        .keys()
        .find(|k| !["identity", "lens", "all", "dimensions"].contains(&k.as_str()))
    {
        return Err(format!("lens match request has unknown key {key}"));
    }
    let identity = LensIdentity::from_json(object.get("identity").unwrap_or(&json!({})))?;
    let choice = match object.get("lens") {
        None => "auto",
        Some(v) => v.as_str().ok_or("lens must be \"auto\" or a lens id")?,
    };
    let all = match object.get("all") {
        None => false,
        Some(v) => v.as_bool().ok_or("all must be true or false")?,
    };
    let plane = dimensions(object.get("dimensions"))?;
    let db = database()?;
    let explicit = if choice == "auto" {
        None
    } else {
        Some(
            db.lenses
                .iter()
                .position(|l| l.raw.id == choice)
                .ok_or_else(|| format!("unknown lens id {choice}"))?,
        )
    };

    let camera = match (&identity.make, &identity.model) {
        (Some(make), Some(model)) => {
            let matching = || {
                db.cameras
                    .iter()
                    .filter(|c| same(&c.maker, make) && same(&c.model, model))
            };
            matching()
                .find(|c| c.variant.is_none())
                .or_else(|| matching().next())
        }
        _ => None,
    };
    let (crop, crop_source) = match camera {
        Some(c) => (Some(c.crop), Some("camera")),
        None => match (identity.focal_length_35mm, identity.focal_length) {
            (Some(f35), Some(f)) => (Some(f35 / f), Some("exif_35mm")),
            _ => (None, None),
        },
    };
    let mount_family: Option<BTreeSet<&str>> = camera.map(|c| {
        let mut family = BTreeSet::from([c.mount.as_str()]);
        if let Some(compat) = db.mounts.get(&c.mount) {
            family.extend(compat.iter().map(String::as_str));
        }
        family
    });
    let on_mount = |lens: &Lens| {
        mount_family
            .as_ref()
            .is_none_or(|family| lens.raw.mounts.iter().any(|m| family.contains(m.as_str())))
    };

    let exif_lens = [&identity.lens_make, &identity.lens_model]
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let exif_words = words(&exif_lens);
    let mut reason: Option<String> = None;
    let fixed = camera
        .filter(|c| c.mount.chars().next().is_some_and(char::is_lowercase))
        .map(|c| {
            db.lenses
                .iter()
                .enumerate()
                .filter(|(_, l)| l.raw.mounts.contains(&c.mount))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        })
        .filter(|lenses| lenses.len() == 1);
    let candidates: Vec<usize> = if let Some(fixed) = fixed {
        fixed
    } else if exif_words.is_empty() {
        reason = Some("the EXIF names no lens".into());
        Vec::new()
    } else {
        let mut ranked: Vec<(f64, f64, usize)> = db
            .lenses
            .iter()
            .enumerate()
            .filter(|(_, lens)| {
                let named = exif_words.is_subset(&lens.words)
                    || (!lens.model_words.is_empty() && lens.model_words.is_subset(&exif_words));
                let focal = identity.focal_length.is_none_or(|f| {
                    f >= lens.raw.focal[0] - FOCAL_SLACK_MM
                        && f <= lens.raw.focal[1] + FOCAL_SLACK_MM
                });
                named && focal && on_mount(lens)
            })
            .map(|(i, lens)| {
                let shared = exif_words.intersection(&lens.words).count() as f64;
                let union = exif_words.union(&lens.words).count() as f64;
                let crop_distance = crop.map_or(0.0, |c| (lens.raw.crop / c).ln().abs());
                (shared / union, crop_distance, i)
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        if ranked.is_empty() {
            reason = Some(format!("no Lensfun lens matches \"{exif_lens}\""));
        }
        ranked.into_iter().map(|(_, _, i)| i).collect()
    };

    let selected = explicit.or_else(|| candidates.first().copied());
    if explicit.is_some() {
        reason = None;
    }
    // The frame check (auto only; an explicit id is the caller's choice).
    if explicit.is_none() && selected.is_some() {
        if let (Some((pw, ph)), Some(ew), Some(eh)) =
            (plane, identity.pixel_width, identity.pixel_height)
        {
            let aspect = |w: f64, h: f64| w.max(h) / w.min(h);
            let (ew, eh) = (f64::from(ew), f64::from(eh));
            if (aspect(pw, ph) / aspect(ew, eh)).ln().abs() > FRAME_TOLERANCE {
                reason = Some(format!(
                    "the image is not the camera frame ({}×{} against EXIF {}×{}); choose a lens \
                     to apply one anyway",
                    pw, ph, ew, eh
                ));
            }
        }
    }
    let mut profile = Value::Null;
    if let (Some(index), None) = (selected, &reason) {
        match resolve(&db.lenses[index], &identity, crop) {
            Ok((p, note)) => {
                profile = p;
                reason = note;
            }
            Err(why) => reason = Some(why),
        }
    }
    let header = &db.header;
    let mut answer = json!({
        "database": {
            "name": header.get("name"), "tag": header.get("tag"),
            "license": header.get("license"), "license_url": header.get("license_url"),
            "url": header.get("url"),
        },
        "camera": camera.map(|c| json!({
            "maker": c.maker, "model": c.model, "mount": c.mount, "crop_factor": c.crop,
        })),
        "crop_factor": crop,
        "crop_factor_source": crop_source,
        "candidates": candidates.iter().map(|&i| lens_json(&db.lenses[i])).collect::<Vec<_>>(),
        "selected": selected.map(|i| lens_json(&db.lenses[i])),
        "profile": profile,
        "reason": reason,
    });
    if all {
        let family: Vec<Value> = if mount_family.is_some() {
            db.lenses
                .iter()
                .filter(|l| on_mount(l))
                .map(lens_json)
                .collect()
        } else {
            Vec::new()
        };
        answer["mount_lenses"] = Value::Array(family);
    }
    Ok(answer)
}

/// `lens`'s profile JSON at the identity's focal length, f-number and
/// distance with image crop factor `crop`, plus a note when part of it could
/// not be resolved; `Err` is the reason there is no profile.
fn resolve(
    lens: &Lens,
    identity: &LensIdentity,
    crop: Option<f64>,
) -> Result<(Value, Option<String>), String> {
    let raw = &lens.raw;
    let crop = crop.ok_or("crop factor unknown")?;
    if !(crop > 0.1 && crop <= 20.0) {
        return Err(format!("crop factor {crop} is outside 0.1–20"));
    }
    let focal = match identity.focal_length {
        Some(f) => f,
        None if raw.focal[0] == raw.focal[1] => raw.focal[0],
        None => return Err("focal length unknown".into()),
    };
    let distortion = interpolate_rows(&lens.distortion, focal);
    let tca = interpolate_rows(&lens.tca, focal);
    let mut note = None;
    let distance = identity.subject_distance.unwrap_or(DEFAULT_DISTANCE_M);
    let vignetting = match identity.f_number {
        Some(n) => pa_at(&raw.pa, focal, n, distance),
        None => {
            if !raw.pa.is_empty() {
                note = Some("f-number unknown, so vignetting is not corrected".to_owned());
            }
            None
        }
    };
    if distortion.is_none() && tca.is_none() && vignetting.is_none() {
        return Err(note.unwrap_or_else(|| "the lens has no calibration for this shot".into()));
    }
    let model = |row: Option<Row>| {
        row.map_or(Value::Null, |row| {
            let mut object = Map::new();
            object.insert("model".into(), json!(row.model));
            for (key, term) in row.keys.iter().zip(row.terms) {
                object.insert((*key).into(), json!(term));
            }
            Value::Object(object)
        })
    };
    let profile = json!({
        "id": raw.id,
        "crop_factor": crop,
        "calibration_crop_factor": raw.crop,
        "aspect_ratio": raw.aspect,
        "center": raw.center,
        "focal_length": focal,
        "f_number": identity.f_number,
        "subject_distance": identity.f_number.map(|_| distance),
        "distortion": model(distortion),
        "tca": model(tca),
        "vignetting": vignetting.map(|k| json!({"model": "pa", "k1": k[0], "k2": k[1], "k3": k[2]})),
    });
    Ok((profile, note))
}
