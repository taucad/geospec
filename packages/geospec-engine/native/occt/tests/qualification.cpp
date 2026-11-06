// Synthetic OCCT controls for the bridge: shapes STEP cannot carry (INTERNAL
// edge uses, instance scales and orientations, faces without edges), built
// here and run against the bridge's own functions. The `qualification`
// feature compiles the bridge through this translation unit, and
// tests/qualification.rs runs each control by name.
// The bridge records what each STEP admission did for the admission controls below.
#define GEOSPEC_OCCT_ADMISSION_STATS 1
#include "../bridge/geospec_occt_bridge.cpp"

#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepPrimAPI_MakeBox.hxx>

#include <cstdio>
#include <cstring>
#include <initializer_list>
#include <stdexcept>
#include <thread>

namespace {

void check(bool condition, const std::string& message) {
  if (!condition) throw std::runtime_error(message);
}

TopoDS_Compound compound(std::initializer_list<TopoDS_Shape> children) {
  BRep_Builder builder;
  TopoDS_Compound result;
  builder.MakeCompound(result);
  for (const TopoDS_Shape& child : children) builder.Add(result, child);
  return result;
}

TopoDS_Shape place(const TopoDS_Shape& shape, double x) {
  gp_Trsf transform;
  transform.SetTranslation(gp_Vec(x, 0, 0));
  return shape.Located(TopLoc_Location(transform));
}

std::vector<TopoDS_Face> box_faces() {
  std::vector<TopoDS_Face> faces;
  for (TopExp_Explorer face(BRepPrimAPI_MakeBox(1, 2, 3).Shell(), TopAbs_FACE);
       face.More(); face.Next()) {
    faces.push_back(TopoDS::Face(face.Current()));
  }
  return faces;
}

TopoDS_Shell shell_of(const std::vector<TopoDS_Face>& faces) {
  BRep_Builder builder;
  TopoDS_Shell shell;
  builder.MakeShell(shell);
  for (const TopoDS_Face& face : faces) builder.Add(shell, face);
  return shell;
}

// A document whose occurrences are `leaves`, as admission places them.
geospec_occt_document document_for(const TopoDS_Shape& shape,
                                   std::initializer_list<TopoDS_Shape> leaves = {}) {
  geospec_occt_document document;
  document.shape = shape;
  for (const TopoDS_Shape& leaf : leaves) {
    OccurrenceFacts occurrence;
    occurrence.shape = leaf;
    document.occurrences.push_back(std::move(occurrence));
  }
  return document;
}

// The public closure FFI, as the Rust connector reads it.
geospec_occt_closure_facts closure_of(const geospec_occt_document& document) {
  geospec_occt_closure_facts facts{};
  check(geospec_occt_validity_closure(&document, &facts, nullptr) == GEOSPEC_OCCT_OK,
        "closure facet call failed");
  return facts;
}

geospec_occt_closure_group group_of(const geospec_occt_document& document,
                                    size_t index, std::vector<uint32_t>& occurrences) {
  geospec_occt_closure_group group{};
  occurrences.assign(document.occurrences.size(), 0);
  check(geospec_occt_validity_closure_group(&document, index, &group,
                                            occurrences.data(), occurrences.size(),
                                            nullptr) == GEOSPEC_OCCT_OK,
        "closure group call failed");
  occurrences.resize(group.occurrence_count);
  return group;
}

bool analyzer_valid(const TopoDS_Shape& shape) {
  return BRepCheck_Analyzer(shape, true, false, false).IsValid();
}

// V1: an INTERNAL edge use is skipped.
void internal_edge_is_skipped() {
  // A closed box whose first face also carries an INTERNAL edge in its own
  // wire: counted, that single use would open the shell.
  std::vector<TopoDS_Face> faces = box_faces();
  BRep_Builder builder;
  TopoDS_Face with_internal = TopoDS::Face(faces[0].EmptyCopied());
  for (TopoDS_Iterator wire(faces[0], false, false); wire.More(); wire.Next()) {
    builder.Add(with_internal, wire.Value());
  }
  TopoDS_Wire internal;
  builder.MakeWire(internal);
  builder.Add(internal, BRepBuilderAPI_MakeEdge(gp_Pnt(0, 0.5, 1), gp_Pnt(0, 1.5, 2))
                            .Edge()
                            .Oriented(TopAbs_INTERNAL));
  builder.Add(with_internal, internal);
  faces[0] = with_internal;
  const TopoDS_Shell shell = shell_of(faces);
  uint32_t internal_uses = 0;
  for (TopExp_Explorer edge(shell, TopAbs_EDGE); edge.More(); edge.Next()) {
    if (edge.Current().Orientation() == TopAbs_INTERNAL) ++internal_uses;
  }
  check(internal_uses == 1, "the control must carry one INTERNAL edge use");
  const geospec_occt_document document = document_for(shell);
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.shell_count == 1 && facts.open_edge_count == 0 &&
            facts.nonmanifold_edge_count == 0 && facts.failing_group_count == 0,
        "an INTERNAL edge use must not open or over-share the shell");
  std::string reason;
  const geospec_occt_validity_facts validity = compute_validity(document, reason);
  check(validity.free_bounds == 0 && validity.closed_shells == 1,
        "validity must read the facet for an INTERNAL edge");
}

// V1: every use of a face counts.
void face_used_twice_counts_every_use() {
  // A closed box listing its first face twice: that face's four edges have
  // three uses, odd and non-manifold at once.
  std::vector<TopoDS_Face> faces = box_faces();
  faces.push_back(faces[0]);
  const geospec_occt_document document = document_for(shell_of(faces));
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.open_edge_count == 4 && facts.nonmanifold_edge_count == 4 &&
            facts.failing_group_count == 1,
        "a face used twice must make its four edges open and non-manifold");
  std::vector<uint32_t> occurrences;
  const geospec_occt_closure_group group = group_of(document, 0, occurrences);
  check(!group.free_faces && group.sample_count == 4, "four samples of the shell");
  for (uint32_t sample = 0; sample < group.sample_count; ++sample) {
    check(group.samples[sample].face_uses == 3, "each sample edge has three face uses");
  }
  std::string reason;
  const geospec_occt_validity_facts validity = compute_validity(document, reason);
  check(validity.free_bounds == 4 && validity.nonmanifold_edge_count == 4 &&
            validity.closed_shells == 0 && validity.closed_solids == 0,
        "validity must report the over-used face");
}

// V1: the faces outside any shell are one group.
void free_faces_are_one_group() {
  // Two adjacent box faces outside any shell, beside a closed box: their
  // shared edge has two uses only when the free faces are one group.
  const std::vector<TopoDS_Face> faces = box_faces();
  TopoDS_Face first = faces[0], second;
  for (size_t index = 1; index < faces.size() && second.IsNull(); ++index) {
    for (TopExp_Explorer a(first, TopAbs_EDGE); a.More() && second.IsNull(); a.Next()) {
      for (TopExp_Explorer b(faces[index], TopAbs_EDGE); b.More(); b.Next()) {
        if (a.Current().IsSame(b.Current())) {
          second = faces[index];
          break;
        }
      }
    }
  }
  check(!second.IsNull(), "the control needs two adjacent faces");
  const TopoDS_Shape box = place(BRepPrimAPI_MakeBox(1, 2, 3).Solid(), 10);
  const TopoDS_Shape sheet = compound({first, second});
  const geospec_occt_document document =
      document_for(compound({sheet, box}), {sheet, box});
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.shell_count == 1 && facts.free_face_count == 2 &&
            facts.open_edge_count == 6 && facts.nonmanifold_edge_count == 0 &&
            facts.failing_group_count == 1,
        "two free faces sharing an edge must be one group with six open edges");
  std::vector<uint32_t> occurrences;
  const geospec_occt_closure_group group = group_of(document, 0, occurrences);
  check(group.free_faces == 1 && group.open_edge_count == 6 &&
            occurrences == std::vector<uint32_t>{0},
        "the free-face group names the occurrence that holds it");
}

// V1: an instanced open shell is one definition.
void instanced_open_shell_counts_once() {
  // An open box shell (five faces) placed twice as two leaf occurrences.
  std::vector<TopoDS_Face> faces = box_faces();
  faces.pop_back();
  const TopoDS_Shell open = shell_of(faces);
  const TopoDS_Shape first = place(open, 0), second = place(open, 10);
  const geospec_occt_document document =
      document_for(compound({first, second}), {first, second});
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.shell_count == 1 && facts.open_edge_count == 4 &&
            facts.failing_group_count == 1,
        "an instanced open shell is one definition with four open edges");
  std::vector<uint32_t> occurrences;
  const geospec_occt_closure_group group = group_of(document, 0, occurrences);
  check(!group.free_faces && group.open_edge_count == 4 &&
            occurrences == std::vector<uint32_t>{0, 1},
        "the failing shell names both occurrences");
  for (uint32_t sample = 0; sample < group.sample_count; ++sample) {
    const double* center = group.samples[sample].center;
    check(center[0] >= -1e-9 && center[0] <= 1 + 1e-9,
          "samples are placed at the first instance");
  }
  std::string reason;
  const geospec_occt_validity_facts validity = compute_validity(document, reason);
  check(validity.free_bounds == 4 && validity.closed_shells == 0,
        "validity counts the definition once");
}

// V1: one pass names each failing group's leaves.
void attribution_names_each_leaf_once() {
  // An open shell and a face-used-twice shell, placed under a parent
  // occurrence: leaf 1 holds the open shell twice, leaf 2 holds it once and
  // the other shell once. Each group names its leaves in ordinal order, a
  // leaf once however many instances it holds, and never the parent.
  std::vector<TopoDS_Face> faces = box_faces();
  std::vector<TopoDS_Face> overused = faces;
  overused.push_back(faces[0]);
  faces.pop_back();
  const TopoDS_Shell open = shell_of(faces), twice = shell_of(overused);
  const TopoDS_Shape pair = compound({place(open, 0), place(open, 10)});
  const TopoDS_Shape mixed = compound({place(open, 20), place(twice, 30)});
  const TopoDS_Shape root = compound({pair, mixed});
  geospec_occt_document document = document_for(root, {root, pair, mixed});
  document.occurrences[1].parent = 0;
  document.occurrences[2].parent = 0;
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.shell_count == 2 && facts.failing_group_count == 2,
        "two failing shell definitions");
  std::vector<uint32_t> occurrences;
  check(group_of(document, 0, occurrences).open_edge_count == 4 &&
            occurrences == std::vector<uint32_t>{1, 2},
        "the open shell names leaves 1 and 2 once each");
  check(group_of(document, 1, occurrences).nonmanifold_edge_count == 4 &&
            occurrences == std::vector<uint32_t>{2},
        "the over-used shell names leaf 2 only");
  std::vector<uint32_t> short_buffer(1, 0);
  geospec_occt_closure_group group{};
  check(geospec_occt_validity_closure_group(&document, 0, &group, short_buffer.data(),
                                            short_buffer.size(), nullptr) ==
            GEOSPEC_OCCT_INVALID_ARGUMENT,
        "a buffer shorter than the group's leaves is refused");
}

// V1: a group with no counted edge use, or a shape with no face, proves
// nothing closed.
void edgeless_and_faceless_prove_nothing_closed() {
  // An unbounded plane face has no wire, as a surfaceless face has none.
  const TopoDS_Face face = BRepBuilderAPI_MakeFace(gp_Pln()).Face();
  check(!TopExp_Explorer(face, TopAbs_EDGE).More(), "the control face has no edge");
  const TopoDS_Shell shell = shell_of({face});
  const geospec_occt_document document = document_for(shell, {shell});
  const geospec_occt_closure_facts facts = closure_of(document);
  check(facts.shell_count == 1 && facts.open_edge_count == 0 &&
            facts.nonmanifold_edge_count == 0 && facts.failing_group_count == 1,
        "an edgeless shell fails with no open or non-manifold edge");
  std::vector<uint32_t> occurrences;
  const geospec_occt_closure_group group = group_of(document, 0, occurrences);
  check(!group.free_faces && group.sample_count == 0 &&
            occurrences == std::vector<uint32_t>{0},
        "the edgeless shell names its occurrence, with no sample");
  std::string reason;
  check(compute_validity(document, reason).closed_shells == 0,
        "validity never calls an edgeless shell closed");
  const geospec_occt_document free_face = document_for(compound({face}));
  const geospec_occt_closure_facts free_facts = closure_of(free_face);
  check(free_facts.free_face_count == 1 && free_facts.failing_group_count == 1,
        "an edgeless free face fails as the free-face group");
  const geospec_occt_closure_facts none = closure_of(document_for(compound({})));
  check(none.shell_count == 0 && none.free_face_count == 0 &&
            none.failing_group_count == 0,
        "a shape with no face has no closure group; the claim fails it");
}

// V3: a scaled placement falls through to the located analyses.
void scale_two_falls_through() {
  // A definition-valid box placed rigidly and at scale two.
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  gp_Trsf scale;
  scale.SetScale(gp_Pnt(0, 0, 0), 2.0);
  scale.SetTranslationPart(gp_Vec(10, 0, 0));
  const TopoDS_Shape scaled = compound({box, box.Located(TopLoc_Location(scale))});
  std::vector<TopoDS_Shape> solids;
  check(collect_validation_solids(scaled, solids) && solids.size() == 2 &&
            disjoint_validation_solids(solids),
        "the scale-two control reaches the leaf branch");
  check(!rigid_placements(solids), "a scale-two placement is not rigid");
  check(analyzer_valid(box),
        "the definition itself is valid, so only the fall-through can refuse it");
  bool located = true;
  for (const TopoDS_Shape& solid : solids) located = located && analyzer_valid(solid);
  check(shape_is_valid(scaled).valid == (located || analyzer_valid(scaled)),
        "the scale-two verdict is the located loop's, not the definition's");
}

// V3: rigid instances of one definition answer from one analysis.
void rigid_instances_answer_from_one_analysis() {
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  const TopoDS_Shape rigid = compound({place(box, 0), place(box, 5), place(box, 10)});
  std::vector<TopoDS_Shape> solids;
  check(collect_validation_solids(rigid, solids) && rigid_placements(solids),
        "translated instances are rigid");
  uint32_t analyses = 0;
  check(shape_is_valid(rigid, &analyses).valid && analyses == 1,
        "three rigid instances of one valid definition answer from one analysis");
}

// V3: a REVERSED instance is not answered from its definition.
void reversed_instance_falls_through() {
  // A 10 mm box with a 2 mm void, placed FORWARD and REVERSED. Reversing
  // the instance turns the void shell against the solid, so the analyzer
  // rejects it though the FORWARD definition is valid.
  const TopoDS_Shell outer = BRepPrimAPI_MakeBox(gp_Pnt(0, 0, 0), 10, 10, 10).Shell();
  const TopoDS_Shell cavity = BRepPrimAPI_MakeBox(gp_Pnt(4, 4, 4), 2, 2, 2).Shell();
  BRep_Builder builder;
  TopoDS_Solid solid;
  builder.MakeSolid(solid);
  builder.Add(solid, outer);
  builder.Add(solid, cavity.Reversed());
  const TopoDS_Shape reversed = place(solid, 50).Reversed();
  check(analyzer_valid(solid) && !analyzer_valid(reversed),
        "the FORWARD definition is valid and its REVERSED instance is not");
  const TopoDS_Shape shape = compound({place(solid, 0), reversed});
  std::vector<TopoDS_Shape> solids;
  check(collect_validation_solids(shape, solids) && solids.size() == 2 &&
            disjoint_validation_solids(solids),
        "the control reaches the leaf branch");
  check(!rigid_placements(solids), "a REVERSED instance is not a rigid placement");
  uint32_t analyses = 0;
  const SourceValidity validity = shape_is_valid(shape, &analyses);
  check(!validity.valid && validity.invalid_solid_count == 1 && analyses == 2,
        "both instances are analyzed and the REVERSED one is invalid");
}

// P8 controls. A solid whose REVERSED instance the analyzer rejects (see
// reversed_instance_falls_through), and compounds that reach each branch of
// shape_is_valid: distinct definitions, rigid instances, an invalid instance
// first, last and among many, a scaled placement and a lone solid.
TopoDS_Solid cavity_solid() {
  const TopoDS_Shell outer = BRepPrimAPI_MakeBox(gp_Pnt(0, 0, 0), 10, 10, 10).Shell();
  const TopoDS_Shell cavity = BRepPrimAPI_MakeBox(gp_Pnt(4, 4, 4), 2, 2, 2).Shell();
  BRep_Builder builder;
  TopoDS_Solid solid;
  builder.MakeSolid(solid);
  builder.Add(solid, outer);
  builder.Add(solid, cavity.Reversed());
  return solid;
}

// A solid whose cavity shell keeps its FORWARD orientation: invalid as
// defined, so rigid FORWARD instances of it fail the definition loop.
TopoDS_Solid forward_invalid_solid() {
  const TopoDS_Shell outer = BRepPrimAPI_MakeBox(gp_Pnt(0, 0, 0), 10, 10, 10).Shell();
  const TopoDS_Shell cavity = BRepPrimAPI_MakeBox(gp_Pnt(4, 4, 4), 2, 2, 2).Shell();
  BRep_Builder builder;
  TopoDS_Solid solid;
  builder.MakeSolid(solid);
  builder.Add(solid, outer);
  builder.Add(solid, cavity);
  return solid;
}

struct SplitControl {
  const char* name;
  TopoDS_Shape shape;
  bool launches;  // a loop has two or more solids to analyze and no dominant one
};

std::vector<SplitControl> split_controls() {
  std::vector<SplitControl> shapes;
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  const TopoDS_Solid solid = cavity_solid();
  const TopoDS_Shape bad = place(solid, 500).Reversed();
  BRep_Builder builder;
  TopoDS_Compound distinct, instances, bad_first, bad_last, bad_among, dominant, defined_bad,
      quarter;
  for (TopoDS_Compound* c : {&distinct, &instances, &bad_first, &bad_last, &bad_among,
                             &dominant, &defined_bad, &quarter}) {
    builder.MakeCompound(*c);
  }
  builder.Add(bad_first, bad);
  for (int i = 0; i < 9; ++i) {
    const TopoDS_Shape own = BRepPrimAPI_MakeBox(gp_Pnt(20.0 * i, 0, 0), 1 + i, 2, 3).Solid();
    builder.Add(distinct, own);
    builder.Add(instances, place(box, 10.0 * i));
    builder.Add(bad_first, own);
    builder.Add(bad_last, own);
    builder.Add(bad_among, own);
    if (i == 4) builder.Add(bad_among, bad);
    if (i < 3) builder.Add(dominant, own);
    if (i < 6) builder.Add(quarter, own);
    builder.Add(defined_bad, own);
  }
  builder.Add(bad_last, bad);
  // One solid with more than a quarter of the faces: 12 of 30.
  builder.Add(dominant, place(solid, 500));
  // Exactly a quarter, 12 of 48, still splits.
  builder.Add(quarter, place(solid, 500));
  // Rigid FORWARD instances of an invalid definition after nine valid boxes.
  const TopoDS_Solid forward_bad = forward_invalid_solid();
  builder.Add(defined_bad, place(forward_bad, 600));
  builder.Add(defined_bad, place(forward_bad, 700));
  gp_Trsf scale;
  scale.SetScale(gp_Pnt(0, 0, 0), 2.0);
  scale.SetTranslationPart(gp_Vec(10, 0, 0));
  shapes.push_back({"distinct", distinct, true});
  shapes.push_back({"instances", instances, false});
  shapes.push_back({"invalid-first", bad_first, true});
  shapes.push_back({"invalid-last", bad_last, true});
  shapes.push_back({"invalid-among", bad_among, true});
  shapes.push_back({"dominant", dominant, false});
  shapes.push_back({"invalid-definition", defined_bad, true});
  shapes.push_back({"exact-quarter", quarter, true});
  shapes.push_back({"scaled", compound({box, box.Located(TopLoc_Location(scale))}), false});
  shapes.push_back({"lone-solid", box, false});
  shapes.push_back({"lone-in-compound", compound({box}), false});
  return shapes;
}

std::string validity_bytes(const geospec_occt_document& document, ValiditySplit* split) {
  std::string reason;
  const geospec_occt_validity_facts f = compute_validity(document, reason, split);
  std::string bytes;
  const auto put = [&bytes](const auto& field) {
    bytes.append(reinterpret_cast<const char*>(&field), sizeof field);
  };
  put(f.valid), put(f.max_tolerance), put(f.free_bounds), put(f.same_parameter);
  put(f.closed_shells), put(f.closed_solids), put(f.solid_count);
  put(f.invalid_solid_count), put(f.open_edge_count), put(f.closed_wires);
  put(f.nonmanifold_edge_count);
  return bytes + "|" + reason;
}

// P8: the split's facts are the serial path's bytes at widths 1, 2, 4 and 8,
// on a private pool wide enough for each, and width 1 runs exactly the
// serial analyses on the caller.
void validity_split_matches_serial() {
  const occ::handle<OSD_ThreadPool> pool = new OSD_ThreadPool(8);
  check(pool->NbThreads() >= 8, "the private pool has eight threads");
  for (const auto& [name, shape, launches] : split_controls()) {
    const std::string label(name);
    uint32_t serial_analyses = 0;
    const SourceValidity serial = shape_is_valid(shape, &serial_analyses);
    const std::string serial_bytes = validity_bytes(document_for(shape), nullptr);
    for (int width : {1, 2, 4, 8}) {
      const std::string at = label + " at width " + std::to_string(width);
      for (int repeat = 0; repeat < 3; ++repeat) {
        ValiditySplit split;
        split.pool = pool.get();
        split.width = width;
        uint32_t analyses = 0;
        const SourceValidity granted = shape_is_valid(shape, &analyses, &split);
        check(granted.valid == serial.valid &&
                  granted.invalid_solid_count == serial.invalid_solid_count,
              at + ": the split decides what the serial loops decide");
        check(split.used_parallel == (width >= 2 && launches),
              at + ": parallel use is reported exactly when a launch had threads");
        check(split.threads <= width, at + ": no launch exceeds the width");
        if (width == 1) {
          check(analyses == serial_analyses, at + ": width one runs the serial analyses");
        }
        if (label == "invalid-definition") {
          // Serial: nine boxes and the definition, then the ten located
          // solids up to the first instance of the invalid definition. The
          // split reuses those ten answers and analyzes nothing again.
          check(serial_analyses == 20, at + ": the serial loops analyze twenty solids");
          check(width == 1 || analyses == 10, at + ": the located loop reuses every answer");
        }
        ValiditySplit facts_split;
        facts_split.pool = pool.get();
        facts_split.width = width;
        check(validity_bytes(document_for(shape), &facts_split) == serial_bytes,
              at + ": the validity facts and reason are the serial bytes");
      }
    }
  }
}

// P8 seam controls. A scripted analyzer stands in for BRepCheck: each listed
// solid answers valid, invalid or throws, optionally after a delay, so the
// order in which answers finish differs from index order. The outcome of a
// loop is "valid", "invalid" or the thrown message, and the split must give
// the serial loop's outcome at every width.
struct Scripted {
  TopoDS_Shape solid;
  char answer;  // 'v' valid, 'i' invalid, 't' throws
  int delay_ms;
};

SolidAnalyzer scripted_analyzer(const std::vector<Scripted>& script) {
  return [&script](const TopoDS_Shape& solid) {
    for (const Scripted& entry : script) {
      if (!entry.solid.IsEqual(solid)) continue;
      if (entry.delay_ms > 0) {
        std::this_thread::sleep_for(std::chrono::milliseconds(entry.delay_ms));
      }
      if (entry.answer == 't') {
        throw Standard_Failure(("scripted throw at x=" +
                                std::to_string(static_cast<int>(
                                    entry.solid.Location().Transformation()
                                        .TranslationPart().X())))
                                   .c_str());
      }
      return entry.answer == 'v';
    }
    throw std::runtime_error("the scripted analyzer met an unlisted solid");
  };
}

template <typename Run>
std::string outcome(Run run) {
  try {
    return run() ? "valid" : "invalid";
  } catch (const Standard_Failure& failure) {
    return std::string("threw: ") + failure.what();
  }
}

void validity_split_keeps_serial_failure_order() {
  const occ::handle<OSD_ThreadPool> pool = new OSD_ThreadPool(8);
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  // Eight located instances of one box, given to solids_valid directly.
  std::vector<TopoDS_Shape> list;
  for (int i = 0; i < 8; ++i) list.push_back(place(box, 10.0 * i));
  struct Case {
    const char* name;
    std::vector<std::pair<int, std::pair<char, int>>> script;  // index -> answer, delay
    const char* expected;
  };
  const std::vector<Case> cases = {
      // A throw at 3 finishes first; the invalid solid at 2 decides.
      {"throw-after-invalid", {{2, {'i', 60}}, {3, {'t', 0}}}, "invalid"},
      // An invalid solid at 3 finishes first; the throw at 2 decides.
      {"invalid-after-throw", {{2, {'t', 60}}, {3, {'i', 0}}}, "threw: scripted throw at x=20"},
      // Two throws: the lower index decides whatever finishes first.
      {"two-throws", {{1, {'t', 60}}, {2, {'t', 0}}}, "threw: scripted throw at x=10"},
      // Invalid at 1 late, invalid at 4 early: plain invalid.
      {"two-invalid", {{1, {'i', 60}}, {4, {'i', 0}}}, "invalid"},
  };
  for (const Case& c : cases) {
    std::vector<Scripted> script;
    for (int i = 0; i < 8; ++i) script.push_back({list[i], 'v', 0});
    for (const auto& [index, answer] : c.script) {
      script[index].answer = answer.first;
      script[index].delay_ms = answer.second;
    }
    const SolidAnalyzer analyzer = scripted_analyzer(script);
    const std::string serial =
        outcome([&] { return solids_valid(list, nullptr, nullptr, nullptr, &analyzer); });
    check(serial == c.expected, std::string(c.name) + ": the serial loop gives " + serial);
    for (int width : {1, 2, 4, 8}) {
      for (int repeat = 0; repeat < 5; ++repeat) {
        ValiditySplit split;
        split.pool = pool.get();
        split.width = width;
        const std::string granted = outcome(
            [&] { return solids_valid(list, nullptr, &split, nullptr, &analyzer); });
        check(granted == serial, std::string(c.name) + " at width " + std::to_string(width) +
                                     ": " + granted + ", serial " + serial);
        check(split.used_parallel == (width >= 2), std::string(c.name) + ": the loop launched");
      }
    }
  }
  // Through shape_is_valid: rigid instances of eight definitions, the third
  // invalid (late) and the sixth throwing (early). The definition loop keeps
  // the throw at a position past its first failure; the located loop starts
  // from those answers and must stop at the invalid solid, never rethrow.
  std::vector<TopoDS_Shape> solids;
  BRep_Builder builder;
  TopoDS_Compound shape;
  builder.MakeCompound(shape);
  for (int i = 0; i < 8; ++i) {
    const TopoDS_Shape own = BRepPrimAPI_MakeBox(gp_Pnt(20.0 * i, 0, 0), 1 + i, 2, 3).Solid();
    solids.push_back(own);
    builder.Add(shape, own);
  }
  std::vector<Scripted> script;
  for (int i = 0; i < 8; ++i) script.push_back({solids[i], 'v', 0});
  script[2] = {solids[2], 'i', 60};
  script[5] = {solids[5], 't', 0};
  const SolidAnalyzer analyzer = scripted_analyzer(script);
  uint32_t serial_analyses = 0;
  const auto run = [&](ValiditySplit* split, uint32_t* analyses) {
    return outcome([&] { return shape_is_valid(shape, analyses, split, &analyzer).valid; });
  };
  const std::string serial = run(nullptr, &serial_analyses);
  // Serial: definitions 0..2, located 0..2, then the real whole-shape check,
  // which finds eight valid boxes.
  check(serial == "valid" && serial_analyses == 6,
        "carried throw: the serial loops stop at the invalid solid, then the whole shape decides");
  for (int width : {1, 2, 4, 8}) {
    for (int repeat = 0; repeat < 5; ++repeat) {
      ValiditySplit split;
      split.pool = pool.get();
      split.width = width;
      uint32_t analyses = 0;
      const std::string granted = run(&split, &analyses);
      check(granted == serial, "carried throw at width " + std::to_string(width) + ": " +
                                   granted + ", serial " + serial);
    }
  }
}

// P8: many located instances of one definition analyzed at once. Mirrored
// and translated instances share every TShape and geometry handle; a REVERSED
// invalid instance last keeps them off the definition loop and makes the
// located loop launch over all of them. Real analyzers, 50 rounds per width.
void validity_split_shares_topology_safely() {
  const occ::handle<OSD_ThreadPool> pool = new OSD_ThreadPool(8);
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  BRep_Builder builder;
  TopoDS_Compound shape;
  builder.MakeCompound(shape);
  for (int i = 0; i < 8; ++i) builder.Add(shape, place(box, 10.0 * i));
  for (int i = 0; i < 4; ++i) {
    gp_Trsf mirror;
    mirror.SetMirror(gp_Ax2(gp_Pnt(200.0 + 10.0 * i, 0, 0), gp_Dir(1, 0, 0)));
    builder.Add(shape, box.Located(TopLoc_Location(mirror)));
  }
  builder.Add(shape, place(cavity_solid(), 500).Reversed());
  std::vector<TopoDS_Shape> solids;
  check(collect_validation_solids(shape, solids) && solids.size() == 13 &&
            disjoint_validation_solids(solids) && !rigid_placements(solids) &&
            !dominant_solid(solids),
        "the shared-topology control reaches an ungated located loop");
  uint32_t serial_analyses = 0;
  const SourceValidity serial = shape_is_valid(shape, &serial_analyses);
  const std::string serial_bytes = validity_bytes(document_for(shape), nullptr);
  for (int width : {2, 4, 8}) {
    for (int repeat = 0; repeat < 50; ++repeat) {
      ValiditySplit split;
      split.pool = pool.get();
      split.width = width;
      const SourceValidity granted = shape_is_valid(shape, nullptr, &split);
      check(granted.valid == serial.valid &&
                granted.invalid_solid_count == serial.invalid_solid_count &&
                split.used_parallel,
            "shared topology at width " + std::to_string(width) + ": the serial answer");
      if (repeat % 10 == 0) {
        ValiditySplit facts_split;
        facts_split.pool = pool.get();
        facts_split.width = width;
        check(validity_bytes(document_for(shape), &facts_split) == serial_bytes,
              "shared topology at width " + std::to_string(width) + ": the serial bytes");
      }
    }
  }
}

// P8: the effective width never exceeds four, whatever the grant; no grant
// keeps the serial path.
void validity_split_width_caps_at_four() {
  check(validity_split_width(0) == 0 && validity_split_width(1) == 1 &&
            validity_split_width(2) == 2 && validity_split_width(3) == 3 &&
            validity_split_width(4) == 4 && validity_split_width(5) == 4 &&
            validity_split_width(8) == 4 && validity_split_width(64) == 4,
        "the split width is min(grant, 4), and 0 without a grant");
  const occ::handle<OSD_ThreadPool> pool = new OSD_ThreadPool(8);
  const TopoDS_Shape shape = split_controls().front().shape;
  ValiditySplit split;
  split.pool = pool.get();
  split.width = validity_split_width(8);
  check(shape_is_valid(shape, nullptr, &split).valid, "the distinct boxes are valid");
  check(split.used_parallel && split.threads >= 2 && split.threads <= 4,
        "a grant of eight launches on at most four threads");
}

// P8: without a grant, validity runs no split launch and reports no parallel
// use, through the public entry and the dedicated entry at grant 0.
void validity_default_is_serial() {
  const TopoDS_Shape shape = split_controls().front().shape;
  const uint32_t before = validity_split_launches;
  for (int entry = 0; entry < 2; ++entry) {
    const geospec_occt_document document = document_for(shape);
    geospec_occt_validity_facts facts{};
    int used_parallel = 0;
    int status = GEOSPEC_OCCT_OK;
    if (entry == 0) {
      status = geospec_occt_validity(&document, &facts, nullptr, nullptr);
    } else {
      used_parallel = -1;
      status = geospec_occt_validity_dedicated(&document, 0, &used_parallel, &facts,
                                               nullptr, nullptr);
    }
    check(status == GEOSPEC_OCCT_OK && facts.valid == 1, "the default validity call succeeds");
    check(used_parallel == 0, "the default path reports no parallel use");
  }
  check(validity_split_launches == before, "the default path launches no split");
}

// S4: the continuous-wall path judges its outer shell by computed edge-use
// parity, never by the stored Closed() flag a healer may write. A box whose
// shell stores Closed()=false still closes by parity; before this gate W2B
// refused it with "Continuous wall requires one closed outer shell without
// cavities." (M0, before S4: "A regular-solid shape contains an open shell.").
std::string wall_domain(bool stored_closed) {
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(10.0, 20.0, 30.0).Shape();
  TopExp_Explorer explorer(box, TopAbs_SHELL);
  TopoDS_Shape shell = explorer.Current();
  shell.Closed(stored_closed);  // on the TShape the solid shares
  check(BRep_Tool::IsClosed(shell), "the box shell closes by edge-use parity");
  geospec_occt_continuous_wall_domain domain;
  std::memset(&domain, 0, sizeof domain);  // padding compares too
  ShapeIndex faces, edges;
  std::string message;
  check(classify_continuous_wall(box, domain, faces, edges, message), message);
  return std::string(reinterpret_cast<const char*>(&domain), sizeof domain);
}

void wall_closure_ignores_the_stored_flag() {
  check(wall_domain(false) == wall_domain(true),
        "the stored Closed() flag moves no continuous-wall byte");
}

// The admission product index and the ordered owner joins.
void product_owner_joins() {
  const std::vector<ProductFacts> products = {
      {"0:1", "first"}, {"0:2", "second"}, {"0:1", "duplicate"}};
  const auto indices = index_products(products);
  check(product_index(indices, "0:1") == 0 && product_index(indices, "0:2") == 1,
        "products index by label, the first duplicate winning");
  bool refused = false;
  try {
    (void)product_index(indices, "missing");
  } catch (const Standard_Failure& failure) {
    refused = std::strcmp(failure.what(),
                          "Occurrence product is absent from product facts.") == 0;
  }
  check(refused, "a missing product fails with its message");
  std::vector<OccurrenceFacts> occurrences(5);
  occurrences[0].product = 1;
  occurrences[1].product = 0;
  occurrences[2].product = 1;
  occurrences[3].product = 1;
  occurrences[4].product = 4;  // Outside the product sequence: no owner.
  const auto owners = index_product_owners(products.size(), occurrences);
  check(owners[0] == std::vector<int>{1} && owners[1] == std::vector<int>{0, 2, 3} &&
            owners[2].empty(),
        "owners join in occurrence order, and an outside product owns nothing");
}

// M2: the filter boxes of a body's faces grow by each face's largest
// tolerance, so a vertex that lies off its faces' surfaces stays inside them;
// the body bounds fold the boxes ungrown, as bounds() does.
void component_face_boxes_grow_by_their_tolerances() {
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(10, 10, 10).Solid();
  TopoDS_Vertex corner;
  for (TopExp_Explorer vertex(box, TopAbs_VERTEX); vertex.More() && corner.IsNull();
       vertex.Next()) {
    const TopoDS_Vertex& current = TopoDS::Vertex(vertex.Current());
    if (BRep_Tool::Pnt(current).Distance(gp_Pnt(10, 10, 10)) < 1e-9) corner = current;
  }
  check(!corner.IsNull(), "the control needs the box's far corner");
  // The corner 0.05 mm above its edges' ends, with a tolerance that still
  // joins them.
  BRep_Builder().UpdateVertex(corner, gp_Pnt(10, 10, 10.05), 0.06);
  const geospec_occt_document document = document_for(box);
  geospec_occt_component_bodies* bodies = nullptr;
  size_t body_count = 0, face_count = 0;
  const geospec_occt_charge approve = [](void*, uint64_t) { return 0; };
  check(geospec_occt_component_bodies_new(&document, nullptr, 0, approve, nullptr,
                                          &bodies, &body_count, &face_count,
                                          nullptr) == GEOSPEC_OCCT_OK &&
            body_count == 1 && face_count == 6,
        "the box is one solid body of six faces");
  geospec_occt_component_body body{};
  std::vector<geospec_occt_bounds> faces(face_count);
  const int status = geospec_occt_component_bodies_facts(bodies, &body, 1, faces.data(),
                                                         faces.size(), nullptr);
  geospec_occt_component_bodies_release(bodies);
  check(status == GEOSPEC_OCCT_OK, "the body facts are read");
  const geospec_occt_bounds exact = memo_bounds(document, box);
  check(std::memcmp(&body.bounds, &exact, sizeof exact) == 0,
        "the body bounds stay the ungrown fold");
  check(exact.max[2] < 10.05, "the memo boxes miss the raised corner");
  size_t reaching = 0;
  for (const geospec_occt_bounds& face : faces) reaching += face.max[2] >= 10.05 ? 1 : 0;
  check(reaching == 3, "the three faces at the corner reach it");
}

// The report prototype copy: only a compound of located-disjoint leaves
// whose definitions share no descendant is eligible.
void prototype_copy_eligibility() {
  const TopoDS_Shape box = BRepPrimAPI_MakeBox(1, 2, 3).Solid();
  check(prototype_copy_eligible(compound({place(box, 0), place(box, 5)})),
        "two placed instances of one solid are eligible");
  check(!prototype_copy_eligible(box), "a single solid takes the general copy");
  // Two distinct solids on one shell: disjoint placements, one shared
  // unlocated descendant.
  TopoDS_Shape shell;
  for (TopoDS_Iterator child(box, true, true); child.More(); child.Next()) {
    if (child.Value().ShapeType() == TopAbs_SHELL) shell = child.Value();
  }
  BRep_Builder builder;
  TopoDS_Solid other;
  builder.MakeSolid(other);
  builder.Add(other, shell);
  const TopoDS_Shape shared = compound({box, place(other, 10)});
  std::vector<TopoDS_Shape> solids;
  check(collect_validation_solids(shared, solids) && disjoint_validation_solids(solids),
        "the shared-descendant control passes the placement gate");
  check(!prototype_copy_eligible(shared), "definitions sharing a descendant are refused");
  check(!prototype_copy_eligible(compound({box, box})), "one placed leaf twice is refused");
  check(!prototype_copy_eligible(compound({box, shell})),
        "a leaf inside another leaf is refused");
}

struct Control {
  const char* name;
  void (*run)();
};

constexpr Control kControls[] = {
    {"internal-edge-is-skipped", internal_edge_is_skipped},
    {"face-used-twice-counts-every-use", face_used_twice_counts_every_use},
    {"free-faces-are-one-group", free_faces_are_one_group},
    {"instanced-open-shell-counts-once", instanced_open_shell_counts_once},
    {"attribution-names-each-leaf-once", attribution_names_each_leaf_once},
    {"edgeless-and-faceless-prove-nothing-closed",
     edgeless_and_faceless_prove_nothing_closed},
    {"scale-two-falls-through", scale_two_falls_through},
    {"rigid-instances-answer-from-one-analysis", rigid_instances_answer_from_one_analysis},
    {"reversed-instance-falls-through", reversed_instance_falls_through},
    {"wall-closure-ignores-the-stored-flag", wall_closure_ignores_the_stored_flag},
    {"validity-split-matches-serial", validity_split_matches_serial},
    {"validity-split-width-caps-at-four", validity_split_width_caps_at_four},
    {"validity-default-is-serial", validity_default_is_serial},
    {"validity-split-keeps-serial-failure-order", validity_split_keeps_serial_failure_order},
    {"validity-split-shares-topology-safely", validity_split_shares_topology_safely},
    {"product-owner-joins", product_owner_joins},
    {"prototype-copy-eligibility", prototype_copy_eligibility},
    {"component-face-boxes-grow-by-their-tolerances",
     component_face_boxes_grow_by_their_tolerances},
};

}  // namespace

// Runs control `name`: 0 when it holds, 1 with the failed check in `message`,
// 2 for an unknown name.
extern "C" int geospec_occt_qualify(const char* name, char* message,
                                    size_t capacity) noexcept {
  const auto report = [&](const char* text) {
    if (capacity != 0) std::snprintf(message, capacity, "%s", text);
  };
  for (const Control& control : kControls) {
    if (std::strcmp(control.name, name) != 0) continue;
    try {
      control.run();
      return 0;
    } catch (const Standard_Failure& failure) {
      report(failure.what());
    } catch (const std::exception& error) {
      report(error.what());
    }
    return 1;
  }
  report("unknown qualification control");
  return 2;
}

// STEP admission controls (tests/step_threaded_admission.rs). Selects the next
// admissions' mode (-1 environment, 0 serial, 1 threaded) and whether every
// deferred completion's stand-in check fails, so that the threaded transfer diverges
// and is read again in place.
extern "C" void geospec_occt_qualify_step_admission_mode(int mode,
                                                         bool force_divergence) noexcept {
  step_admission_mode_override = mode;
  step_admission_force_divergence = force_divergence;
}

// What the last STEP admission did: threaded (0 or 1), solids completed on a
// worker, solids completed in place because they share entities with a pending
// solid, worker completions whose result replaced bound shapes, and threaded
// transfers read again in place.
extern "C" void geospec_occt_qualify_last_step_admission(int* threaded, int* deferred_fixes,
                                                         int* shared_fixes,
                                                         int* reshaped_fixes,
                                                         int* serial_retries) noexcept {
  *threaded = last_step_admission.threaded ? 1 : 0;
  *deferred_fixes = last_step_admission.deferred_fixes;
  *shared_fixes = last_step_admission.shared_fixes;
  *reshaped_fixes = last_step_admission.reshaped_fixes;
  *serial_retries = last_step_admission.serial_retries;
}
