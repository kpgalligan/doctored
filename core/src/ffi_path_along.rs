//! C FFI for distance along a path and outline maps — the three exports after
//! `dr_path_subpath_at` in the "Paths" section of `include/doctored_core.h`,
//! over the kernels `path_along` and `outline_map`. They take no document:
//! one shape as the shape JSON (read by `ffi_path::shape`), text back through
//! `ffi_path::owned`. `dr_path_frames` WRITES the caller's frame buffer (a
//! caller-owned buffer, as core/CLAUDE.md's "Purity" allows), and only on
//! success.

use std::ffi::c_char;
use std::fmt::Write as _;
use std::ptr;

use serde_json::Value;

use crate::artwork_coverage::MAX_PATH_POINTS;
use crate::ffi_path::{owned, shape, SHAPE_JSON_LIMIT};
use crate::ffi_util::{borrow_cstr_bounded, fallible_op};
use crate::outline_map::{map_outline, OutlineMap};
use crate::path_along::{Along, MAX_FRAMES};
use crate::path_geometry::PathShape;

/// The longest map JSON `dr_outline_map` reads: the shape JSON's own cap
/// (`ffi_path`'s `SHAPE_JSON_LIMIT`) plus room for the map's few scalar keys.
const MAP_JSON_LIMIT: usize = SHAPE_JSON_LIMIT + 4096;

/// A finite number under `key` of the map object, 0 when absent.
fn number(map: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    match map.get(key) {
        None => Ok(0.0),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("an outline map's {key} must be a finite number")),
    }
}

/// The subpath index of a shape export (`u32` at the boundary).
fn index(subpath: u32) -> usize {
    subpath as usize
}

/// The length of subpath `subpath` of the shape in `shape_json`, closing
/// segment included, measured over its flattening at 1/64 px
/// (`path_along`'s module doc bounds the error). -1 with `err_out` set for
/// NULL or malformed JSON, a refused shape, no such subpath, or a subpath
/// with no length.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `err_out` must be
/// NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_length(
    shape_json: *const c_char,
    subpath: u32,
    err_out: *mut *mut c_char,
) -> f64 {
    unsafe {
        fallible_op(
            err_out,
            "measuring a path",
            -1.0,
            || Ok(Along::new(&shape(shape_json)?, index(subpath), false)?.length()),
            |length| length,
        )
    }
}

/// The frame at each of `count` distances along subpath `subpath` (measured
/// from its other end when `reversed`), written to the CALLER'S `frames`
/// buffer as `x, y, tx, ty` per distance, the tangent of unit length. A
/// closed subpath wraps; an open one writes four NaN where a distance lies
/// outside `0..=length`, as it does for a distance that is not finite.
/// `frames_count` must be exactly `count * 4` and `count` at most 65,536;
/// `distances` and `frames` may be NULL only when `count` is 0. False with
/// `err_out` set for any of those, NULL or malformed JSON, a refused shape,
/// no such subpath or a subpath with no length, and then nothing is written.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `distances` must be
/// NULL or readable for `count` doubles; `frames` must be NULL or writable
/// for `frames_count` doubles; `err_out` must be NULL or a writable CString
/// pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_frames(
    shape_json: *const c_char,
    subpath: u32,
    reversed: bool,
    distances: *const f64,
    count: usize,
    frames: *mut f64,
    frames_count: usize,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "framing a path",
            false,
            || {
                if count > MAX_FRAMES
                    || count.checked_mul(4) != Some(frames_count)
                    || (count > 0 && (distances.is_null() || frames.is_null()))
                {
                    return Err("invalid path frames buffers".into());
                }
                let along = Along::new(&shape(shape_json)?, index(subpath), reversed)?;
                if count == 0 {
                    return Ok(());
                }
                // `count` is bounded above and `frames_count` recomputed
                // from it.
                let wanted = std::slice::from_raw_parts(distances, count);
                let mut values = Vec::with_capacity(count * 4);
                for &d in wanted {
                    match along.frame(d) {
                        Some(f) => {
                            values.extend([f.point[0], f.point[1], f.tangent[0], f.tangent[1]])
                        }
                        None => values.extend([f64::NAN; 4]),
                    }
                }
                std::slice::from_raw_parts_mut(frames, count * 4).copy_from_slice(&values);
                Ok(())
            },
            |()| true,
        )
    }
}

/// The outline (`dr_artwork_path_coverage`'s verbs 0 move, 1 line, 2 quad,
/// 3 cubic, 4 close and `point_count` x, y pairs) through the map in
/// `map_json`, as `{"commands":[...],"points":[[x,y],...]}` with only verbs
/// 0, 1 and 4, each piece within `tolerance` pixels of the mapped curve
/// (`outline_map`'s module doc). `map_json` is
/// `{"kind":"along","shape":<shape JSON>,"subpath":i,"reversed":bool,
/// "start":n,"baseline":n,"shift":n}`. NULL with `err_out` set for a NULL
/// buffer with a non-zero count, more than 262,144 verbs or points in or
/// out, an unknown verb, a point count that does not match, a coordinate
/// that is not finite, NULL or malformed map JSON, an unknown kind, a number
/// that is not finite, no such subpath or one with no length, and a
/// tolerance that is not positive and finite (a smaller one is raised to
/// 1/4096). Free with dr_string_free.
///
/// # Safety
/// `commands` must be NULL or readable for `command_count` bytes; `points`
/// must be NULL or readable for `point_count * 2` doubles; `map_json` must be
/// NULL or a NUL-terminated string; `err_out` must be NULL or a writable
/// CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_outline_map(
    commands: *const u8,
    command_count: usize,
    points: *const f64,
    point_count: usize,
    map_json: *const c_char,
    tolerance: f64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "mapping an outline",
            ptr::null_mut(),
            || {
                if (commands.is_null() && command_count != 0)
                    || (points.is_null() && point_count != 0)
                    || command_count > MAX_PATH_POINTS
                    || point_count > MAX_PATH_POINTS
                {
                    return Err("invalid outline map buffers".into());
                }
                let text = borrow_cstr_bounded(map_json, "outline map json", MAP_JSON_LIMIT)?;
                let value: Value = serde_json::from_str(text)
                    .map_err(|e| format!("outline map json is malformed: {e}"))?;
                let map = value
                    .as_object()
                    .ok_or("an outline map must be an object")?;
                if map.get("kind").and_then(Value::as_str) != Some("along") {
                    return Err("an outline map's kind must be along".into());
                }
                let path =
                    PathShape::from_value(map.get("shape").ok_or("an outline map needs a shape")?)?;
                let subpath = map
                    .get("subpath")
                    .and_then(Value::as_u64)
                    .and_then(|i| usize::try_from(i).ok())
                    .ok_or("an outline map's subpath must be an index")?;
                let reversed = match map.get("reversed") {
                    None => false,
                    Some(v) => v
                        .as_bool()
                        .ok_or("an outline map's reversed must be a boolean")?,
                };
                let (start, baseline, shift) = (
                    number(map, "start")?,
                    number(map, "baseline")?,
                    number(map, "shift")?,
                );
                let along = Along::new(&path, subpath, reversed)?;
                let commands = if command_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(commands, command_count)
                };
                let points = if point_count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(points.cast::<[f64; 2]>(), point_count)
                };
                let map = OutlineMap::Along {
                    along: &along,
                    start,
                    baseline,
                    shift,
                };
                let (verbs, mapped) = map_outline(commands, points, &map, tolerance)?;
                // Written directly, every coordinate at full precision (a
                // shortest round-trip form), so the host rebuilds exactly
                // the points the map produced.
                let mut out = String::from("{\"commands\":[");
                for (i, verb) in verbs.iter().enumerate() {
                    let comma = if i > 0 { "," } else { "" };
                    let _ = write!(out, "{comma}{verb}");
                }
                out.push_str("],\"points\":[");
                for (i, p) in mapped.iter().enumerate() {
                    let comma = if i > 0 { "," } else { "" };
                    let _ = write!(out, "{comma}[{},{}]", p[0], p[1]);
                }
                out.push_str("]}");
                Ok(out)
            },
            owned,
        )
    }
}
