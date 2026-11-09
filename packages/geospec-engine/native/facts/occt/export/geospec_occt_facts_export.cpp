// The facts layer's OCCT export (see geospec_occt_facts_export.h).
//
// One translation unit with the bridge: the facts crate's OCCT adapter
// compiles this file, which includes the bridge's source, so that the export
// reads the shape the engine's STEP admission built (read profile, authored
// fixes, threaded admission) and measures the bridge's own whole-shape facts.
// The bridge's source is not changed and no product links this file.
//
// Per face, on the face oriented FORWARD (so that the material lies on the
// left of every wire, OCCT's convention):
// - the surface through BRep_Tool::Surface (located), rectangular trims
//   unwrapped; planes, cylinders, cones, spheres and tori pass through in
//   OCCT's parameterisation (an indirect frame as the direct frame (x, -y, z)
//   with u negated); B-spline nets as they are, Bezier nets as B-splines; a
//   periodic net is made non-periodic with its origin moved, by exact knot
//   insertion, so that its one-period domain holds the face's trims, which are
//   shifted by whole periods to match; every other kind has no export;
// - per wire, in BRepTools_WireExplorer order, each edge use's 2D curve from
//   BRep_Tool::CurveOnSurface over the edge's range (so seams take their
//   orientation's pcurve): lines as lines, B-splines as they are (a periodic
//   one with its origin at the range's start), Bezier curves as B-splines, and
//   conics as OCCT's exact rational B-splines over their own parameter
//   (`reparameterised`); offset curves have no export.
// The adapter checks every export against the evaluation functions here, which
// evaluate the original OCCT geometry.

#include "../../../occt/bridge/geospec_occt_bridge.cpp"

#include "geospec_occt_facts_export.h"

#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_NurbsConvert.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepGProp.hxx>
#include <BRepLib.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRepTopAdaptor_FClass2d.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <Bnd_Box2d.hxx>
#include <BndLib_Add2dCurve.hxx>
#include <ElCLib.hxx>
#include <GProp_GProps.hxx>
#include <Geom2dAPI_ProjectPointOnCurve.hxx>
#include <Geom2dConvert.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2d_BezierCurve.hxx>
#include <Geom2d_Circle.hxx>
#include <Geom2d_Ellipse.hxx>
#include <Geom2d_Hyperbola.hxx>
#include <Geom2d_Line.hxx>
#include <Geom2d_OffsetCurve.hxx>
#include <Geom2d_Parabola.hxx>
#include <Geom2d_TrimmedCurve.hxx>
#include <GeomConvert.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_Circle.hxx>
#include <Geom_BezierSurface.hxx>
#include <Geom_ConicalSurface.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom_OffsetSurface.hxx>
#include <Geom_Plane.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <Geom_SphericalSurface.hxx>
#include <Geom_SurfaceOfLinearExtrusion.hxx>
#include <Geom_SurfaceOfRevolution.hxx>
#include <Geom_ToroidalSurface.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Shell.hxx>
#include <TopoDS_Solid.hxx>
#include <gp_Ax3.hxx>
#include <gp_Pln.hxx>
#include <gp_Trsf.hxx>

#include <chrono>
#include <cmath>
#include <initializer_list>
#include <limits>
#include <string>
#include <utility>
#include <vector>

namespace facts_export {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// A refusal raised while exporting one face.
struct Refused {
  int code;
  std::string detail;
};

struct Trim {
  // The edge's 2D curve, trimmed curves unwrapped (its parameter is the
  // edge's).
  occ::handle<Geom2d_Curve> original;
  // The exported curve when it is a B-spline.
  occ::handle<Geom2d_BSplineCurve> nurbs;
  std::vector<double> knots;
  std::vector<double> poles;
  std::vector<double> weights;
  geospec_occt_facts_trim record{};
};

struct Face {
  occ::handle<Geom_Surface> original;
  // -1 when the frame was reflected (u negated), else 1.
  double sign = 1.0;
  bool moved = false;
  double shift[2] = {0.0, 0.0};
  std::vector<double> knots_u;
  std::vector<double> knots_v;
  std::vector<double> poles;
  std::vector<double> weights;
  std::vector<Trim> trims;
  std::vector<geospec_occt_facts_trim> records;
  std::vector<size_t> loops;
};

void put3(double out[3], const gp_XYZ& p) {
  out[0] = p.X();
  out[1] = p.Y();
  out[2] = p.Z();
}

bool rigid(const TopLoc_Location& location, int* placement) {
  const gp_Trsf t = location.Transformation();
  if (std::abs(std::abs(t.ScaleFactor()) - 1.0) > TopLoc_Location::ScalePrec()) {
    *placement = GEOSPEC_OCCT_FACTS_SCALED;
    return false;
  }
  if (t.IsNegative()) {
    *placement = GEOSPEC_OCCT_FACTS_MIRRORED;
    return false;
  }
  *placement = GEOSPEC_OCCT_FACTS_RIGID;
  return true;
}

int orientation_of(TopAbs_Orientation o) {
  switch (o) {
    case TopAbs_FORWARD:
      return GEOSPEC_OCCT_FACTS_FORWARD;
    case TopAbs_REVERSED:
      return GEOSPEC_OCCT_FACTS_REVERSED;
    case TopAbs_INTERNAL:
      return GEOSPEC_OCCT_FACTS_INTERNAL;
    default:
      return GEOSPEC_OCCT_FACTS_EXTERNAL;
  }
}

// A frame for the kinds of revolution: direct as it is, or reflected.
void frame(const gp_Ax3& position, geospec_occt_facts_surface& s, Face& face) {
  put3(s.origin, position.Location().XYZ());
  put3(s.x, position.XDirection().XYZ());
  put3(s.z, position.Direction().XYZ());
  gp_XYZ y = position.YDirection().XYZ();
  if (!position.Direct()) {
    y.Reverse();
    face.sign = -1.0;
  }
  put3(s.y, y);
}

// The knots of `count` poles of degree `degree` as one flat vector.
template <typename Knots, typename Mults>
std::vector<double> flat(const Knots& knots, const Mults& mults) {
  std::vector<double> out;
  for (int i = knots.Lower(); i <= knots.Upper(); ++i) {
    for (int m = 0; m < mults.Value(i); ++m) out.push_back(knots.Value(i));
  }
  return out;
}

// Inserts `u` as a knot of the periodic net in direction `dir` (0: u) and
// makes it the origin; returns false when OCCT cannot.
bool move_origin(Geom_BSplineSurface& b, int dir, double u) {
  if (dir == 0) {
    b.InsertUKnot(u, 1, 0.0, true);
    for (int i = 1; i <= b.NbUKnots(); ++i) {
      if (b.UKnot(i) == u) {
        b.SetUOrigin(i);
        return true;
      }
    }
  } else {
    b.InsertVKnot(u, 1, 0.0, true);
    for (int i = 1; i <= b.NbVKnots(); ++i) {
      if (b.VKnot(i) == u) {
        b.SetVOrigin(i);
        return true;
      }
    }
  }
  return false;
}

// The 2D curve of one edge use as a trim; `original` and the range are set.
void export_trim(Trim& t, double first, double last) {
  geospec_occt_facts_trim& r = t.record;
  occ::handle<Geom2d_Curve> c = t.original;
  r.range[0] = first;
  r.range[1] = last;
  if (auto line = occ::down_cast<Geom2d_Line>(c)) {
    r.kind = GEOSPEC_OCCT_FACTS_TRIM_LINE;
    r.source = GEOSPEC_OCCT_FACTS_CURVE_LINE;
    r.origin[0] = line->Location().X();
    r.origin[1] = line->Location().Y();
    r.direction[0] = line->Direction().X();
    r.direction[1] = line->Direction().Y();
    return;
  }
  occ::handle<Geom2d_BSplineCurve> b;
  if (auto bspline = occ::down_cast<Geom2d_BSplineCurve>(c)) {
    b = occ::down_cast<Geom2d_BSplineCurve>(bspline->Copy());
    r.source = b->IsPeriodic() ? GEOSPEC_OCCT_FACTS_CURVE_PERIODIC_BSPLINE
                               : GEOSPEC_OCCT_FACTS_CURVE_BSPLINE;
    if (b->IsPeriodic()) {
      const double lo = b->FirstParameter();
      const double period = b->LastParameter() - lo;
      const double slack = 1e-12 * period;
      if (first < lo - slack || last > lo + period + slack) {
        // The range's start as the origin: the domain [f0, f0 + period]
        // then holds the range, moved by whole periods (the same points).
        const double f0 = ElCLib::InPeriod(first, lo, lo + period);
        b->InsertKnot(f0, 1, 0.0);
        bool moved = false;
        for (int i = 1; i <= b->NbKnots() && !moved; ++i) {
          if (b->Knot(i) == f0) {
            b->SetOrigin(i);
            moved = true;
          }
        }
        if (!moved) throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION,
                                  "cannot move a periodic trim's origin"};
        r.range[0] = f0;
        r.range[1] = f0 + (last - first);
      }
      b->SetNotPeriodic();
    }
  } else if (auto bezier = occ::down_cast<Geom2d_BezierCurve>(c)) {
    r.source = GEOSPEC_OCCT_FACTS_CURVE_BEZIER;
    b = Geom2dConvert::CurveToBSplineCurve(bezier);
  } else if (occ::down_cast<Geom2d_Circle>(c) || occ::down_cast<Geom2d_Ellipse>(c) ||
             occ::down_cast<Geom2d_Hyperbola>(c) || occ::down_cast<Geom2d_Parabola>(c)) {
    r.source = occ::down_cast<Geom2d_Circle>(c)     ? GEOSPEC_OCCT_FACTS_CURVE_CIRCLE
               : occ::down_cast<Geom2d_Ellipse>(c)  ? GEOSPEC_OCCT_FACTS_CURVE_ELLIPSE
               : occ::down_cast<Geom2d_Hyperbola>(c) ? GEOSPEC_OCCT_FACTS_CURVE_HYPERBOLA
                                                    : GEOSPEC_OCCT_FACTS_CURVE_PARABOLA;
    occ::handle<Geom2d_TrimmedCurve> arc = new Geom2d_TrimmedCurve(c, first, last);
    b = Geom2dConvert::CurveToBSplineCurve(arc, Convert_TgtThetaOver2);
    if (b.IsNull()) throw Refused{GEOSPEC_OCCT_FACTS_UNSUPPORTED_TRIM, "conic conversion failed"};
    if (b->IsPeriodic()) b->SetNotPeriodic();
    r.reparameterised = 1;
    r.range[0] = b->FirstParameter();
    r.range[1] = b->LastParameter();
  } else if (occ::down_cast<Geom2d_OffsetCurve>(c)) {
    r.source = GEOSPEC_OCCT_FACTS_CURVE_OFFSET;
    throw Refused{GEOSPEC_OCCT_FACTS_UNSUPPORTED_TRIM, "offset trim curve"};
  } else {
    r.source = GEOSPEC_OCCT_FACTS_CURVE_OTHER;
    throw Refused{GEOSPEC_OCCT_FACTS_UNSUPPORTED_TRIM,
                  std::string("trim curve ") + c->DynamicType()->Name()};
  }
  t.nurbs = b;
  r.kind = GEOSPEC_OCCT_FACTS_TRIM_NURBS;
  r.degree = static_cast<size_t>(b->Degree());
  t.knots = flat(b->Knots(), b->Multiplicities());
  for (int i = 1; i <= b->NbPoles(); ++i) {
    t.poles.push_back(b->Pole(i).X());
    t.poles.push_back(b->Pole(i).Y());
  }
  if (b->IsRational()) {
    for (int i = 1; i <= b->NbPoles(); ++i) t.weights.push_back(b->Weight(i));
  }
}

// The oriented ends of a trim in the original parameter plane.
std::pair<gp_Pnt2d, gp_Pnt2d> ends(const Trim& t, double first, double last) {
  const gp_Pnt2d a = t.original->Value(first);
  const gp_Pnt2d b = t.original->Value(last);
  return t.record.reversed ? std::make_pair(b, a) : std::make_pair(a, b);
}

// Closes the tolerance gaps of one wire's trims (`trims[start..]`, with their
// edges and ranges): where a trim ends away from where the next starts by more
// than the integrator's chaining tolerance (a relative 1e-6), not by a whole
// period of the surface, and both ends lie on the surface within the shared
// vertex's tolerance of the vertex, a line from the one to the other is
// inserted, flagged `gap`. OCCT's model joins the two edges at that vertex
// within its tolerance; the line is the straight join in the parameter plane,
// and the adapter refuses the face on it (tolerance gap), since the join
// changes the face by an amount no integration bound covers. Ends a whole
// period apart (within the chaining tolerance) are a period wrap and get no
// line. Any other gap stays, and the integrator refuses it (open trim loop).
void fill_gaps(std::vector<Trim>& trims, size_t start, const std::vector<TopoDS_Edge>& edges,
               const std::vector<std::pair<double, double>>& ranges,
               const occ::handle<Geom_Surface>& surface) {
  const size_t n = trims.size() - start;
  if (n == 0) return;
  std::vector<std::pair<gp_Pnt2d, gp_Pnt2d>> points;
  for (size_t i = 0; i < n; ++i) {
    points.push_back(ends(trims[start + i], ranges[i].first, ranges[i].second));
  }
  std::vector<Trim> out;
  for (size_t i = 0; i < n; ++i) {
    const gp_Pnt2d p = points[i].second;
    const gp_Pnt2d q = points[(i + 1) % n].first;
    out.push_back(std::move(trims[start + i]));
    const double scale = 1.0 + std::max({std::abs(p.X()), std::abs(p.Y()), std::abs(q.X()),
                                         std::abs(q.Y())});
    const gp_Vec2d d(p, q);
    if (d.Magnitude() <= 1e-6 * scale) continue;
    bool wrap = false;
    for (int dir = 0; dir < 2; ++dir) {
      const bool periodic = dir == 0 ? surface->IsUPeriodic() : surface->IsVPeriodic();
      if (!periodic) continue;
      const double period = dir == 0 ? surface->UPeriod() : surface->VPeriod();
      const double along = dir == 0 ? d.X() : d.Y();
      const double other = dir == 0 ? d.Y() : d.X();
      if (std::abs(std::abs(along) - period) <= 1e-6 * scale && std::abs(other) <= 1e-6 * scale) {
        wrap = true;
      }
    }
    if (wrap) continue;
    const TopoDS_Vertex v = TopExp::LastVertex(edges[i], true);
    if (v.IsNull()) continue;
    const gp_Pnt at = BRep_Tool::Pnt(v);
    const double tolerance = BRep_Tool::Tolerance(v);
    if (surface->Value(p.X(), p.Y()).Distance(at) > tolerance ||
        surface->Value(q.X(), q.Y()).Distance(at) > tolerance) {
      continue;
    }
    Trim line;
    line.original = new Geom2d_Line(p, gp_Dir2d(d));
    line.record.gap = 1;
    export_trim(line, 0.0, d.Magnitude());
    out.push_back(std::move(line));
  }
  trims.resize(start);
  for (Trim& t : out) trims.push_back(std::move(t));
}

// Moves the trim's exported numbers by u' = sign u + shift[0], v' = v + shift[1].
void map_trim(Trim& t, double sign, const double shift[2]) {
  geospec_occt_facts_trim& r = t.record;
  if (r.kind == GEOSPEC_OCCT_FACTS_TRIM_LINE) {
    r.origin[0] = sign * r.origin[0] + shift[0];
    r.origin[1] = r.origin[1] + shift[1];
    r.direction[0] = sign * r.direction[0];
    return;
  }
  for (size_t i = 0; i + 1 < t.poles.size(); i += 2) {
    t.poles[i] = sign * t.poles[i] + shift[0];
    t.poles[i + 1] = t.poles[i + 1] + shift[1];
  }
}

// The face's surface as a definition, after its trims are read (a periodic
// net moves its origin to hold them).
// The source kind of a face's surface (trimmed surfaces by their basis), for
// coverage: known before the trims are read, so that a face refused on a
// trim is counted by its own surface kind.
int surface_source(occ::handle<Geom_Surface> g) {
  while (auto trimmed = occ::down_cast<Geom_RectangularTrimmedSurface>(g)) {
    g = trimmed->BasisSurface();
  }
  if (g.IsNull()) return GEOSPEC_OCCT_FACTS_SOURCE_OTHER;
  if (occ::down_cast<Geom_Plane>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_PLANE;
  if (occ::down_cast<Geom_CylindricalSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_CYLINDER;
  if (occ::down_cast<Geom_ConicalSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_CONE;
  if (occ::down_cast<Geom_SphericalSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_SPHERE;
  if (occ::down_cast<Geom_ToroidalSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_TORUS;
  if (auto b = occ::down_cast<Geom_BSplineSurface>(g)) {
    return (b->IsUPeriodic() || b->IsVPeriodic()) ? GEOSPEC_OCCT_FACTS_SOURCE_PERIODIC_BSPLINE
                                                  : GEOSPEC_OCCT_FACTS_SOURCE_BSPLINE;
  }
  if (occ::down_cast<Geom_BezierSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_BEZIER;
  if (occ::down_cast<Geom_SurfaceOfRevolution>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_REVOLUTION;
  if (occ::down_cast<Geom_SurfaceOfLinearExtrusion>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_EXTRUSION;
  if (occ::down_cast<Geom_OffsetSurface>(g)) return GEOSPEC_OCCT_FACTS_SOURCE_OFFSET;
  return GEOSPEC_OCCT_FACTS_SOURCE_OTHER;
}

void export_surface(Face& face, const occ::handle<Geom_Surface>& located,
                    geospec_occt_facts_surface& s, int& source,
                    const double extent[2][2]) {
  occ::handle<Geom_Surface> g = located;
  while (auto trimmed = occ::down_cast<Geom_RectangularTrimmedSurface>(g)) {
    g = trimmed->BasisSurface();
  }
  face.original = g;
  occ::handle<Geom_BSplineSurface> b;
  if (auto plane = occ::down_cast<Geom_Plane>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_PLANE;
    s.kind = GEOSPEC_OCCT_FACTS_PLANE;
    const gp_Ax3& p = plane->Position();
    put3(s.origin, p.Location().XYZ());
    put3(s.x, p.XDirection().XYZ());
    put3(s.y, p.YDirection().XYZ());
    put3(s.z, p.Direction().XYZ());
    return;
  }
  if (auto cylinder = occ::down_cast<Geom_CylindricalSurface>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_CYLINDER;
    s.kind = GEOSPEC_OCCT_FACTS_CYLINDER;
    frame(cylinder->Position(), s, face);
    s.radius = cylinder->Radius();
    return;
  }
  if (auto cone = occ::down_cast<Geom_ConicalSurface>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_CONE;
    s.kind = GEOSPEC_OCCT_FACTS_CONE;
    frame(cone->Position(), s, face);
    s.radius = cone->RefRadius();
    s.semi_angle = cone->SemiAngle();
    return;
  }
  if (auto sphere = occ::down_cast<Geom_SphericalSurface>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_SPHERE;
    s.kind = GEOSPEC_OCCT_FACTS_SPHERE;
    frame(sphere->Position(), s, face);
    s.radius = sphere->Radius();
    return;
  }
  if (auto torus = occ::down_cast<Geom_ToroidalSurface>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_TORUS;
    s.kind = GEOSPEC_OCCT_FACTS_TORUS;
    frame(torus->Position(), s, face);
    s.radius = torus->MajorRadius();
    s.minor_radius = torus->MinorRadius();
    return;
  }
  if (auto bspline = occ::down_cast<Geom_BSplineSurface>(g)) {
    b = occ::down_cast<Geom_BSplineSurface>(bspline->Copy());
    source = (b->IsUPeriodic() || b->IsVPeriodic()) ? GEOSPEC_OCCT_FACTS_SOURCE_PERIODIC_BSPLINE
                                                    : GEOSPEC_OCCT_FACTS_SOURCE_BSPLINE;
  } else if (auto bezier = occ::down_cast<Geom_BezierSurface>(g)) {
    source = GEOSPEC_OCCT_FACTS_SOURCE_BEZIER;
    b = GeomConvert::SurfaceToBSplineSurface(bezier);
  } else {
    source = occ::down_cast<Geom_SurfaceOfRevolution>(g)       ? GEOSPEC_OCCT_FACTS_SOURCE_REVOLUTION
             : occ::down_cast<Geom_SurfaceOfLinearExtrusion>(g) ? GEOSPEC_OCCT_FACTS_SOURCE_EXTRUSION
             : occ::down_cast<Geom_OffsetSurface>(g)            ? GEOSPEC_OCCT_FACTS_SOURCE_OFFSET
                                                                : GEOSPEC_OCCT_FACTS_SOURCE_OTHER;
    throw Refused{GEOSPEC_OCCT_FACTS_UNSUPPORTED_SURFACE,
                  std::string("surface ") + g->DynamicType()->Name()};
  }
  // A periodic direction whose domain does not hold the trims: the origin
  // moves to where it does (the trims' extent centred in one period), and the
  // trims move by the whole periods between.
  for (int dir = 0; dir < 2; ++dir) {
    const bool periodic = dir == 0 ? b->IsUPeriodic() : b->IsVPeriodic();
    if (!periodic) continue;
    double u1, u2, v1, v2;
    b->Bounds(u1, u2, v1, v2);
    const double lo = dir == 0 ? u1 : v1;
    const double period = dir == 0 ? u2 - u1 : v2 - v1;
    const double slack = 1e-12 * period;
    const double a = extent[dir][0];
    const double z = extent[dir][1];
    if (a >= lo - slack && z <= lo + period + slack) continue;
    if (!(z - a <= period + slack)) continue;  // wider than a period: the integrator refuses
    const double origin = a - 0.5 * std::max(0.0, period - (z - a));
    const double turns = std::floor((origin - lo) / period);
    const double moved = origin - turns * period;
    if (!move_origin(*b, dir, moved)) {
      throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION, "cannot move a periodic net's origin"};
    }
    face.shift[dir] = -turns * period;
    face.moved = true;
  }
  if (b->IsUPeriodic()) b->SetUNotPeriodic();
  if (b->IsVPeriodic()) b->SetVNotPeriodic();
  s.kind = GEOSPEC_OCCT_FACTS_NURBS;
  s.degree_u = static_cast<size_t>(b->UDegree());
  s.degree_v = static_cast<size_t>(b->VDegree());
  s.count_u = static_cast<size_t>(b->NbUPoles());
  s.count_v = static_cast<size_t>(b->NbVPoles());
  face.knots_u = flat(b->UKnots(), b->UMultiplicities());
  face.knots_v = flat(b->VKnots(), b->VMultiplicities());
  const bool rational = b->IsURational() || b->IsVRational();
  for (int i = 1; i <= b->NbUPoles(); ++i) {
    for (int j = 1; j <= b->NbVPoles(); ++j) {
      const gp_Pnt p = b->Pole(i, j);
      face.poles.push_back(p.X());
      face.poles.push_back(p.Y());
      face.poles.push_back(p.Z());
      if (rational) face.weights.push_back(b->Weight(i, j));
    }
  }
}

}  // namespace facts_export

struct geospec_occt_facts_export {
  const geospec_occt_document* document = nullptr;
  TopoDS_Shape shape;
  std::vector<TopoDS_Shape> solids;
  std::vector<std::vector<TopoDS_Face>> faces;
  size_t free_faces = 0;
  facts_export::Face current;
};

namespace facts_export {

void index(geospec_occt_facts_export& e) {
  for (TopExp_Explorer solid(e.shape, TopAbs_SOLID); solid.More(); solid.Next()) {
    e.solids.push_back(solid.Current());
  }
  e.faces.resize(e.solids.size());
  for (TopExp_Explorer face(e.shape, TopAbs_FACE, TopAbs_SOLID); face.More(); face.Next()) {
    ++e.free_faces;
  }
}

// The first shell of a sewing of `faces`, as a solid oriented outward.
TopoDS_Shape sewn_solid(std::initializer_list<TopoDS_Shape> faces) {
  BRepBuilderAPI_Sewing sewing;
  for (const TopoDS_Shape& face : faces) sewing.Add(face);
  sewing.Perform();
  TopoDS_Shell shell;
  for (TopExp_Explorer s(sewing.SewedShape(), TopAbs_SHELL); s.More(); s.Next()) {
    shell = TopoDS::Shell(s.Current());
    break;
  }
  if (shell.IsNull()) {  // one face: sewing returns the face itself
    BRep_Builder builder;
    builder.MakeShell(shell);
    for (TopExp_Explorer f(sewing.SewedShape(), TopAbs_FACE); f.More(); f.Next()) {
      builder.Add(shell, f.Current());
    }
    shell.Closed(BRep_Tool::IsClosed(shell));
  }
  TopoDS_Solid solid = BRepBuilderAPI_MakeSolid(shell);
  BRepLib::OrientClosedSolid(solid);
  return solid;
}

// One face of a closed surface over [u0, u1] x [v0, v1], its seams sewn.
TopoDS_Shape closed_face_solid(const occ::handle<Geom_Surface>& surface, double u0, double u1,
                               double v0, double v1) {
  return sewn_solid({BRepBuilderAPI_MakeFace(surface, u0, u1, v0, v1, Precision::Confusion())});
}

// A planar face on one closed rim edge (in a plane z = const), its wire
// turned so that the face lies inside it.
TopoDS_Face cap(const TopoDS_Edge& edge) {
  const gp_Pln plane(BRep_Tool::Pnt(TopExp::FirstVertex(edge)), gp::DZ());
  TopoDS_Wire wire = BRepBuilderAPI_MakeWire(edge);
  TopoDS_Face face = BRepBuilderAPI_MakeFace(plane, wire, true);
  if (BRepTopAdaptor_FClass2d(face, Precision::Confusion()).PerformInfinitePoint() == TopAbs_IN) {
    face = BRepBuilderAPI_MakeFace(plane, TopoDS::Wire(wire.Reversed()), true);
  }
  return face;
}

// A tube face of `surface` (about the z axis) over u in [u0, u1] and v in
// [0, h], capped by planar faces on its two rims and sewn into a solid.
TopoDS_Shape capped_tube(const occ::handle<Geom_Surface>& surface, double u0, double u1,
                         double h) {
  TopoDS_Face side = BRepBuilderAPI_MakeFace(surface, u0, u1, 0.0, h, Precision::Confusion());
  std::vector<TopoDS_Face> caps;
  for (TopExp_Explorer e(side, TopAbs_EDGE); e.More(); e.Next()) {
    const TopoDS_Edge& edge = TopoDS::Edge(e.Current());
    if (BRep_Tool::Degenerated(edge)) continue;
    double first, last;
    const occ::handle<Geom_Curve> curve = BRep_Tool::Curve(edge, first, last);
    // The rims are the edges whose ends coincide; the two seam edges are not.
    if (curve->Value(first).Distance(curve->Value(last)) > Precision::Confusion()) continue;
    caps.push_back(cap(edge));
  }
  if (caps.size() != 2) return TopoDS_Shape();
  return sewn_solid({side, caps[0], caps[1]});
}

// Moves the 2D curve of the first edge use of the first face by `by` in the
// parameter plane, in place, and widens that edge's and its vertices'
// tolerances to `tolerance`.
void shift_pcurve(const TopoDS_Shape& solid, const gp_Vec2d& by, double tolerance) {
  BRep_Builder builder;
  TopExp_Explorer f(solid, TopAbs_FACE);
  if (!f.More()) return;
  const TopoDS_Face face = TopoDS::Face(f.Current());
  TopExp_Explorer e(face, TopAbs_EDGE);
  if (!e.More()) return;
  const TopoDS_Edge edge = TopoDS::Edge(e.Current());
  double first, last;
  occ::handle<Geom2d_Curve> c = BRep_Tool::CurveOnSurface(edge, face, first, last);
  if (c.IsNull()) return;
  c = occ::down_cast<Geom2d_Curve>(c->Copy());
  c->Translate(by);
  builder.UpdateEdge(edge, c, face, tolerance);
  builder.UpdateVertex(TopExp::FirstVertex(edge), tolerance);
  builder.UpdateVertex(TopExp::LastVertex(edge), tolerance);
}

// Replaces the 2D curve of the cylindrical face's first edge use that is not a
// seam or degenerate by a zero offset of itself (the same points, a curve kind
// the export does not take), in place.
void offset_pcurve(const TopoDS_Shape& solid) {
  BRep_Builder builder;
  for (TopExp_Explorer f(solid, TopAbs_FACE); f.More(); f.Next()) {
    const TopoDS_Face face = TopoDS::Face(f.Current());
    if (occ::down_cast<Geom_CylindricalSurface>(BRep_Tool::Surface(face)).IsNull()) continue;
    for (TopExp_Explorer e(face, TopAbs_EDGE); e.More(); e.Next()) {
      const TopoDS_Edge edge = TopoDS::Edge(e.Current());
      if (BRep_Tool::Degenerated(edge) || BRep_Tool::IsClosed(edge, face)) continue;
      double first, last;
      const occ::handle<Geom2d_Curve> c = BRep_Tool::CurveOnSurface(edge, face, first, last);
      if (c.IsNull()) continue;
      builder.UpdateEdge(edge, new Geom2d_OffsetCurve(c, 0.0), face, BRep_Tool::Tolerance(edge));
      return;
    }
  }
}

// A cylinder r 2, h 5 whose side face has no seam: its two rims are closed
// edges, each its own loop, whose 2D curves (lines over u in [0, 2 pi]) end
// one period from where they start. Planar caps close it.
TopoDS_Shape seamless_cylinder() {
  const occ::handle<Geom_CylindricalSurface> surface =
      new Geom_CylindricalSurface(gp_Ax3(gp::XOY()), 2);
  BRep_Builder builder;
  TopoDS_Face side;
  builder.MakeFace(side, surface, Precision::Confusion());
  TopoDS_Edge rims[2];
  for (int k = 0; k < 2; ++k) {
    const double z = 5.0 * k;
    rims[k] = BRepBuilderAPI_MakeEdge(
        new Geom_Circle(gp_Ax2(gp_Pnt(0, 0, z), gp::DZ(), gp::DX()), 2), 0.0, 2 * M_PI);
    builder.UpdateEdge(rims[k], new Geom2d_Line(gp_Pnt2d(0.0, z), gp_Dir2d(1, 0)), side,
                       Precision::Confusion());
    TopoDS_Wire wire;
    builder.MakeWire(wire);
    // The bottom rim runs +u (material at v > 0 on its left), the top -u.
    builder.Add(wire, k == 0 ? rims[k] : TopoDS::Edge(rims[k].Reversed()));
    builder.Add(side, wire);
  }
  return sewn_solid({side, cap(rims[0]), cap(rims[1])});
}

// The adapter tests' shapes, each with a closed-form volume and area.
TopoDS_Shape synthetic(int kind) {
  const double pi = M_PI;
  switch (kind) {
    case 0:  // box 10 x 20 x 30 at (1, 2, 3)
      return BRepPrimAPI_MakeBox(gp_Pnt(1, 2, 3), 10, 20, 30).Shape();
    case 1:  // cylinder r 3, h 7, tilted axis
      return BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(1, -2, 0.5), gp_Dir(1, 2, 2)), 3, 7).Shape();
    case 2:  // frustum r 4 to 1, h 5
      return BRepPrimAPI_MakeCone(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)), 4, 1, 5).Shape();
    case 3:  // sphere r 2.5
      return BRepPrimAPI_MakeSphere(gp_Pnt(3, 1, -2), 2.5).Shape();
    case 4:  // torus R 5, r 1.5
      return BRepPrimAPI_MakeTorus(gp_Ax2(gp_Pnt(0, 0, 1), gp_Dir(0, 0, 1)), 5, 1.5).Shape();
    case 5: {  // the cylinder of kind 1 mirrored (indirect frames)
      gp_Trsf mirror;
      mirror.SetMirror(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)));
      return BRepBuilderAPI_Transform(synthetic(1), mirror, true).Shape();
    }
    case 6: {  // box 10 x 20 x 30 minus a through hole r 2 along z
      TopoDS_Shape box = BRepPrimAPI_MakeBox(gp_Pnt(0, 0, 0), 10, 20, 30).Shape();
      TopoDS_Shape hole =
          BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(5, 10, -1), gp_Dir(0, 0, 1)), 2, 32).Shape();
      return BRepAlgoAPI_Cut(box, hole).Shape();
    }
    case 7:  // the cylinder of kind 1 as B-spline faces
      return BRepBuilderAPI_NurbsConvert(synthetic(1), true).Shape();
    case 8: {  // the sphere of kind 3 rotated and moved by a location
      gp_Trsf t;
      t.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 1, 0)), 0.7);
      t.SetTranslationPart(gp_Vec(4, -3, 2));
      return synthetic(3).Moved(TopLoc_Location(t));
    }
    case 9:  // one-face sphere r 2 with its seam at u = 3
      return closed_face_solid(new Geom_SphericalSurface(gp_Ax3(gp::XOY()), 2), 3.0, 3 + 2 * pi,
                               -pi / 2, pi / 2);
    case 10:  // one-face torus R 4, r 1 with its seams at u = 3 and v = 1
      return closed_face_solid(new Geom_ToroidalSurface(gp_Ax3(gp::XOY()), 4, 1), 3, 3 + 2 * pi,
                               1, 1 + 2 * pi);
    case 11:  // cylinder r 2, h 5 with its seam at u = 3 and planar caps
      return capped_tube(new Geom_CylindricalSurface(gp_Ax3(gp::XOY()), 2), 3, 3 + 2 * pi, 5);
    case 12: {  // a periodic rational B-spline cylinder r 2, h 5, seam inside the period
      occ::handle<Geom_Surface> cylinder = new Geom_RectangularTrimmedSurface(
          new Geom_CylindricalSurface(gp_Ax3(gp::XOY()), 2), 0.0, 2 * pi, 0.0, 5.0);
      occ::handle<Geom_BSplineSurface> b = GeomConvert::SurfaceToBSplineSurface(cylinder);
      b->SetUPeriodic();
      double u1, u2, v1, v2;
      b->Bounds(u1, u2, v1, v2);
      return capped_tube(b, u1 + 1.0, u1 + 1.0 + (u2 - u1), 5);
    }
    case 13:  // a one-face solid on an offset sphere: no export (a refusal)
      return closed_face_solid(
          new Geom_OffsetSurface(new Geom_SphericalSurface(gp_Ax3(gp::XOY()), 2), 0.5), 3.0,
          3 + 2 * pi, -pi / 2, pi / 2);
    case 14: {  // the cylinder of kind 1 under a mirroring location (a refusal)
      gp_Trsf mirror;
      mirror.SetMirror(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)));
      return synthetic(1).Moved(TopLoc_Location(mirror));
    }
    case 15:  // a cylinder whose rims' 2D curves end a period from their start: a wrap
      return seamless_cylinder();
    case 16: {  // the box of kind 0 with one pcurve 2e-4 off its vertices (tolerance 1e-3)
      TopoDS_Shape solid = synthetic(0);
      shift_pcurve(solid, gp_Vec2d(2e-4, 2e-4), 1e-3);
      return solid;
    }
    case 17: {  // kind 11 with its cylinder's bottom rim as an offset 2D curve: no export
      TopoDS_Shape solid = synthetic(11);
      offset_pcurve(solid);
      return solid;
    }
    case 18: {  // the box of kind 0 with 1 free face, 2 free edges and 3 free vertices
      BRep_Builder builder;
      TopoDS_Compound compound;
      builder.MakeCompound(compound);
      builder.Add(compound, synthetic(0));
      builder.Add(compound, BRepBuilderAPI_MakeFace(gp_Pln(gp_Pnt(0, 0, 40), gp_Dir(0, 0, 1)),
                                                    0, 4, 0, 4)
                                .Face());
      for (int k = 0; k < 2; ++k) {
        builder.Add(compound,
                    BRepBuilderAPI_MakeEdge(gp_Pnt(20 + k, 0, 0), gp_Pnt(20 + k, 5, 0)).Edge());
      }
      for (int k = 0; k < 3; ++k) {
        builder.Add(compound, BRepBuilderAPI_MakeVertex(gp_Pnt(30 + k, 0, 0)).Vertex());
      }
      return compound;
    }
    default:
      return TopoDS_Shape();
  }
}

}  // namespace facts_export

extern "C" {

int geospec_occt_facts_export_new(const geospec_occt_document* document,
                                  geospec_occt_facts_export** out_export,
                                  geospec_occt_string* error) noexcept {
  if (document == nullptr || out_export == nullptr) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "A document and an output are required.", error);
  }
  *out_export = nullptr;
  return guarded(error, [&]() -> int {
    auto e = std::make_unique<geospec_occt_facts_export>();
    e->document = document;
    e->shape = document->shape;
    facts_export::index(*e);
    *out_export = e.release();
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_export_synthetic(int kind, geospec_occt_facts_export** out_export,
                                        geospec_occt_string* error) noexcept {
  if (out_export == nullptr) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "An output is required.", error);
  }
  *out_export = nullptr;
  return guarded(error, [&]() -> int {
    auto e = std::make_unique<geospec_occt_facts_export>();
    e->shape = facts_export::synthetic(kind);
    if (e->shape.IsNull()) {
      return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "No such synthetic shape.", error);
    }
    facts_export::index(*e);
    *out_export = e.release();
    return GEOSPEC_OCCT_OK;
  });
}

void geospec_occt_facts_export_release(geospec_occt_facts_export* facts_export) noexcept {
  delete facts_export;
}

size_t geospec_occt_facts_body_count(const geospec_occt_facts_export* facts_export) noexcept {
  return facts_export == nullptr ? 0 : facts_export->solids.size();
}

size_t geospec_occt_facts_free_face_count(
    const geospec_occt_facts_export* facts_export) noexcept {
  return facts_export == nullptr ? 0 : facts_export->free_faces;
}

int geospec_occt_facts_shape(const geospec_occt_facts_export* facts_export, double* out_volume,
                             double* out_area, double* out_centre, double* out_ms,
                             geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out_volume == nullptr || out_area == nullptr ||
      out_centre == nullptr || out_ms == nullptr) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "An export and outputs are required.", error);
  }
  return guarded(error, [&]() -> int {
    const auto start = std::chrono::steady_clock::now();
    if (facts_export->document != nullptr) {
      const geospec_occt_shape_facts f = source_shape_facts(
          *facts_export->document, GEOSPEC_OCCT_REPORT_VOLUME | GEOSPEC_OCCT_REPORT_AREA);
      *out_volume = f.volume;
      *out_area = f.surface_area;
      std::copy(std::begin(f.center_of_mass), std::end(f.center_of_mass), out_centre);
    } else {
      GProp_GProps volume;
      BRepGProp::VolumeProperties(facts_export->shape, volume);
      *out_volume = volume.Mass();
      facts_export::put3(out_centre, volume.CentreOfMass().XYZ());
      *out_area = surface_area(facts_export->shape);
    }
    *out_ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start)
                  .count();
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_body(geospec_occt_facts_export* facts_export, size_t body,
                            geospec_occt_facts_body_record* out_body,
                            geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out_body == nullptr || body >= facts_export->solids.size()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "Facts body index/output is invalid.", error);
  }
  return guarded(error, [&]() -> int {
    const TopoDS_Shape& solid = facts_export->solids[body];
    geospec_occt_facts_body_record r{};
    std::vector<TopoDS_Face>& faces = facts_export->faces[body];
    faces.clear();
    for (TopExp_Explorer f(solid, TopAbs_FACE); f.More(); f.Next()) {
      faces.push_back(TopoDS::Face(f.Current()));
    }
    r.face_count = faces.size();
    r.closed = 1;
    for (TopExp_Explorer s(solid, TopAbs_SHELL); s.More(); s.Next()) {
      ++r.shell_count;
      if (!BRep_Tool::IsClosed(s.Current())) r.closed = 0;
    }
    if (r.shell_count == 0) r.closed = 0;
    facts_export::rigid(solid.Location(), &r.placement);
    Bnd_Box box;
    BRepBndLib::Add(solid, box, false);
    r.box_void = box.IsVoid() ? 1 : 0;
    if (!box.IsVoid()) {
      box.Get(r.box_min[0], r.box_min[1], r.box_min[2], r.box_max[0], r.box_max[1], r.box_max[2]);
    }
    for (TopExp_Explorer v(solid, TopAbs_VERTEX); v.More(); v.Next()) {
      r.max_tolerance = std::max(r.max_tolerance, BRep_Tool::Tolerance(TopoDS::Vertex(v.Current())));
    }
    for (TopExp_Explorer e(solid, TopAbs_EDGE); e.More(); e.Next()) {
      r.max_tolerance = std::max(r.max_tolerance, BRep_Tool::Tolerance(TopoDS::Edge(e.Current())));
    }
    for (const TopoDS_Face& f : faces) {
      r.max_tolerance = std::max(r.max_tolerance, BRep_Tool::Tolerance(f));
    }
    const auto start = std::chrono::steady_clock::now();
    GProp_GProps volume;
    BRepGProp::VolumeProperties(solid, volume);
    GProp_GProps area;
    BRepGProp::SurfaceProperties(solid, area);
    r.gprop_ms =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
    r.gprop_volume = volume.Mass();
    r.gprop_area = area.Mass();
    facts_export::put3(r.gprop_centre, volume.CentreOfMass().XYZ());
    *out_body = r;
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_body_eps(const geospec_occt_facts_export* facts_export, size_t body,
                                double eps, double* out, geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr || body >= facts_export->solids.size() ||
      !(eps > 0.0)) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "Facts body index, accuracy or output is invalid.",
                error);
  }
  return guarded(error, [&]() -> int {
    const TopoDS_Shape& solid = facts_export->solids[body];
    const auto start = std::chrono::steady_clock::now();
    GProp_GProps volume;
    const double volume_error = BRepGProp::VolumeProperties(solid, volume, eps, false, false);
    GProp_GProps area;
    const double area_error = BRepGProp::SurfaceProperties(solid, area, eps, false);
    out[0] = volume.Mass();
    out[1] = area.Mass();
    facts_export::put3(out + 2, volume.CentreOfMass().XYZ());
    out[5] = volume_error;
    out[6] = area_error;
    out[7] =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_body_gprop(const geospec_occt_facts_export* facts_export, size_t body,
                                  double* out, geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr || body >= facts_export->solids.size()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "Facts body index or output is invalid.", error);
  }
  return guarded(error, [&]() -> int {
    const TopoDS_Shape& solid = facts_export->solids[body];
    const auto start = std::chrono::steady_clock::now();
    GProp_GProps volume;
    BRepGProp::VolumeProperties(solid, volume);
    GProp_GProps area;
    BRepGProp::SurfaceProperties(solid, area);
    out[5] =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
    out[0] = volume.Mass();
    out[1] = area.Mass();
    facts_export::put3(out + 2, volume.CentreOfMass().XYZ());
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_face(geospec_occt_facts_export* facts_export, size_t body, size_t face,
                            geospec_occt_facts_face_record* out_face,
                            geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out_face == nullptr || body >= facts_export->faces.size() ||
      face >= facts_export->faces[body].size()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT,
                "Facts face index/output is invalid (export the body first).", error);
  }
  return guarded(error, [&]() -> int {
    using namespace facts_export;
    Face& current = facts_export->current;
    current = Face();
    geospec_occt_facts_face_record r{};
    const TopoDS_Face& source = facts_export->faces[body][face];
    r.orientation = orientation_of(source.Orientation());
    const TopoDS_Face forward = TopoDS::Face(source.Oriented(TopAbs_FORWARD));
    try {
      r.source = surface_source(BRep_Tool::Surface(forward));
      int placement = 0;
      if (!rigid(forward.Location(), &placement)) {
        throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION,
                      placement == GEOSPEC_OCCT_FACTS_MIRRORED ? "mirrored face placement"
                                                               : "scaled face placement"};
      }
      const occ::handle<Geom_Surface> surface = BRep_Tool::Surface(forward);
      if (surface.IsNull()) {
        throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION, "face without surface"};
      }
      // The trims, wire by wire, with the extent of their points.
      double extent[2][2] = {{std::numeric_limits<double>::infinity(),
                              -std::numeric_limits<double>::infinity()},
                             {std::numeric_limits<double>::infinity(),
                              -std::numeric_limits<double>::infinity()}};
      for (TopExp_Explorer w(forward, TopAbs_WIRE); w.More(); w.Next()) {
        const TopoDS_Wire& wire = TopoDS::Wire(w.Current());
        size_t uses = 0;
        for (TopExp_Explorer e(wire, TopAbs_EDGE); e.More(); e.Next()) ++uses;
        size_t count = 0;
        const size_t start = current.trims.size();
        std::vector<TopoDS_Edge> edges;
        std::vector<std::pair<double, double>> ranges;
        for (BRepTools_WireExplorer x(wire, forward); x.More(); x.Next()) {
          const TopoDS_Edge& edge = x.Current();
          if (edge.Orientation() != TopAbs_FORWARD && edge.Orientation() != TopAbs_REVERSED) {
            throw Refused{GEOSPEC_OCCT_FACTS_UNSUPPORTED_TRIM, "internal or external edge use"};
          }
          double first = 0.0, last = 0.0;
          occ::handle<Geom2d_Curve> c = BRep_Tool::CurveOnSurface(edge, forward, first, last);
          if (c.IsNull()) {
            throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION, "edge use without a 2D curve"};
          }
          while (auto trimmed = occ::down_cast<Geom2d_TrimmedCurve>(c)) c = trimmed->BasisCurve();
          if (!(first < last)) {
            throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION, "edge range is empty"};
          }
          Trim t;
          t.original = c;
          t.record.reversed = edge.Orientation() == TopAbs_REVERSED ? 1 : 0;
          t.record.seam = BRep_Tool::IsClosed(edge, forward) ? 1 : 0;
          t.record.degenerate = BRep_Tool::Degenerated(edge) ? 1 : 0;
          try {
            export_trim(t, first, last);
          } catch (const Refused&) {
            // Counted with the trims read so far, then the face refuses.
            current.trims.push_back(std::move(t));
            throw;
          }
          Bnd_Box2d box;
          BndLib_Add2dCurve::AddOptimal(c, first, last, 0.0, box);
          if (!box.IsVoid()) {
            double x0, y0, x1, y1;
            box.Get(x0, y0, x1, y1);
            extent[0][0] = std::min(extent[0][0], x0);
            extent[0][1] = std::max(extent[0][1], x1);
            extent[1][0] = std::min(extent[1][0], y0);
            extent[1][1] = std::max(extent[1][1], y1);
          }
          current.trims.push_back(std::move(t));
          edges.push_back(edge);
          ranges.emplace_back(first, last);
          ++count;
        }
        if (count != uses) {
          throw Refused{GEOSPEC_OCCT_FACTS_INVALID_DEFINITION,
                        "the wire explorer visits " + std::to_string(count) + " of " +
                            std::to_string(uses) + " edge uses"};
        }
        fill_gaps(current.trims, start, edges, ranges, surface);
        if (count != 0) current.loops.push_back(current.trims.size() - start);
      }
      export_surface(current, surface, r.surface, r.source, extent);
      // Exported coordinates: u' = sign u + shift[0], v' = v + shift[1]. A
      // reflection mirrors the parameter plane: every loop is reversed so that
      // the face stays on its left.
      for (Trim& t : current.trims) map_trim(t, current.sign, current.shift);
      if (current.sign < 0.0) {
        size_t start = 0;
        for (size_t n : current.loops) {
          std::reverse(current.trims.begin() + static_cast<std::ptrdiff_t>(start),
                       current.trims.begin() + static_cast<std::ptrdiff_t>(start + n));
          start += n;
        }
        for (Trim& t : current.trims) t.record.reversed = t.record.reversed ? 0 : 1;
        r.reflected = 1;
      }
    } catch (const Refused& refused) {
      // The edge uses read so far, for coverage only: their kinds and flags,
      // without knots, poles or loops, so that nothing can be integrated.
      std::vector<geospec_occt_facts_trim> read;
      for (const Trim& t : current.trims) {
        geospec_occt_facts_trim record = t.record;
        record.knots = nullptr;
        record.knot_count = 0;
        record.poles = nullptr;
        record.pole_count = 0;
        record.weights = nullptr;
        read.push_back(record);
      }
      current = Face();
      current.records = std::move(read);
      r.refusal = refused.code;
      r.surface = geospec_occt_facts_surface{};
      r.trims = current.records.empty() ? nullptr : current.records.data();
      r.trim_count = current.records.size();
      r.loops = nullptr;
      r.loop_count = 0;
      *out_face = r;
      return fail(GEOSPEC_OCCT_OK, refused.detail, error);
    }
    r.moved_origin = current.moved ? 1 : 0;
    r.shift[0] = current.shift[0];
    r.shift[1] = current.shift[1];
    current.records.clear();
    for (Trim& t : current.trims) {
      geospec_occt_facts_trim record = t.record;
      if (record.kind == GEOSPEC_OCCT_FACTS_TRIM_NURBS) {
        record.knots = t.knots.data();
        record.knot_count = t.knots.size();
        record.poles = t.poles.data();
        record.pole_count = t.poles.size() / 2;
        record.weights = t.weights.empty() ? nullptr : t.weights.data();
      }
      current.records.push_back(record);
    }
    if (r.surface.kind == GEOSPEC_OCCT_FACTS_NURBS) {
      r.surface.knots_u = current.knots_u.data();
      r.surface.knot_count_u = current.knots_u.size();
      r.surface.knots_v = current.knots_v.data();
      r.surface.knot_count_v = current.knots_v.size();
      r.surface.poles = current.poles.data();
      r.surface.weights = current.weights.empty() ? nullptr : current.weights.data();
    }
    r.trims = current.records.data();
    r.trim_count = current.records.size();
    r.loops = current.loops.data();
    r.loop_count = current.loops.size();
    *out_face = r;
    if (error != nullptr) error->length = 0;
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_surface_d1(const geospec_occt_facts_export* facts_export, double u,
                                  double v, double* out, geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr || facts_export->current.original.IsNull()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "No current face surface.", error);
  }
  return guarded(error, [&]() -> int {
    const facts_export::Face& f = facts_export->current;
    gp_Pnt p;
    gp_Vec du, dv;
    f.original->D1(f.sign * (u - f.shift[0]), v - f.shift[1], p, du, dv);
    facts_export::put3(out, p.XYZ());
    facts_export::put3(out + 3, (f.sign * du).XYZ());
    facts_export::put3(out + 6, dv.XYZ());
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_trim_d1(const geospec_occt_facts_export* facts_export, size_t trim,
                               double t, double* out, geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr || trim >= facts_export->current.trims.size()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "No such trim of the current face.", error);
  }
  return guarded(error, [&]() -> int {
    const facts_export::Face& f = facts_export->current;
    const facts_export::Trim& piece = f.trims[trim];
    gp_Pnt2d p;
    gp_Vec2d d(facts_export::kNaN, facts_export::kNaN);
    double distance = 0.0;
    if (piece.record.reparameterised) {
      const gp_Pnt2d q = piece.nurbs->Value(t);
      Geom2dAPI_ProjectPointOnCurve projection(q, piece.original);
      if (projection.NbPoints() == 0) {
        return fail(GEOSPEC_OCCT_NATIVE_ERROR, "Trim projection found no point.", error);
      }
      p = projection.NearestPoint();
      distance = projection.LowerDistance();
    } else {
      piece.original->D1(t, p, d);
    }
    out[0] = f.sign * p.X() + f.shift[0];
    out[1] = p.Y() + f.shift[1];
    out[2] = f.sign * d.X();
    out[3] = d.Y();
    out[4] = distance;
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_body_addoptimal(const geospec_occt_facts_export* facts_export, size_t body,
                                       double* out, geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr || body >= facts_export->solids.size()) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "Facts body index/output is invalid.", error);
  }
  return guarded(error, [&]() -> int {
    const TopoDS_Shape& solid = facts_export->solids[body];
    const auto start = std::chrono::steady_clock::now();
    Bnd_Box fold;
    double voids = 0.0;
    for (TopExp_Explorer face(solid, TopAbs_FACE); face.More(); face.Next()) {
      Bnd_Box box;
      BRepBndLib::AddOptimal(face.Current(), box, false, false);
      if (box.IsVoid()) {
        voids += 1.0;
        continue;
      }
      double xmin, ymin, zmin, xmax, ymax, zmax;
      box.Get(xmin, ymin, zmin, xmax, ymax, zmax);
      fold.Update(xmin, ymin, zmin, xmax, ymax, zmax);
    }
    out[6] =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
    out[7] = voids;
    if (fold.IsVoid()) {
      for (int k = 0; k < 6; ++k) out[k] = facts_export::kNaN;
    } else {
      fold.Get(out[0], out[1], out[2], out[3], out[4], out[5]);
    }
    return GEOSPEC_OCCT_OK;
  });
}

int geospec_occt_facts_free_counts(const geospec_occt_facts_export* facts_export, size_t* out,
                                   geospec_occt_string* error) noexcept {
  if (facts_export == nullptr || out == nullptr) {
    return fail(GEOSPEC_OCCT_INVALID_ARGUMENT, "An export and outputs are required.", error);
  }
  return guarded(error, [&]() -> int {
    size_t counts[3] = {0, 0, 0};
    const TopoDS_Shape& shape = facts_export->shape;
    for (TopExp_Explorer f(shape, TopAbs_FACE, TopAbs_SOLID); f.More(); f.Next()) ++counts[0];
    for (TopExp_Explorer e(shape, TopAbs_EDGE, TopAbs_FACE); e.More(); e.Next()) ++counts[1];
    for (TopExp_Explorer v(shape, TopAbs_VERTEX, TopAbs_EDGE); v.More(); v.Next()) ++counts[2];
    std::copy(std::begin(counts), std::end(counts), out);
    return GEOSPEC_OCCT_OK;
  });
}

}  // extern "C"
