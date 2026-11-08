// The facts layer's OCCT export: each face of an admitted OCCT B-rep as the
// numbers of a geospec-facts `FaceDefinition` (surface kind and parameters or
// NURBS net, oriented 2D trims with their ranges, loops, orientation), with
// the original geometry's evaluation for the adapter's export check.
//
// The export is compiled only by the facts crate's OCCT adapter
// (`native/facts/occt`), in one translation unit with the bridge, so that it
// reads the shape the engine's own STEP admission built. No product path
// links it. Every pointer an export call returns stays valid until the next
// call on the same export; an export is used by one thread at a time.

#ifndef GEOSPEC_OCCT_FACTS_EXPORT_H
#define GEOSPEC_OCCT_FACTS_EXPORT_H

#include "geospec_occt_bridge.h"

#ifdef __cplusplus
#define GEOSPEC_OCCT_FACTS_NOEXCEPT noexcept
#else
#define GEOSPEC_OCCT_FACTS_NOEXCEPT
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct geospec_occt_facts_export geospec_occt_facts_export;

// The definition's surface kinds, in geospec-facts' (OCCT's) parameterisations.
enum geospec_occt_facts_surface_kind {
  GEOSPEC_OCCT_FACTS_PLANE = 0,
  GEOSPEC_OCCT_FACTS_CYLINDER = 1,
  GEOSPEC_OCCT_FACTS_CONE = 2,
  GEOSPEC_OCCT_FACTS_SPHERE = 3,
  GEOSPEC_OCCT_FACTS_TORUS = 4,
  GEOSPEC_OCCT_FACTS_NURBS = 5
};

// The OCCT surface a face carried before export (diagnostic).
enum geospec_occt_facts_source_surface {
  GEOSPEC_OCCT_FACTS_SOURCE_PLANE = 0,
  GEOSPEC_OCCT_FACTS_SOURCE_CYLINDER = 1,
  GEOSPEC_OCCT_FACTS_SOURCE_CONE = 2,
  GEOSPEC_OCCT_FACTS_SOURCE_SPHERE = 3,
  GEOSPEC_OCCT_FACTS_SOURCE_TORUS = 4,
  GEOSPEC_OCCT_FACTS_SOURCE_BSPLINE = 5,
  GEOSPEC_OCCT_FACTS_SOURCE_PERIODIC_BSPLINE = 6,
  GEOSPEC_OCCT_FACTS_SOURCE_BEZIER = 7,
  GEOSPEC_OCCT_FACTS_SOURCE_REVOLUTION = 8,
  GEOSPEC_OCCT_FACTS_SOURCE_EXTRUSION = 9,
  GEOSPEC_OCCT_FACTS_SOURCE_OFFSET = 10,
  GEOSPEC_OCCT_FACTS_SOURCE_OTHER = 11
};

enum geospec_occt_facts_trim_kind {
  GEOSPEC_OCCT_FACTS_TRIM_LINE = 0,
  GEOSPEC_OCCT_FACTS_TRIM_NURBS = 1
};

// The OCCT 2D curve an edge use carried before export (diagnostic).
enum geospec_occt_facts_source_curve {
  GEOSPEC_OCCT_FACTS_CURVE_LINE = 0,
  GEOSPEC_OCCT_FACTS_CURVE_CIRCLE = 1,
  GEOSPEC_OCCT_FACTS_CURVE_ELLIPSE = 2,
  GEOSPEC_OCCT_FACTS_CURVE_HYPERBOLA = 3,
  GEOSPEC_OCCT_FACTS_CURVE_PARABOLA = 4,
  GEOSPEC_OCCT_FACTS_CURVE_BSPLINE = 5,
  GEOSPEC_OCCT_FACTS_CURVE_PERIODIC_BSPLINE = 6,
  GEOSPEC_OCCT_FACTS_CURVE_BEZIER = 7,
  GEOSPEC_OCCT_FACTS_CURVE_OFFSET = 8,
  GEOSPEC_OCCT_FACTS_CURVE_OTHER = 9
};

// Why a face has no definition; the detail is in the call's error string.
enum geospec_occt_facts_refusal {
  GEOSPEC_OCCT_FACTS_DEFINED = 0,
  GEOSPEC_OCCT_FACTS_UNSUPPORTED_SURFACE = 1,
  GEOSPEC_OCCT_FACTS_UNSUPPORTED_TRIM = 2,
  GEOSPEC_OCCT_FACTS_INVALID_DEFINITION = 3
};

// TopAbs orientation of the face in its solid.
enum geospec_occt_facts_orientation {
  GEOSPEC_OCCT_FACTS_FORWARD = 0,
  GEOSPEC_OCCT_FACTS_REVERSED = 1,
  GEOSPEC_OCCT_FACTS_INTERNAL = 2,
  GEOSPEC_OCCT_FACTS_EXTERNAL = 3
};

// The solid's placement relative to its definition.
enum geospec_occt_facts_placement {
  GEOSPEC_OCCT_FACTS_RIGID = 0,
  GEOSPEC_OCCT_FACTS_MIRRORED = 1,
  GEOSPEC_OCCT_FACTS_SCALED = 2
};

// One body: a solid of the shape, in TopExp_Explorer order.
typedef struct geospec_occt_facts_body_record {
  size_t face_count;
  size_t shell_count;
  // Every shell closed by edge-use parity (BRep_Tool::IsClosed).
  int closed;
  int placement;
  // BRepBndLib::Add (no triangulation): a box containing the solid.
  int box_void;
  double box_min[3];
  double box_max[3];
  // The largest vertex, edge or face tolerance.
  double max_tolerance;
  // BRepGProp without Eps on this solid, as the bridge measures solids:
  // VolumeProperties (volume, centre of mass) and SurfaceProperties (area).
  double gprop_volume;
  double gprop_area;
  double gprop_centre[3];
  double gprop_ms;
} geospec_occt_facts_body_record;

// A surface. Plane: origin, x = S_u, y = S_v. Kinds of revolution: the frame
// (origin, x, y, z right-handed), radius (the major radius of a torus),
// minor_radius (torus) and semi_angle (cone). NURBS: degrees, pole counts,
// full knot vectors, poles (x, y, z) in u-major order, weights or null.
typedef struct geospec_occt_facts_surface {
  int kind;
  double origin[3];
  double x[3];
  double y[3];
  double z[3];
  double radius;
  double minor_radius;
  double semi_angle;
  size_t degree_u;
  size_t degree_v;
  size_t count_u;
  size_t count_v;
  const double* knots_u;
  size_t knot_count_u;
  const double* knots_v;
  size_t knot_count_v;
  const double* poles;
  const double* weights;
} geospec_occt_facts_surface;

// One trim piece: a line (origin, direction) or a NURBS curve (degree, full
// knot vector, poles (u, v), weights or null) over `range`, traversed from
// range[1] to range[0] when `reversed`. `reparameterised`: the piece is an
// exact conversion (a conic as a rational B-spline) whose parameter is not
// the edge's; the check then compares positions only.
typedef struct geospec_occt_facts_trim {
  int kind;
  int source;
  int reparameterised;
  int reversed;
  int seam;
  int degenerate;
  // A line the export closes a tolerance gap with (see the source).
  int gap;
  double origin[2];
  double direction[2];
  size_t degree;
  const double* knots;
  size_t knot_count;
  const double* poles;
  size_t pole_count;
  const double* weights;
  double range[2];
} geospec_occt_facts_trim;

// One face. `refusal` is nonzero when the face has no definition. The trims
// follow each wire in BRepTools_WireExplorer order on the forward face, so
// that the face lies on their left; `orientation` is the face's in its solid.
// `reflected`: an indirect frame of revolution exported as the direct frame
// (x, -y, z) with u negated in every trim and every loop reversed.
// `shift`: the whole periods added to every trim's u and v so that a periodic
// net's domain holds them.
typedef struct geospec_occt_facts_face_record {
  int refusal;
  int source;
  int orientation;
  int reflected;
  // A periodic net's origin moved (by exact knot insertion) to hold the trims.
  int moved_origin;
  double shift[2];
  geospec_occt_facts_surface surface;
  const geospec_occt_facts_trim* trims;
  size_t trim_count;
  const size_t* loops;
  size_t loop_count;
} geospec_occt_facts_face_record;

// An export of the document's admitted shape. The document must outlive it.
int geospec_occt_facts_export_new(const geospec_occt_document* document,
                                  geospec_occt_facts_export** out_export,
                                  geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
// A synthetic shape the adapter's tests build (see the source for the kinds).
int geospec_occt_facts_export_synthetic(int kind, geospec_occt_facts_export** out_export,
                                        geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
void geospec_occt_facts_export_release(geospec_occt_facts_export* facts_export)
    GEOSPEC_OCCT_FACTS_NOEXCEPT;

size_t geospec_occt_facts_body_count(const geospec_occt_facts_export* facts_export)
    GEOSPEC_OCCT_FACTS_NOEXCEPT;
// Faces of the shape outside every solid (not exported).
size_t geospec_occt_facts_free_face_count(const geospec_occt_facts_export* facts_export)
    GEOSPEC_OCCT_FACTS_NOEXCEPT;
// The bridge's whole-shape volume, centre of mass and area of a document
// export (its source shape facts); BRepGProp's whole-shape calls on a
// synthetic one.
int geospec_occt_facts_shape(const geospec_occt_facts_export* facts_export,
                             double* out_volume, double* out_area, double* out_centre,
                             double* out_ms, geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
int geospec_occt_facts_body(geospec_occt_facts_export* facts_export, size_t body,
                            geospec_occt_facts_body_record* out_body,
                            geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
// BRepGProp's adaptive measurement of solid `body` at relative accuracy
// `eps` (OnlyClosed and SkipShared false): volume, area, centre (3), the
// volume's and area's error estimates and the milliseconds (8 values).
int geospec_occt_facts_body_eps(const geospec_occt_facts_export* facts_export, size_t body,
                                double eps, double* out,
                                geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
// Face `face` of body `body`; it becomes the export's current face. Requires
// a prior geospec_occt_facts_body call for `body`, which collects the body's
// faces: without it the call fails with GEOSPEC_OCCT_INVALID_ARGUMENT. A face
// whose joins need a tolerance gap carries those joins as trims flagged `gap`
// (the straight join in the parameter plane); the adapter refuses such a face.
// A refused face (`refusal` nonzero) still reports its surface's `source` and,
// for coverage only, the edge uses read before the refusal (including the
// refused one) as `trims`, with their kinds and flags but no knots, poles or
// loops: it has no definition to integrate.
int geospec_occt_facts_face(geospec_occt_facts_export* facts_export, size_t body,
                            size_t face, geospec_occt_facts_face_record* out_face,
                            geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
// The current face's original surface at the exported (u, v): the point,
// S_u and S_v in the exported parameters (9 values).
int geospec_occt_facts_surface_d1(const geospec_occt_facts_export* facts_export,
                                  double u, double v, double* out,
                                  geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;
// Trim `trim` of the current face at its exported parameter `t`, from the
// edge's original 2D curve, in the exported (u, v): the point, the
// derivative (NaN when reparameterised) and, when reparameterised, the
// distance from the conversion's point to the original curve (5 values).
int geospec_occt_facts_trim_d1(const geospec_occt_facts_export* facts_export,
                               size_t trim, double t, double* out,
                               geospec_occt_string* error) GEOSPEC_OCCT_FACTS_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#endif
