//! The Duotone description's kernel (#1 Duotone phase 1a,
//! `designs/duotone-mode.md`): the inks, their curves, the overprint
//! colours, their one JSON form and the colour mix the view shows.
//!
//! # The description
//!
//! One to [`MAX_INKS`] inks (Monotone, Duotone, Tritone and Quadtone), each
//! a name of 1 to [`MAX_NAME_CHARS`] characters, a D50 CIE Lab colour and a
//! transfer curve of 13 slots, the inputs [`CURVE_INPUTS`] in percent; a
//! slot is an output percentage or blank, and the 0 % and 100 % slots must
//! be set. Ink colours are Lab because the PSD duotone record carries a Lab
//! colour, and Lab keeps the view and later conversions independent of the
//! working RGB. Beside the
//! inks, one overprint colour per combination of two or more inks
//! ([`overprint_subsets`]: by size, then lexicographic), each authored Lab
//! or blank (the default).
//!
//! # The mix (declared; unmeasured until Duotone phase 2's take)
//!
//! A gray code `g` (unit) is ink `k = 1 − g` (Gray's K % rule). Ink `i`'s
//! coverage is `cᵢ = Sᵢ(255k)/255`, `Sᵢ` Curves' natural cubic spline
//! (`adjust_curves::Spline`, flat beyond its ends, clamped) through the set
//! slots `(2.55·xⱼ, 2.55·yⱼ)`. The coverage enters the mix through the
//! document's Gray tone curve, `c′ᵢ = 1 − decode(1 − cᵢ)` (the caller's
//! `decode`, the Gray profile's kTRC), so a Monotone Black ink on a straight
//! curve reproduces the Grayscale display. The colour is the Neugebauer sum
//! over every subset `S` of the inks of `Πᵢ∈S c′ᵢ · Πⱼ∉S (1 − c′ⱼ) · P_S` in
//! PCS XYZ (D50): `P_∅` the paper, Lab (100, 0, 0); `P_{i}` ink `i`'s
//! colour; `P_S` for two or more inks the authored overprint, else
//! `W ⊙ Πᵢ∈S (Pᵢ ⊘ W)` per component (`W` the paper), so the default sum is
//! the subtractive product `W ⊙ Πᵢ (1 − c′ᵢ + c′ᵢ·Pᵢ/W)`. Lab⇄XYZ is
//! `icc_lut`'s one pair.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::adjust_curves::Spline;
use crate::duotone_ado::DuotoneFileBlock;
use crate::icc_lut::{lab_to_xyz, xyz_to_lab};

/// The Duotone Curve dialog's 13 inputs, in percent.
pub(crate) const CURVE_INPUTS: [f64; 13] = [
    0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 100.0,
];

/// Quadtone's four inks.
pub(crate) const MAX_INKS: usize = 4;

/// The longest ink name, in characters.
pub(crate) const MAX_NAME_CHARS: usize = 64;

/// One ink (module doc).
#[derive(Clone, Debug, PartialEq)]
pub struct DuotoneInk {
    pub name: String,
    /// D50 CIE Lab.
    pub lab: [f64; 3],
    /// Output percent per [`CURVE_INPUTS`] slot; `None` is blank.
    pub curve: [Option<f64>; 13],
}

/// The description (module doc): its inks and one overprint slot per
/// [`overprint_subsets`] entry, in that order.
#[derive(Clone, Debug)]
pub struct DuotoneInks {
    inks: Vec<DuotoneInk>,
    overprints: Vec<Option<[f64; 3]>>,
    /// `P_S` per subset mask, derived once from the two above.
    primaries: Vec<[f64; 3]>,
    /// The file block this description was read from (`duotone_ado`,
    /// phase 1b), so an unchanged description writes it back byte for byte.
    /// Equality ignores it: it is where the inks came from, not what they
    /// are.
    carried: Option<Arc<DuotoneFileBlock>>,
}

impl PartialEq for DuotoneInks {
    fn eq(&self, other: &Self) -> bool {
        self.inks == other.inks && self.overprints == other.overprints
    }
}

/// Every combination of two or more of `n` inks (0-based indices), by size
/// and then lexicographic: `1+2`; `1+2, 1+3, 2+3, 1+2+3`; …
pub(crate) fn overprint_subsets(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for size in 2..=n {
        let mut pick: Vec<usize> = (0..size).collect();
        loop {
            out.push(pick.clone());
            // The next combination in lexicographic order, if any.
            let Some(at) = (0..size).rev().find(|&i| pick[i] < n - size + i) else {
                break;
            };
            pick[at] += 1;
            for i in at + 1..size {
                pick[i] = pick[i - 1] + 1;
            }
        }
    }
    out
}

/// A combination's key, its 1-based ink numbers joined by `+`.
fn subset_key(subset: &[usize]) -> String {
    subset
        .iter()
        .map(|i| (i + 1).to_string())
        .collect::<Vec<_>>()
        .join("+")
}

/// `value` as a Lab triple in range, else `None`.
fn lab_value(value: &Value) -> Option<[f64; 3]> {
    let items = value.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let lab = [items[0].as_f64()?, items[1].as_f64()?, items[2].as_f64()?];
    lab_in_range(lab).then_some(lab)
}

pub(crate) fn lab_in_range(lab: [f64; 3]) -> bool {
    lab.iter().all(|v| v.is_finite())
        && (0.0..=100.0).contains(&lab[0])
        && (-128.0..=127.0).contains(&lab[1])
        && (-128.0..=127.0).contains(&lab[2])
}

/// The range sentence of a Lab field.
pub(crate) fn lab_sentence(what: &str) -> String {
    format!("{what} must be [L, a, b] with L from 0 to 100 and a and b from -128 to 127")
}

impl DuotoneInk {
    /// Checks one ink (`number` 1-based, for the sentences).
    fn check(&self, number: usize) -> Result<(), String> {
        let chars = self.name.chars().count();
        if chars == 0 || chars > MAX_NAME_CHARS {
            return Err(format!(
                "ink {number}'s name must be 1 to {MAX_NAME_CHARS} characters"
            ));
        }
        if !lab_in_range(self.lab) {
            return Err(lab_sentence(&format!("ink {number}'s lab")));
        }
        if self
            .curve
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=100.0).contains(v))
        {
            return Err(curve_sentence(number));
        }
        if self.curve[0].is_none() || self.curve[12].is_none() {
            return Err(format!(
                "ink {number}'s curve must set its 0 % and 100 % slots"
            ));
        }
        Ok(())
    }

    fn parse(value: &Value, number: usize) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or_else(|| format!("ink {number} must be an object with name, lab and curve"))?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "name" | "lab" | "curve"))
        {
            return Err(format!("ink {number} has an unknown key \"{key}\""));
        }
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("ink {number}'s name must be 1 to {MAX_NAME_CHARS} characters"))?
            .to_string();
        let lab = object
            .get("lab")
            .and_then(lab_value)
            .ok_or_else(|| lab_sentence(&format!("ink {number}'s lab")))?;
        let slots = object
            .get("curve")
            .and_then(Value::as_array)
            .filter(|slots| slots.len() == CURVE_INPUTS.len())
            .ok_or_else(|| curve_sentence(number))?;
        let mut curve = [None; 13];
        for (slot, value) in curve.iter_mut().zip(slots) {
            *slot = match value {
                Value::Null => None,
                other => Some(other.as_f64().ok_or_else(|| curve_sentence(number))?),
            };
        }
        let ink = Self { name, lab, curve };
        ink.check(number)?;
        Ok(ink)
    }

    fn to_json(&self) -> Value {
        json!({"name": self.name, "lab": self.lab, "curve": self.curve})
    }

    /// The coverage spline through the set slots (module doc).
    fn spline(&self) -> Spline {
        let points: Vec<(f64, f64)> = CURVE_INPUTS
            .iter()
            .zip(&self.curve)
            .filter_map(|(&x, y)| y.map(|y| (2.55 * x, 2.55 * y)))
            .collect();
        Spline::new(&points)
    }
}

fn curve_sentence(number: usize) -> String {
    format!(
        "ink {number}'s curve must be 13 numbers from 0 to 100 or null, for the inputs 0, 5, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95 and 100 %"
    )
}

impl DuotoneInks {
    /// A checked description: 1 to [`MAX_INKS`] inks, each checked, and one
    /// overprint slot per combination, each blank or a Lab in range.
    pub fn new(inks: Vec<DuotoneInk>, overprints: Vec<Option<[f64; 3]>>) -> Result<Self, String> {
        if inks.is_empty() || inks.len() > MAX_INKS {
            return Err(format!(
                "A Duotone holds 1 to {MAX_INKS} inks (Monotone, Duotone, Tritone or Quadtone), not {}.",
                inks.len()
            ));
        }
        for (i, ink) in inks.iter().enumerate() {
            ink.check(i + 1)?;
        }
        let subsets = overprint_subsets(inks.len());
        if overprints.len() != subsets.len() {
            return Err(format!(
                "{} inks take {} overprint colours, not {}",
                inks.len(),
                subsets.len(),
                overprints.len()
            ));
        }
        for (subset, lab) in subsets.iter().zip(&overprints) {
            if lab.is_some_and(|lab| !lab_in_range(lab)) {
                return Err(lab_sentence(&format!("overprint {}", subset_key(subset))));
            }
        }
        let mut out = Self {
            inks,
            overprints,
            primaries: Vec::new(),
            carried: None,
        };
        out.primaries = out.primaries();
        Ok(out)
    }

    /// The file block this description was read from, if any.
    pub(crate) fn carried(&self) -> Option<&DuotoneFileBlock> {
        self.carried.as_deref()
    }

    /// The same description carrying `block`, the file it was read from.
    pub(crate) fn with_carried(self, block: DuotoneFileBlock) -> Self {
        Self {
            carried: Some(Arc::new(block)),
            ..self
        }
    }

    /// Each combination's effective colour as Lab, in [`overprint_subsets`]
    /// order: the authored overprint, else the default product's.
    pub(crate) fn overprint_colours(&self) -> Vec<[f64; 3]> {
        overprint_subsets(self.inks.len())
            .iter()
            .zip(&self.overprints)
            .map(|(subset, lab)| {
                lab.unwrap_or_else(|| {
                    let mask: usize = subset.iter().map(|i| 1 << i).sum();
                    xyz_to_lab(self.primaries[mask])
                })
            })
            .collect()
    }

    /// The options JSON (`{"inks":[…],"overprints":{key:[L,a,b]|null}}`;
    /// unknown keys refuse, a missing or null overprint is the default).
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("duotone options are not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("duotone options must be a JSON object")?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "inks" | "overprints"))
        {
            return Err(format!("duotone options have an unknown key \"{key}\""));
        }
        let list = object
            .get("inks")
            .and_then(Value::as_array)
            .ok_or("duotone options need inks, a list of 1 to 4 inks")?;
        if list.is_empty() || list.len() > MAX_INKS {
            return Err(format!(
                "A Duotone holds 1 to {MAX_INKS} inks (Monotone, Duotone, Tritone or Quadtone), not {}.",
                list.len()
            ));
        }
        let inks = list
            .iter()
            .enumerate()
            .map(|(i, ink)| DuotoneInk::parse(ink, i + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let subsets = overprint_subsets(inks.len());
        let keys: Vec<String> = subsets.iter().map(|s| subset_key(s)).collect();
        let mut overprints = vec![None; subsets.len()];
        match object.get("overprints") {
            None | Some(Value::Null) => {}
            Some(Value::Object(entries)) => {
                for (key, value) in entries {
                    let at = keys.iter().position(|k| k == key).ok_or_else(|| {
                        format!(
                            "overprints key \"{key}\" is not a combination of two or more of the {} inks (\"1+2\", …)",
                            inks.len()
                        )
                    })?;
                    overprints[at] = match value {
                        Value::Null => None,
                        other => Some(
                            lab_value(other)
                                .ok_or_else(|| lab_sentence(&format!("overprint {key}")))?,
                        ),
                    };
                }
            }
            Some(_) => return Err("overprints must be an object keyed \"1+2\", …".into()),
        }
        Self::new(inks, overprints)
    }

    pub fn inks(&self) -> &[DuotoneInk] {
        &self.inks
    }

    /// One slot per [`overprint_subsets`] entry; `None` is the default.
    pub fn overprints(&self) -> &[Option<[f64; 3]>] {
        &self.overprints
    }

    /// The Type popup's name for the ink count.
    pub fn type_name(&self) -> &'static str {
        match self.inks.len() {
            1 => "monotone",
            2 => "duotone",
            3 => "tritone",
            _ => "quadtone",
        }
    }

    /// The normalized options: the type, every ink, every overprint key
    /// (null where default) and `overprint_display`, each combination's
    /// effective colour as Lab.
    pub fn to_json(&self) -> Value {
        let primaries = &self.primaries;
        let mut overprints = Map::new();
        let mut display = Map::new();
        for (subset, lab) in overprint_subsets(self.inks.len())
            .iter()
            .zip(&self.overprints)
        {
            let key = subset_key(subset);
            overprints.insert(key.clone(), lab.map_or(Value::Null, |lab| json!(lab)));
            let mask: usize = subset.iter().map(|i| 1 << i).sum();
            display.insert(key, json!(xyz_to_lab(primaries[mask])));
        }
        json!({
            "type": self.type_name(),
            "inks": self.inks.iter().map(DuotoneInk::to_json).collect::<Vec<_>>(),
            "overprints": overprints,
            "overprint_display": display,
        })
    }

    /// Each ink's coverage spline, in ink order.
    pub fn coverages(&self) -> Vec<Spline> {
        self.inks.iter().map(DuotoneInk::spline).collect()
    }

    /// `P_S` in PCS XYZ for every subset mask (bit `i` = ink `i`): the
    /// paper, the inks, the authored overprints or their default product
    /// (module doc).
    fn primaries(&self) -> Vec<[f64; 3]> {
        let n = self.inks.len();
        let paper = lab_to_xyz([100.0, 0.0, 0.0]);
        let inks: Vec<[f64; 3]> = self.inks.iter().map(|ink| lab_to_xyz(ink.lab)).collect();
        let mut out = vec![paper; 1 << n];
        for (mask, slot) in out.iter_mut().enumerate().skip(1) {
            // The default: W ⊙ Πᵢ (Pᵢ ⊘ W), a single ink's colour itself.
            *slot = std::array::from_fn(|c| {
                (0..n)
                    .filter(|i| mask & (1 << i) != 0)
                    .fold(paper[c], |acc, i| acc * inks[i][c] / paper[c])
            });
        }
        for (subset, lab) in overprint_subsets(n).iter().zip(&self.overprints) {
            if let Some(lab) = lab {
                let mask: usize = subset.iter().map(|i| 1 << i).sum();
                out[mask] = lab_to_xyz(*lab);
            }
        }
        out
    }

    /// The colour of ink `k` (0…1) in PCS XYZ (D50): the module doc's
    /// Neugebauer sum, `splines` [`Self::coverages`], `decode` the Gray tone
    /// curve (a unit code to its relative luminance).
    pub fn mix_xyz(&self, splines: &[Spline], k: f64, decode: &dyn Fn(f64) -> f64) -> [f64; 3] {
        let coverage: Vec<f64> = splines
            .iter()
            .map(|spline| {
                let c = (spline.value(255.0 * k.clamp(0.0, 1.0)) / 255.0).clamp(0.0, 1.0);
                (1.0 - decode(1.0 - c)).clamp(0.0, 1.0)
            })
            .collect();
        let mut xyz = [0.0; 3];
        for (mask, primary) in self.primaries.iter().enumerate() {
            let weight: f64 = coverage
                .iter()
                .enumerate()
                .map(|(i, &c)| if mask & (1 << i) != 0 { c } else { 1.0 - c })
                .product();
            for (slot, p) in xyz.iter_mut().zip(primary) {
                *slot += weight * p;
            }
        }
        xyz
    }
}
