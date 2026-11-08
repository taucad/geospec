//! The C ABI of `export/geospec_occt_facts_export.h` and the bridge calls the
//! adapter makes.

#![allow(unsafe_code, missing_docs)]

use std::ffi::c_char;

pub const OK: i32 = 0;

pub const SURFACE_PLANE: i32 = 0;
pub const SURFACE_CYLINDER: i32 = 1;
pub const SURFACE_CONE: i32 = 2;
pub const SURFACE_SPHERE: i32 = 3;
pub const SURFACE_TORUS: i32 = 4;
pub const SURFACE_NURBS: i32 = 5;

pub const TRIM_LINE: i32 = 0;
pub const TRIM_NURBS: i32 = 1;

pub const REFUSAL_UNSUPPORTED_SURFACE: i32 = 1;
pub const REFUSAL_UNSUPPORTED_TRIM: i32 = 2;

pub const FORWARD: i32 = 0;
pub const REVERSED: i32 = 1;
pub const INTERNAL: i32 = 2;

pub const RIGID: i32 = 0;
pub const MIRRORED: i32 = 1;

#[repr(C)]
pub struct Document {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Export {
    _private: [u8; 0],
}

#[repr(C)]
pub struct StringBuffer {
    pub data: *mut c_char,
    pub capacity: usize,
    pub length: usize,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Body {
    pub face_count: usize,
    pub shell_count: usize,
    pub closed: i32,
    pub placement: i32,
    pub box_void: i32,
    pub box_min: [f64; 3],
    pub box_max: [f64; 3],
    pub max_tolerance: f64,
    pub gprop_volume: f64,
    pub gprop_area: f64,
    pub gprop_centre: [f64; 3],
    pub gprop_ms: f64,
}

#[repr(C)]
pub struct Surface {
    pub kind: i32,
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
    pub radius: f64,
    pub minor_radius: f64,
    pub semi_angle: f64,
    pub degree_u: usize,
    pub degree_v: usize,
    pub count_u: usize,
    pub count_v: usize,
    pub knots_u: *const f64,
    pub knot_count_u: usize,
    pub knots_v: *const f64,
    pub knot_count_v: usize,
    pub poles: *const f64,
    pub weights: *const f64,
}

#[repr(C)]
pub struct Trim {
    pub kind: i32,
    pub source: i32,
    pub reparameterised: i32,
    pub reversed: i32,
    pub seam: i32,
    pub degenerate: i32,
    pub gap: i32,
    pub origin: [f64; 2],
    pub direction: [f64; 2],
    pub degree: usize,
    pub knots: *const f64,
    pub knot_count: usize,
    pub poles: *const f64,
    pub pole_count: usize,
    pub weights: *const f64,
    pub range: [f64; 2],
}

#[repr(C)]
pub struct Face {
    pub refusal: i32,
    pub source: i32,
    pub orientation: i32,
    pub reflected: i32,
    pub moved_origin: i32,
    pub shift: [f64; 2],
    pub surface: Surface,
    pub trims: *const Trim,
    pub trim_count: usize,
    pub loops: *const usize,
    pub loop_count: usize,
}

extern "C" {
    pub fn geospec_occt_open_step(
        bytes: *const u8,
        length: usize,
        out_document: *mut *mut Document,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_release(document: *mut Document);

    pub fn geospec_occt_facts_export_new(
        document: *const Document,
        out_export: *mut *mut Export,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_export_synthetic(
        kind: i32,
        out_export: *mut *mut Export,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_export_release(export: *mut Export);
    pub fn geospec_occt_facts_body_count(export: *const Export) -> usize;
    pub fn geospec_occt_facts_free_face_count(export: *const Export) -> usize;
    pub fn geospec_occt_facts_shape(
        export: *const Export,
        out_volume: *mut f64,
        out_area: *mut f64,
        out_centre: *mut f64,
        out_ms: *mut f64,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_body(
        export: *mut Export,
        body: usize,
        out_body: *mut Body,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_body_eps(
        export: *const Export,
        body: usize,
        eps: f64,
        out: *mut f64,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_face(
        export: *mut Export,
        body: usize,
        face: usize,
        out_face: *mut Face,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_surface_d1(
        export: *const Export,
        u: f64,
        v: f64,
        out: *mut f64,
        error: *mut StringBuffer,
    ) -> i32;
    pub fn geospec_occt_facts_trim_d1(
        export: *const Export,
        trim: usize,
        t: f64,
        out: *mut f64,
        error: *mut StringBuffer,
    ) -> i32;
}

/// A message buffer for one call.
pub struct ErrorBuffer {
    bytes: Vec<u8>,
    raw: StringBuffer,
}

impl ErrorBuffer {
    pub fn new() -> Self {
        let mut bytes = vec![0_u8; 1024];
        let raw = StringBuffer {
            data: bytes.as_mut_ptr().cast(),
            capacity: bytes.len(),
            length: 0,
        };
        Self { bytes, raw }
    }

    pub fn raw(&mut self) -> *mut StringBuffer {
        self.raw.length = 0;
        &mut self.raw
    }

    pub fn message(&self) -> String {
        let n = self.raw.length.min(self.bytes.len());
        String::from_utf8_lossy(self.bytes.get(..n).unwrap_or(&[])).into_owned()
    }
}

impl Default for ErrorBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// `count` values from `data`, or none for a null pointer.
///
/// # Safety
///
/// `data` is null or points to `count` initialised values that outlive the
/// returned slice.
pub unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the caller guarantees `count` values at `data`.
        unsafe { std::slice::from_raw_parts(data, count) }
    }
}
