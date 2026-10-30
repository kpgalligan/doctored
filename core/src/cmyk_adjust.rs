//! KERNEL: Levels, Curves and Hue/Saturation on CMYK ink (#1) — the ink
//! bindings of the existing tone kernels, as free functions over plain
//! arrays — and [`CmykAdjustment`], the parsed op every CMYK caller holds.
//! The record that carries one in a document is `cmyk_adjust_layer`.
//!
//! Since the adjustments batch every other op a CMYK document takes is the
//! [`CmykAdjustment::Ink`] variant: an `InkKernel` (`cmyk_adjust_ink`'s native
//! ink ops, `cmyk_adjust_rgb`'s RGB kernels bound to ink), parsed by
//! `cmyk_adjust_rgb::parse_cmyk`, the one dispatch, which also holds the
//! refusals. [`CmykAdjustment::parse`] keeps Levels, Curves and
//! Hue/Saturation, and refuses every other op by name.
//!
//! # Hue/Saturation
//!
//! The parameters are `hue_saturation`'s own (`HueSaturation::parse`, the
//! RGB schema row unchanged, so the sliders mean what they mean on RGB), and
//! the kernel is the RGB one (`HueSaturation::apply`) run on the C, M and Y
//! plates' stored domain, with K handled as Photoshop handles it
//! ([`hue_saturation_on_ink`]; #1, fitted on the Hue/Saturation take1 and
//! confirmed by `scripts/photoshop-probe/cmyk_k_take.py`'s one take):
//!
//! * not Colorize: `out = apply([1 − C, 1 − M, 1 − Y])`, `C' = 1 − out_r`
//!   (M, Y alike) and `K' = K` exactly, so K-only greys stay K-only and the
//!   black plate is never re-separated. take1 showed Photoshop leaving K
//!   untouched on every master, Lightness and band case.
//! * Colorize (which replaces hue and saturation, so it alone moves K): the
//!   ink's naive RGB `rgb = [(1 − C)(1 − K), (1 − M)(1 − K), (1 − Y)(1 − K)]`
//!   through the kernel, then K as the result's grey component,
//!   `K' = 1 − max(out)`, and `C' = (1 − out_r − K') / (1 − K')` (0 at
//!   `K' = 1`) — a full-GCR separation of the colorized RGB.
//!
//! No profile is read: the rule is a function of the ink alone.
//!
//! # Levels
//!
//! Photoshop stores a CMYK channel as `L = 1 − ink` (white paper is 1) and
//! runs Levels in that domain, so a stage maps `L`, never the ink: one stage
//! is `adjust_levels::LevelsStage` exactly (its `parse`, its `apply`, its
//! identity) with its toe switched off (`stage`: #2 measured the toe on RGB
//! only, so a CMYK stage keeps the pure power), and
//! `ink' = 1 − master(channel(1 − ink))` — the channel's stage first, then
//! the composite's, the order `adjust_levels` measured.
//! The top-level keys are the composite (CMYK) stage's five keys; `c`, `m`,
//! `y` and `k` are optional stage objects of the same five keys.
//!
//! # Curves
//!
//! Points are INK in 0–255 units (255 = 100 % ink), the axes the user sees:
//! keys `cmyk` (the composite) and `c`, `m`, `y`, `k`, each a point list
//! parsed by `adjust_curves::parse_points` into the natural
//! `adjust_curves::Spline` (flat beyond its ends, clamped to 0–255), and
//! `ink' = S_cmyk(S_ink(255·ink)) / 255`. Writing the curve on ink rather
//! than on the stored `L` loses nothing: the point reflection `x ↦ 255 − x`,
//! `y ↦ 255 − y` maps a natural spline through the reflected points onto the
//! natural spline through the originals (the end conditions `S'' = 0` and
//! the tridiagonal system are both symmetric under it), so the two curves
//! are one curve.
//!
//! A drawn (map) curve (the pencil) is keys `cmyk_map`, `c_map`,
//! `m_map`, `y_map` and `k_map`, each exactly 256 integers 0…255
//! (`adjust_curves::parse_map`), on the same INK axes: entry `i` is the
//! output ink code for ink code `i`, the channel's map before the
//! composite's, a missing one the identity. A map key beside a point key is
//! malformed (RGB curves' single `is_map` rule). An 8-bit document reads the
//! byte tables directly, `cmyk_map[p_map[round(255·ink)]] / 255` (the 8-bit
//! RGB reading); a 16-bit one reads each plate's composed 15-bit stage table
//! (`adjust_curves::composed_map_table` / `read_map_table`, the RGB 16-bit
//! map reading). The depth crosses as `depth_bits`, so a sharp drawn map is
//! exact at 8 bits, where the stage tables would smear it.
//!
//! # Refusals
//!
//! An RGB key (`r`, `g`, `b`, `rgb` and their `_map` forms) refuses naming
//! the key; an unknown key or a value outside a stage's ranges refuses as
//! malformed. A missing ink stage or curve is the identity and is skipped,
//! never evaluated.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::adjust_curves::{
    composed_map_table, parse_map, parse_points, read_map_table, Spline, STAGE_NODES,
};
use crate::adjust_hue_saturation::HueSaturation;
use crate::adjust_levels::LevelsStage;
use crate::cmyk_adjust_ink::InkKernel;
use crate::cmyk_document::refused_sentence;

/// The ink channel keys, in C, M, Y, K order.
const INK_KEYS: [&str; 4] = ["c", "m", "y", "k"];

/// The drawn-curve keys: the composite, then C, M, Y, K (module doc).
const MAP_KEYS: [&str; 5] = ["cmyk_map", "c_map", "m_map", "y_map", "k_map"];

/// The five keys of one Levels stage.
const STAGE_KEYS: [&str; 5] = ["black", "white", "gamma", "output_black", "output_white"];

/// A parsed CMYK tone operation (module doc).
#[derive(Clone)]
pub(crate) enum CmykAdjustment {
    Levels {
        master: LevelsStage,
        inks: [LevelsStage; 4],
    },
    Curves {
        master: Option<Spline>,
        inks: [Option<Spline>; 4],
    },
    /// A drawn curve (module doc): `maps` = `[cmyk_map, c_map, m_map, y_map,
    /// k_map]` (absent = identity), read directly at 8 bits; `stages[p]` the
    /// plate's composed 15-bit table, read at 16.
    CurveMaps {
        maps: Arc<[[u8; 256]; 5]>,
        stages: Arc<[[u16; STAGE_NODES + 1]; 4]>,
    },
    /// [`hue_saturation_on_ink`] (module doc).
    HueSaturation(HueSaturation),
    /// Every other CMYK op (module doc).
    Ink(Arc<dyn InkKernel>),
}

/// Hue/Saturation on one straight ink quadruple in [0, 1] (module doc): the
/// RGB kernel on `1 − C`, `1 − M`, `1 − Y` with K kept, or under Colorize
/// the naive RGB of the ink through the kernel, separated with full GCR.
pub(crate) fn hue_saturation_on_ink(kernel: &HueSaturation, ink: [f32; 4]) -> [f32; 4] {
    let [c, m, y, k] = ink;
    if !kernel.colorizes() {
        let out = kernel.apply([1.0 - c, 1.0 - m, 1.0 - y]);
        return [1.0 - out[0], 1.0 - out[1], 1.0 - out[2], k];
    }
    let paper = 1.0 - k;
    let out = kernel.apply([(1.0 - c) * paper, (1.0 - m) * paper, (1.0 - y) * paper]);
    let k_out = (1.0 - out[0].max(out[1]).max(out[2])).clamp(0.0, 1.0);
    let room = 1.0 - k_out;
    let plate = |v: f32| {
        if room <= 0.0 {
            0.0
        } else {
            ((1.0 - v - k_out) / room).clamp(0.0, 1.0)
        }
    };
    [plate(out[0]), plate(out[1]), plate(out[2]), k_out]
}

/// The refusal of an RGB channel key in a CMYK document.
fn rgb_key(key: &str) -> String {
    refused_sentence(&format!("The RGB channel key \"{key}\""))
}

fn stage(params: &Map<String, Value>, what: &str) -> Result<LevelsStage, String> {
    if let Some(key) = params.keys().find(|k| !STAGE_KEYS.contains(&k.as_str())) {
        return Err(format!("unknown CMYK levels {what} parameter {key}"));
    }
    // A CMYK stage keeps the pure power (no toe: #2 measured RGB only).
    let mut stage = LevelsStage::parse(params)
        .ok_or_else(|| format!("invalid CMYK levels {what} parameters"))?;
    stage.toe = None;
    Ok(stage)
}

/// A curve through `(0, 0)`, `(255, 255)` and only diagonal points: the
/// natural spline through collinear points is that line, the identity
/// (`adjust_curves::CurveLuts::is_identity`'s rule).
fn straight(spline: &Spline) -> bool {
    spline.points.first() == Some(&(0.0, 0.0))
        && spline.points.last() == Some(&(255.0, 255.0))
        && spline.points.iter().all(|(x, y)| x == y)
}

impl CmykAdjustment {
    /// Parses `op`'s `params` (module doc). Every op outside Levels, Curves
    /// and Hue/Saturation refuses naming it.
    pub(crate) fn parse(op: &str, params: &Map<String, Value>) -> Result<Self, String> {
        match op {
            "levels" => Self::parse_levels(params),
            "curves" => Self::parse_curves(params),
            "hue_saturation" => HueSaturation::parse(params)
                .map(Self::HueSaturation)
                .ok_or_else(|| "invalid CMYK hue_saturation parameters".to_string()),
            other => Err(refused_sentence(&format!("The \"{other}\" adjustment"))),
        }
    }

    fn parse_levels(params: &Map<String, Value>) -> Result<Self, String> {
        let mut composite = Map::new();
        for (key, value) in params {
            match key.as_str() {
                "r" | "g" | "b" | "rgb" => return Err(rgb_key(key)),
                k if STAGE_KEYS.contains(&k) => {
                    composite.insert(key.clone(), value.clone());
                }
                k if INK_KEYS.contains(&k) => {}
                _ => return Err(format!("unknown CMYK levels parameter {key}")),
            }
        }
        let master = stage(&composite, "composite")?;
        let identity = stage(&Map::new(), "composite")?;
        let mut inks = [identity; 4];
        for (key, slot) in INK_KEYS.iter().zip(inks.iter_mut()) {
            if let Some(value) = params.get(*key) {
                let object = value
                    .as_object()
                    .ok_or_else(|| format!("CMYK levels {key} must be an object"))?;
                *slot = stage(object, key)?;
            }
        }
        Ok(Self::Levels { master, inks })
    }

    fn parse_curves(params: &Map<String, Value>) -> Result<Self, String> {
        for key in params.keys() {
            match key.as_str() {
                "r" | "g" | "b" | "rgb" | "rgb_map" | "r_map" | "g_map" | "b_map" => {
                    return Err(rgb_key(key))
                }
                "cmyk" | "c" | "m" | "y" | "k" => {}
                k if MAP_KEYS.contains(&k) => {}
                _ => return Err(format!("unknown CMYK curves parameter {key}")),
            }
        }
        if MAP_KEYS.iter().any(|key| params.contains_key(*key)) {
            return Self::parse_curve_maps(params);
        }
        let spline = |key: &str| -> Result<Option<Spline>, String> {
            let Some(value) = params.get(key) else {
                return Ok(None);
            };
            let points =
                parse_points(value).ok_or_else(|| format!("invalid CMYK curves {key} points"))?;
            let spline = Spline::new(&points);
            if !spline.m.iter().all(|v| v.is_finite()) {
                return Err(format!("invalid CMYK curves {key} points"));
            }
            Ok(Some(spline))
        };
        Ok(Self::Curves {
            master: spline("cmyk")?,
            inks: [spline("c")?, spline("m")?, spline("y")?, spline("k")?],
        })
    }

    /// A drawn curve's maps (module doc); a point key beside one is
    /// malformed.
    fn parse_curve_maps(params: &Map<String, Value>) -> Result<Self, String> {
        if ["cmyk", "c", "m", "y", "k"]
            .iter()
            .any(|key| params.contains_key(*key))
        {
            return Err("invalid CMYK curves parameters: a map key beside a point key".into());
        }
        let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut maps = [identity; 5];
        for (key, slot) in MAP_KEYS.iter().zip(maps.iter_mut()) {
            if let Some(value) = params.get(*key) {
                *slot = parse_map(value).ok_or_else(|| format!("invalid CMYK curves {key}"))?;
            }
        }
        let stages = std::array::from_fn(|p| composed_map_table(&maps[0], &maps[1 + p]));
        Ok(Self::CurveMaps {
            maps: Arc::new(maps),
            stages: Arc::new(stages),
        })
    }

    /// One straight ink quadruple in [0, 1] (module doc); identity stages
    /// and absent curves are skipped. `depth_bits` (the document's, 8 or 16)
    /// is read only by a drawn curve.
    pub(crate) fn apply(&self, ink: [f32; 4], depth_bits: u32) -> [f32; 4] {
        match self {
            Self::HueSaturation(kernel) => hue_saturation_on_ink(kernel, ink),
            Self::CurveMaps { maps, stages } => std::array::from_fn(|p| {
                if depth_bits <= 8 {
                    let index = (ink[p].clamp(0.0, 1.0) * 255.0).round() as usize;
                    f32::from(maps[0][usize::from(maps[1 + p][index])]) / 255.0
                } else {
                    read_map_table(&stages[p], ink[p])
                }
            }),
            Self::Ink(kernel) => kernel.apply(ink),
            Self::Levels { master, inks } => {
                let master = (!master.is_identity()).then_some(master);
                std::array::from_fn(|c| {
                    let mut l = 1.0 - ink[c];
                    if !inks[c].is_identity() {
                        l = inks[c].apply(l);
                    }
                    if let Some(stage) = master {
                        l = stage.apply(l);
                    }
                    1.0 - l
                })
            }
            Self::Curves { master, inks } => std::array::from_fn(|c| {
                let mut x = f64::from(ink[c]) * 255.0;
                if let Some(spline) = &inks[c] {
                    x = spline.value(x);
                }
                if let Some(spline) = master {
                    x = spline.value(x);
                }
                (x / 255.0) as f32
            }),
        }
    }

    /// Every stage or curve is the identity.
    pub(crate) fn is_identity(&self) -> bool {
        match self {
            Self::Levels { master, inks } => {
                master.is_identity() && inks.iter().all(LevelsStage::is_identity)
            }
            Self::Curves { master, inks } => std::iter::once(master)
                .chain(inks.iter())
                .all(|curve| curve.as_ref().is_none_or(straight)),
            Self::CurveMaps { maps, .. } => maps
                .iter()
                .all(|map| map.iter().enumerate().all(|(i, &v)| usize::from(v) == i)),
            Self::HueSaturation(kernel) => kernel.is_identity(),
            Self::Ink(kernel) => kernel.is_identity(),
        }
    }

    /// The heap bytes the parsed curves, the drawn curve's tables, the
    /// Hue/Saturation band table or the ink op hold (Levels holds none).
    pub(crate) fn heap_bytes(&self) -> u64 {
        match self {
            Self::Levels { .. } => 0,
            Self::CurveMaps { .. } => {
                (std::mem::size_of::<[[u8; 256]; 5]>()
                    + std::mem::size_of::<[[u16; STAGE_NODES + 1]; 4]>()) as u64
            }
            Self::Ink(kernel) => kernel.heap_bytes(),
            Self::HueSaturation(kernel) => {
                (kernel.table().capacity() * std::mem::size_of::<f32>()) as u64
            }
            Self::Curves { master, inks } => std::iter::once(master)
                .chain(inks.iter())
                .flatten()
                .map(|spline| {
                    (spline.points.capacity() * std::mem::size_of::<(f64, f64)>()
                        + spline.m.capacity() * std::mem::size_of::<f64>())
                        as u64
                })
                .sum(),
        }
    }
}
