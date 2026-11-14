//! BÉZIER PATH GEOMETRY — the kernel under `doc_path`: the path model,
//! its JSON, its caps, the outline verbs `artwork_coverage` rasterizes, the
//! control box and the point maps. It names no document type.
//!
//! **Model.** A [`PathShape`] is a fill rule and a list of [`Subpath`]s; a
//! subpath is a list of [`Anchor`]s and a `closed` flag. An anchor holds its
//! point and two handles, all ABSOLUTE canvas coordinates in the canvas's
//! continuous space (the top-left corner of pixel `(x, y)` is `(x, y)`). The
//! segment from anchor `a` to the next anchor `b` is the cubic
//! `(a.point, a.handle_out, b.handle_in, b.point)`, and a straight line when
//! both handles sit on their points. A closed subpath also has the segment
//! from its last anchor back to its first; an open one is closed by a straight
//! line for FILL only. `smooth` is an editing hint (the host keeps a smooth
//! anchor's handles collinear) and changes no geometry.
//!
//! **Quantization.** Every stored coordinate is quantized to four decimals by
//! `style_json::q4_f64`, the crate's one quantizer, when a shape is read
//! ([`PathShape::from_value`]), mapped ([`PathShape::mapped`]) or written
//! ([`PathShape::to_value`]). The stored value is therefore its own canonical
//! form: what a host reads back, echoes in, saves and reopens are all equal,
//! so an echoed shape is refused as unchanged instead of registering a
//! phantom edit. 1e-4 px is far below anything a path is drawn or rasterized
//! at, and a map applied a thousand times drifts by at most a tenth of a
//! pixel.
//!
//! **Caps.** [`MAX_PATH_ANCHORS`] and [`MAX_SUBPATHS`] bound one shape and
//! [`MAX_COORDINATE`] bounds every coordinate; `doc_path` adds the document
//! caps. A subpath with no anchor is refused, which is what lets a shape's
//! encoded size be bounded per anchor (`doc_path::MAX_DOCUMENT_PATH_ANCHORS`).
//!
//! **JSON**, the one form that crosses the FFI, is stored in `.rz` and is the
//! MCP `subpaths` argument:
//! `{"fill_rule":"nonzero"|"evenodd","subpaths":[{"closed":bool,"anchors":
//! [{"x":f,"y":f,"in":[x,y],"out":[x,y],"smooth":bool}]}]}`. `in` and `out`
//! default to the point, `smooth` and `closed` to false and `fill_rule` to
//! `nonzero`; an anchor may also be the pair `[x, y]`, a corner.
//!
//! **Segments.** Segment `i` of a subpath runs from anchor `i` to anchor
//! `i + 1`; a closed subpath's last segment (index `n − 1`) runs from its
//! last anchor back to its first. An open subpath of `n` anchors has `n − 1`
//! segments: the straight closure a fill adds is not one. A closed subpath of
//! ONE anchor has a segment only when a handle makes it a loop.
//!
//! **Coverage** ([`PathShape::coverage`]) is `artwork_coverage::rasterize`
//! over [`PathShape::commands`] and nothing else: the crate's one analytic
//! area rasterizer, so a path's fill, its selection and its vector mask all
//! agree with a shape layer drawn from the same outline. A hard edge is that
//! same coverage cut at one half.
//!
//! **Flattening** ([`PathShape::flattened`]) is for what FOLLOWS the outline
//! (a stroke, the canvas overlay), at the caller's tolerance; the master copy
//! of the subdivision is `artwork_coverage::cubic`, which flattens for
//! coverage at a fixed 1/4096 px and emits edges, not points. It lives in
//! `path_flatten`; the Pen's hit test and anchor edits
//! ([`PathShape::hit`], [`PathShape::inserting_anchor`],
//! [`PathShape::converting_anchor`]) live in `path_edit`.

use serde_json::{json, Map, Value};

use crate::artwork_coverage;
use crate::job_control::JobContext;
use crate::style_json::q4_f64;

/// The most anchors one shape may hold. Three outline points per anchor keeps
/// the largest shape (98,304 points) well inside
/// `artwork_coverage::MAX_PATH_POINTS`.
pub const MAX_PATH_ANCHORS: usize = 32_768;
/// The most subpaths one shape may hold.
pub const MAX_SUBPATHS: usize = 4_096;
/// The largest magnitude a coordinate may have. It is several times the
/// longest canvas side this build allows, so a handle may sit far outside the
/// canvas, and small enough that a quantized coordinate prints in at most
/// twelve characters (`-999999.9999`), which the `.rz` size bound relies on.
pub const MAX_COORDINATE: f64 = 1_000_000.0;
/// The most points [`PathShape::flattened`] answers over all subpaths
/// (16 MiB of points). A path drawn on a canvas flattens to a few thousand
/// at a quarter pixel; only a curve spanning much of the coordinate wall at
/// a tiny tolerance reaches this, and it is refused instead of allocated.
pub const MAX_FLATTENED_POINTS: usize = 1_048_576;
/// The finest flatten tolerance, in pixels: `artwork_coverage`'s own
/// flatness. A smaller request is flattened at this.
pub const MIN_FLATTEN_TOLERANCE: f64 = 1.0 / 4096.0;

pub(crate) type Point = [f64; 2];

/// One anchor: its point and the two handles of the segments that meet at it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub point: [f64; 2],
    /// The second control point of the segment ARRIVING at this anchor.
    pub handle_in: [f64; 2],
    /// The first control point of the segment LEAVING this anchor.
    pub handle_out: [f64; 2],
    /// An editing hint: the host keeps the two handles collinear.
    pub smooth: bool,
}

impl Anchor {
    /// A corner anchor: both handles on the point.
    pub fn corner(point: [f64; 2]) -> Self {
        Anchor {
            point,
            handle_in: point,
            handle_out: point,
            smooth: false,
        }
    }
}

/// One contour.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

/// How overlapping contours combine when the shape is filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// A whole path: every contour and the fill rule. An empty shape (no
/// subpath) is valid and covers nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct PathShape {
    pub subpaths: Vec<Subpath>,
    pub fill_rule: FillRule,
}

impl Default for PathShape {
    fn default() -> Self {
        PathShape {
            subpaths: Vec::new(),
            fill_rule: FillRule::NonZero,
        }
    }
}

/// What [`PathShape::hit`] found under a point. `index` is the anchor's
/// index in its subpath, or the segment's (the module doc's numbering).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathHit {
    None,
    Anchor {
        subpath: usize,
        index: usize,
    },
    HandleIn {
        subpath: usize,
        index: usize,
    },
    HandleOut {
        subpath: usize,
        index: usize,
    },
    /// The nearest point of segment `index`, at parameter `t` in `[0, 1]`:
    /// the cubic's own parameter on a curved segment, the fraction of the
    /// length on a straight one. It is what
    /// [`PathShape::inserting_anchor`] takes.
    Segment {
        subpath: usize,
        index: usize,
        t: f64,
    },
}

pub(crate) fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Whether the segment between two anchors is a straight line: both handles
/// sit on their points. [`PathShape::commands`] draws it with a line verb.
pub(crate) fn is_line(a: &Anchor, b: &Anchor) -> bool {
    a.handle_out == a.point && b.handle_in == b.point
}

impl Subpath {
    /// How many segments the subpath has (the module doc's rule).
    pub(crate) fn segment_count(&self) -> usize {
        let n = self.anchors.len();
        match (n, self.closed) {
            (0, _) => 0,
            (1, true) => {
                let only = &self.anchors[0];
                usize::from(only.handle_out != only.point || only.handle_in != only.point)
            }
            (_, true) => n,
            (_, false) => n - 1,
        }
    }

    /// The two anchors segment `index` joins.
    pub(crate) fn segment(&self, index: usize) -> Option<(&Anchor, &Anchor)> {
        (index < self.segment_count()).then(|| {
            (
                &self.anchors[index],
                &self.anchors[(index + 1) % self.anchors.len()],
            )
        })
    }
}

/// The working memory [`PathShape::coverage`] may hold over a `width` x
/// `height` window, beyond the shape itself: the rasterizer's edge and
/// cluster lists at `artwork_coverage::MAX_PATH_EDGES` (196 bytes an edge,
/// 118 MiB with its growth margin), the u16 plane it answers and its
/// per-row accumulation planes. The master copy of this figure, with its
/// itemization, is `ffi_artwork::dr_artwork_path_coverage`; the two move
/// together with `MAX_PATH_EDGES`.
pub(crate) fn coverage_scratch(width: u32, height: u32) -> u64 {
    118 * 1024 * 1024 + u64::from(width) * u64::from(height) * 2 + u64::from(width) * 24
}

/// A coordinate as the model stores it: quantized, with `-0.0` folded into
/// `0.0` so the printed form has one spelling.
pub(crate) fn stored(v: f64) -> f64 {
    let v = q4_f64(v);
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// A MAPPED coordinate as the model stores it. A map can send a point past
/// the coordinate wall or, for a projective map, to no finite value at all; a
/// path is never dropped, so such a coordinate is put on the wall (0 for a
/// NaN). This is float safety at ±1,000,000 px, not a clamp into the canvas.
fn stored_mapped(v: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    stored(v.clamp(-MAX_COORDINATE, MAX_COORDINATE))
}

/// One coordinate out of JSON: a finite number inside the wall, quantized
/// FIRST so what is stored is exactly what was range-checked.
fn coordinate(v: &Value) -> Result<f64, String> {
    let v = v.as_f64().ok_or("a path coordinate must be a number")?;
    if !v.is_finite() {
        return Err("a path coordinate must be finite".into());
    }
    let v = stored(v);
    if v.abs() > MAX_COORDINATE {
        return Err(format!("a path coordinate is outside ±{MAX_COORDINATE}"));
    }
    Ok(v)
}

/// A `[x, y]` pair out of JSON.
fn pair(v: &Value) -> Result<[f64; 2], String> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => Ok([coordinate(x)?, coordinate(y)?]),
        _ => Err("a path point must be an [x, y] pair".into()),
    }
}

fn anchor(v: &Value) -> Result<Anchor, String> {
    if v.is_array() {
        return Ok(Anchor::corner(pair(v)?));
    }
    let object = v
        .as_object()
        .ok_or("a path anchor must be an object or an [x, y] pair")?;
    let point = [
        coordinate(object.get("x").ok_or("a path anchor needs x")?)?,
        coordinate(object.get("y").ok_or("a path anchor needs y")?)?,
    ];
    let handle = |key: &str| object.get(key).map_or(Ok(point), pair);
    Ok(Anchor {
        point,
        handle_in: handle("in")?,
        handle_out: handle("out")?,
        smooth: flag(object, "smooth")?,
    })
}

/// An optional boolean key; absent is false.
fn flag(object: &Map<String, Value>, key: &str) -> Result<bool, String> {
    match object.get(key) {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("a path's {key} must be a boolean")),
    }
}

fn too_many_subpaths() -> String {
    format!("too many subpaths in a path (max {MAX_SUBPATHS})")
}

fn too_many_anchors() -> String {
    format!("too many anchors in a path (max {MAX_PATH_ANCHORS})")
}

const EMPTY_SUBPATH: &str = "a subpath needs at least one anchor";

impl PathShape {
    /// Reads the module doc's JSON. Refuses a value that is not that shape, a
    /// non-finite coordinate or one past [`MAX_COORDINATE`], more than
    /// [`MAX_SUBPATHS`] subpaths or [`MAX_PATH_ANCHORS`] anchors, and a
    /// subpath with no anchor. Both counts are checked against the array
    /// lengths before an anchor is read.
    pub fn from_value(v: &Value) -> Result<Self, String> {
        let object = v.as_object().ok_or("a path shape must be an object")?;
        let fill_rule = match object.get("fill_rule").map(Value::as_str) {
            None | Some(Some("nonzero")) => FillRule::NonZero,
            Some(Some("evenodd")) => FillRule::EvenOdd,
            Some(_) => return Err("a path's fill_rule must be nonzero or evenodd".into()),
        };
        let list = object
            .get("subpaths")
            .and_then(Value::as_array)
            .ok_or("a path shape needs a subpaths array")?;
        if list.len() > MAX_SUBPATHS {
            return Err(too_many_subpaths());
        }
        let mut subpaths = Vec::with_capacity(list.len());
        let mut total = 0usize;
        for entry in list {
            let entry = entry.as_object().ok_or("a subpath must be an object")?;
            let anchors = entry
                .get("anchors")
                .and_then(Value::as_array)
                .ok_or("a subpath needs an anchors array")?;
            if anchors.is_empty() {
                return Err(EMPTY_SUBPATH.into());
            }
            total += anchors.len();
            if total > MAX_PATH_ANCHORS {
                return Err(too_many_anchors());
            }
            subpaths.push(Subpath {
                anchors: anchors.iter().map(anchor).collect::<Result<_, _>>()?,
                closed: flag(entry, "closed")?,
            });
        }
        Ok(PathShape {
            subpaths,
            fill_rule,
        })
    }

    /// The module doc's JSON, every key written and every coordinate
    /// quantized, so `from_value(to_value())` is the shape itself.
    pub fn to_value(&self) -> Value {
        let subpaths: Vec<Value> = self
            .subpaths
            .iter()
            .map(|subpath| {
                let anchors: Vec<Value> = subpath
                    .anchors
                    .iter()
                    .map(|a| {
                        json!({
                            "x": stored(a.point[0]),
                            "y": stored(a.point[1]),
                            "in": [stored(a.handle_in[0]), stored(a.handle_in[1])],
                            "out": [stored(a.handle_out[0]), stored(a.handle_out[1])],
                            "smooth": a.smooth,
                        })
                    })
                    .collect();
                json!({"closed": subpath.closed, "anchors": anchors})
            })
            .collect();
        json!({
            "fill_rule": match self.fill_rule {
                FillRule::NonZero => "nonzero",
                FillRule::EvenOdd => "evenodd",
            },
            "subpaths": subpaths,
        })
    }

    /// Why this shape may not be stored, or `None`: the rules
    /// [`Self::from_value`] enforces, for a shape built in memory.
    pub fn refusal(&self) -> Option<String> {
        if self.subpaths.len() > MAX_SUBPATHS {
            return Some(too_many_subpaths());
        }
        if self.subpaths.iter().any(|s| s.anchors.is_empty()) {
            return Some(EMPTY_SUBPATH.into());
        }
        if self.anchor_count() > MAX_PATH_ANCHORS {
            return Some(too_many_anchors());
        }
        let inside = |v: f64| v.is_finite() && stored(v).abs() <= MAX_COORDINATE;
        if !self
            .subpaths
            .iter()
            .flat_map(|s| &s.anchors)
            .flat_map(|a| [a.point, a.handle_in, a.handle_out])
            .flatten()
            .all(inside)
        {
            return Some(format!(
                "a path coordinate is not finite or is outside ±{MAX_COORDINATE}"
            ));
        }
        None
    }

    /// The shape as the model stores it — every coordinate quantized — or its
    /// [`Self::refusal`].
    pub fn validated(mut self) -> Result<Self, String> {
        if let Some(reason) = self.refusal() {
            return Err(reason);
        }
        for anchor in self.subpaths.iter_mut().flat_map(|s| &mut s.anchors) {
            for p in [
                &mut anchor.point,
                &mut anchor.handle_in,
                &mut anchor.handle_out,
            ] {
                *p = [stored(p[0]), stored(p[1])];
            }
        }
        Ok(self)
    }

    /// The anchors of every subpath.
    pub fn anchor_count(&self) -> usize {
        self.subpaths.iter().map(|s| s.anchors.len()).sum()
    }

    /// The FILL outline as `artwork_coverage`'s verbs (0 move, 1 line, 3
    /// cubic, 4 close) and their points, shifted so `origin` becomes (0, 0).
    /// A segment whose two handles sit on their points is a line. Every
    /// subpath ends with a close: a closed one after its closing segment, an
    /// open one in place of it (the straight closure a fill uses).
    pub fn commands(&self, origin: (f64, f64)) -> (Vec<u8>, Vec<[f64; 2]>) {
        let at = |p: [f64; 2]| [p[0] - origin.0, p[1] - origin.1];
        let mut verbs = Vec::new();
        let mut points = Vec::new();
        for subpath in &self.subpaths {
            let Some(first) = subpath.anchors.first() else {
                continue;
            };
            verbs.push(0);
            points.push(at(first.point));
            let mut segment = |a: &Anchor, b: &Anchor| {
                if a.handle_out == a.point && b.handle_in == b.point {
                    verbs.push(1);
                    points.push(at(b.point));
                } else {
                    verbs.push(3);
                    points.extend([at(a.handle_out), at(b.handle_in), at(b.point)]);
                }
            };
            for pair in subpath.anchors.windows(2) {
                segment(&pair[0], &pair[1]);
            }
            if subpath.closed {
                let last = &subpath.anchors[subpath.anchors.len() - 1];
                // A one-anchor closed subpath has a closing segment only when
                // a handle makes it a loop.
                if subpath.anchors.len() > 1
                    || last.handle_out != last.point
                    || last.handle_in != last.point
                {
                    segment(last, first);
                }
            }
            verbs.push(4);
        }
        (verbs, points)
    }

    /// The box `[min x, min y, max x, max y]` of every point and handle — the
    /// control hull, which contains the curve. `None` for an empty shape.
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut all = self
            .subpaths
            .iter()
            .flat_map(|s| &s.anchors)
            .flat_map(|a| [a.point, a.handle_in, a.handle_out]);
        let first = all.next()?;
        Some(all.fold([first[0], first[1], first[0], first[1]], |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        }))
    }

    /// Every point and handle through `map`, stored as the model stores a
    /// coordinate. Nothing is dropped; a coordinate the map sends past
    /// [`MAX_COORDINATE`] or to no finite value lands on the wall (0 for a
    /// NaN), so a mapped shape is always one [`Self::refusal`] accepts.
    pub fn mapped(&self, map: impl Fn(f64, f64) -> (f64, f64)) -> Self {
        let point = |p: [f64; 2]| {
            let (x, y) = map(p[0], p[1]);
            [stored_mapped(x), stored_mapped(y)]
        };
        PathShape {
            subpaths: self
                .subpaths
                .iter()
                .map(|subpath| Subpath {
                    anchors: subpath
                        .anchors
                        .iter()
                        .map(|a| Anchor {
                            point: point(a.point),
                            handle_in: point(a.handle_in),
                            handle_out: point(a.handle_out),
                            smooth: a.smooth,
                        })
                        .collect(),
                    closed: subpath.closed,
                })
                .collect(),
            fill_rule: self.fill_rule,
        }
    }

    /// [`Self::mapped`] by the affine `m = [a, b, c, d, tx, ty]`, the
    /// convention of `doc_transform::Affine::apply`: `x' = a·x + c·y + tx`,
    /// `y' = b·x + d·y + ty`.
    pub fn transformed(&self, m: [f64; 6]) -> Self {
        self.mapped(|x, y| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]))
    }

    /// The shape's FILL as u16 area coverage over the `width` x `height`
    /// window whose top-left pixel is canvas pixel `origin`, row-major:
    /// 65535 is a wholly covered pixel. It is `artwork_coverage::rasterize`
    /// over [`Self::commands`] under the shape's fill rule — the crate's one
    /// coverage rasterizer. With `anti_alias` false each sample is cut at one
    /// half (32768 and above is 65535, anything less 0). An empty shape
    /// covers nothing. `Err` is the rasterizer's own refusal (a zero or
    /// oversized window, its edge and work caps) or a cancellation.
    pub(crate) fn coverage(
        &self,
        origin: (i32, i32),
        width: u32,
        height: u32,
        anti_alias: bool,
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let (verbs, points) = self.commands((f64::from(origin.0), f64::from(origin.1)));
        let even_odd = self.fill_rule == FillRule::EvenOdd;
        let mut coverage =
            artwork_coverage::rasterize(&verbs, &points, width, height, even_odd, job)?;
        if !anti_alias {
            for sample in &mut coverage {
                *sample = if *sample >= 32768 { u16::MAX } else { 0 };
            }
        }
        Ok(coverage)
    }
}
