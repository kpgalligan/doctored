//! C FFI for Edit ▸ Puppet Warp: the workspace's mesh and pose
//! handles (`dr_puppet_*`, `dr_doc_puppet_mesh`,
//! `dr_precision_doc_puppet_mesh`) and the committed layer edit at both
//! depths (`dr_doc_puppet_warp(_plan)`, `dr_precision_doc_puppet_warp(_plan)`).
//! The float form commits through the selected-pixels edits
//! (`"op":"puppet_selected"`).
//!
//! Thin shims over the shared `ffi_util` helpers: a mesh and a pose are
//! immutable handles, a pose keeps its own reference to its mesh (so either
//! may be freed first), and nothing unwinds across the boundary.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_puppet::{PuppetWarpRequest, REQUEST_LIMIT};
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, free_handle, required_handle,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::puppet_arap::{solve, Mode, PuppetPose};
use crate::puppet_mesh::{MeshOptions, PuppetMesh};
use crate::puppet_render::{render_preview, PoseMap};
use crate::DrImage;

/// The mesh options' byte cap.
const OPTIONS_LIMIT: usize = 4 * 1024;

fn json_string(value: serde_json::Value) -> Result<*mut c_char, String> {
    CString::new(value.to_string())
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}

/// A canvas-sized coverage slice: NULL is the layer form; otherwise
/// `coverage_len` must equal the canvas's own `width·height`, which is the
/// length the slice is made with.
///
/// # Safety
/// `coverage` must be NULL or valid for `coverage_len` u16 reads.
unsafe fn canvas_coverage<'a>(
    coverage: *const u16,
    coverage_len: usize,
    canvas: (u32, u32),
) -> Result<Option<&'a [u16]>, String> {
    if coverage.is_null() {
        return Ok(None);
    }
    let len = u64::from(canvas.0) * u64::from(canvas.1);
    if coverage_len as u64 != len {
        return Err("puppet warp coverage must be canvas-sized u16".into());
    }
    Ok(Some(unsafe {
        std::slice::from_raw_parts(coverage, len as usize)
    }))
}

/// The canvas size both document types answer.
trait Canvas {
    fn canvas(&self) -> (u32, u32);
}
impl Canvas for DrDocument {
    fn canvas(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
impl Canvas for PrecisionDocument {
    fn canvas(&self) -> (u32, u32) {
        self.dimensions()
    }
}

macro_rules! mesh_ffi {
    ($doc:ty, $name:ident) => {
        /// The mesh of layer `layer`'s opaque pixels (NULL `coverage`), or of
        /// its opaque AND selected pixels (`coverage` canvas-sized u16, the
        /// float form), with `options_json` `{"density","expansion"}`. The
        /// build is admitted first (`mesh_plan_bytes` against `limit` and
        /// `other`). NULL document returns NULL with no error; a refusal
        /// returns NULL with its sentence through `err_out`.
        ///
        /// # Safety
        /// `document` is NULL or live; `coverage` is NULL or valid for
        /// `coverage_len` u16 reads; `options_json` is NULL or NUL
        /// terminated; `err_out` is NULL or writable. Free the message with
        /// dr_string_free and the mesh with dr_puppet_mesh_free.
        #[no_mangle]
        #[allow(clippy::too_many_arguments)]
        pub unsafe extern "C" fn $name(
            document: *const $doc,
            layer: usize,
            coverage: *const u16,
            coverage_len: usize,
            options_json: *const c_char,
            limit: u64,
            other: u64,
            err_out: *mut *mut c_char,
        ) -> *mut PuppetMesh {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                fallible_op(
                    err_out,
                    "building a puppet warp mesh",
                    ptr::null_mut(),
                    || {
                        let document = required_handle(document)?;
                        let options = MeshOptions::from_json(borrow_cstr_bounded(
                            options_json,
                            "options",
                            OPTIONS_LIMIT,
                        )?)?;
                        let coverage =
                            canvas_coverage(coverage, coverage_len, Canvas::canvas(document))?;
                        document.puppet_mesh(
                            layer,
                            coverage,
                            &options,
                            limit,
                            other,
                            JobContext::NONE,
                        )
                    },
                    boxed,
                )
            }
        }
    };
}
mesh_ffi!(DrDocument, dr_doc_puppet_mesh);
mesh_ffi!(PrecisionDocument, dr_precision_doc_puppet_mesh);

/// `{"vertices":[[x,y]],"triangles":[[a,b,c]],"rect":[x,y,w,h]}` in canvas
/// pixels. NULL `mesh` returns NULL with no error.
///
/// # Safety
/// `mesh` is NULL or live; `err_out` is NULL or writable. Free the string
/// with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_puppet_mesh_describe(
    mesh: *const PuppetMesh,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    if mesh.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "describing a puppet warp mesh",
            ptr::null_mut(),
            || json_string(required_handle(mesh)?.describe()),
            |s| s,
        )
    }
}

/// Solve `pins_json` (`{"mode":…,"pins":[…]}`, the request's fields) on
/// `mesh`. NULL `mesh` returns NULL with no error; a refusal (a pin outside
/// the mesh, two pins on one vertex, a bad field) returns NULL with its
/// sentence.
///
/// # Safety
/// `mesh` is NULL or live; `pins_json` is NULL or NUL terminated; `err_out`
/// is NULL or writable. Free the pose with dr_puppet_pose_free.
#[no_mangle]
pub unsafe extern "C" fn dr_puppet_mesh_solve(
    mesh: *const PuppetMesh,
    pins_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut PuppetPose {
    if mesh.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "solving a puppet warp",
            ptr::null_mut(),
            || {
                let mesh = required_handle(mesh)?;
                let text = borrow_cstr_bounded(pins_json, "pins", REQUEST_LIMIT)?;
                let value: serde_json::Value = serde_json::from_str(text)
                    .map_err(|e| format!("invalid puppet warp pins: {e}"))?;
                let object = value
                    .as_object()
                    .ok_or("puppet warp pins must be a JSON object")?;
                if let Some(key) = object
                    .keys()
                    .find(|k| !matches!(k.as_str(), "mode" | "pins"))
                {
                    return Err(format!("invalid puppet warp pins: unknown field `{key}`"));
                }
                let pins = crate::puppet_arap::pins(object.get("pins"))?;
                let mode = Mode::parse(object.get("mode"))?;
                solve(mesh, &pins, mode, JobContext::NONE)
            },
            boxed,
        )
    }
}

/// `{"positions":[x0,y0,…],"pins":[{"vertex":k,"from":[x,y]}],"rect":[x,y,w,h]}`.
/// NULL `pose` returns NULL with no error.
///
/// # Safety
/// `pose` is NULL or live; `err_out` is NULL or writable. Free the string
/// with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_puppet_pose_describe(
    pose: *const PuppetPose,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    if pose.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "describing a puppet warp pose",
            ptr::null_mut(),
            || json_string(required_handle(pose)?.describe()),
            |s| s,
        )
    }
}

/// The workspace preview: `proxy` (a picture of the canvas rect `sx, sy,
/// sw, sh` at any scale), optionally times `coverage` (proxy-sized bytes),
/// warped through `pose`. The canvas rect the answer covers is written to
/// `out_rect` (4 doubles: x, y, w, h). NULL `pose` or `proxy` returns NULL
/// with no error; an empty source, a coverage of another length or a NULL
/// `out_rect` returns NULL with a message.
///
/// # Safety
/// `pose` and `proxy` are NULL or live; `coverage` is NULL or valid for
/// `coverage_len` bytes; `out_rect` is NULL or valid for 4 doubles;
/// `err_out` is NULL or writable. Free the image with dr_image_free.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_puppet_pose_render(
    pose: *const PuppetPose,
    proxy: *const DrImage,
    sx: i64,
    sy: i64,
    sw: u32,
    sh: u32,
    coverage: *const u8,
    coverage_len: usize,
    out_rect: *mut f64,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if pose.is_null() || proxy.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "rendering a puppet warp preview",
            ptr::null_mut(),
            || {
                if out_rect.is_null() {
                    return Err("puppet warp preview needs an out_rect".into());
                }
                let pose = required_handle(pose)?;
                let proxy = &required_handle(proxy)?.pixels;
                let (pw, ph) = proxy.dimensions();
                let len = pw as usize * ph as usize;
                let coverage = if coverage.is_null() {
                    None
                } else if coverage_len != len {
                    return Err("puppet warp preview coverage must be proxy-sized".into());
                } else {
                    Some(std::slice::from_raw_parts(coverage, len))
                };
                let map = PoseMap::new(pose)?;
                let (image, rect) = render_preview(&map, proxy, (sx, sy, sw, sh), coverage)?;
                std::ptr::copy_nonoverlapping(rect.as_ptr(), out_rect, 4);
                Ok(DrImage { pixels: image })
            },
            boxed,
        )
    }
}

/// Frees a pose. NULL is ignored.
///
/// # Safety
/// `pose` is NULL or an unfreed pointer from dr_puppet_mesh_solve.
#[no_mangle]
pub unsafe extern "C" fn dr_puppet_pose_free(pose: *mut PuppetPose) {
    unsafe { free_handle(pose) }
}

/// Frees a mesh. NULL is ignored. Poses solved on it stay valid.
///
/// # Safety
/// `mesh` is NULL or an unfreed pointer from dr_doc_puppet_mesh or
/// dr_precision_doc_puppet_mesh.
#[no_mangle]
pub unsafe extern "C" fn dr_puppet_mesh_free(mesh: *mut PuppetMesh) {
    unsafe { free_handle(mesh) }
}

macro_rules! warp_ffi {
    ($doc:ty, $plan:ident, $edit:ident, $op:path) => {
        /// The plan of a `puppet_warp` request:
        /// `{"retained_bytes","scratch_bytes","rect":[x,y,w,h],"vertices"}`;
        /// an identity request answers `scratch_bytes` 0. NULL document
        /// returns NULL with no error; a refusal returns NULL with its
        /// sentence.
        ///
        /// # Safety
        /// `document` is NULL or live; `request` is NULL or NUL terminated;
        /// `err_out` is NULL or writable. Free the string with
        /// dr_string_free.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                fallible_op(
                    err_out,
                    "planning a puppet warp",
                    ptr::null_mut(),
                    || {
                        let request = PuppetWarpRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(
                            required_handle(document)?.puppet_warp_plan(&request, limit, other)?,
                        )
                    },
                    |s| s,
                )
            }
        }

        /// The edit of a `puppet_warp` request: a new document, or NULL with
        /// no error for an identity request. NULL document returns NULL with
        /// no error; a refusal returns NULL with its sentence.
        ///
        /// # Safety
        /// Handles are NULL or live for the call; `request` is NULL or NUL
        /// terminated; `err_out` is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $edit(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                $op(document, err_out, "editing a puppet warp", |doc: &$doc| {
                    let request = PuppetWarpRequest::parse(borrow_cstr_bounded(
                        request,
                        "request",
                        REQUEST_LIMIT,
                    )?)?;
                    doc.puppet_warp(&request, limit, other, control.as_ref())
                })
            }
        }
    };
}
warp_ffi!(
    DrDocument,
    dr_doc_puppet_warp_plan,
    dr_doc_puppet_warp,
    fallible_doc_op
);
warp_ffi!(
    PrecisionDocument,
    dr_precision_doc_puppet_warp_plan,
    dr_precision_doc_puppet_warp,
    precision_doc_op
);

/// [`fallible_doc_op`]'s typed twin for this file: NULL document yields
/// NULL, `Ok(None)` NULL with no message, `Err` its message.
///
/// # Safety
/// `doc` is NULL or live; `err_out` is NULL or writable.
unsafe fn precision_doc_op(
    doc: *const PrecisionDocument,
    err_out: *mut *mut c_char,
    what: &str,
    op: impl FnOnce(&PrecisionDocument) -> Result<Option<PrecisionDocument>, String>,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || op(required_handle(doc)?),
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
