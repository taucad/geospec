"""Verify the pinned STEP deferred solid fix patch without building OCCT."""

import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[5]
OCCT = ROOT / 'packages/geospec-engine/native/occt'
SOURCE = Path(os.environ.get(
    'GEOSPEC_OCCT_SOURCE',
    ROOT / 'node_modules/.cache/geospec-engine-native/sources/occt',
))
PATCH = OCCT / 'step-deferred-solid-fix.patch'
SHARINGS = OCCT / 'step-assembly-sharings.patch'
HEADER = Path('src/DataExchange/TKDESTEP/STEPControl/STEPControl_ActorRead.hxx')
ACTOR = Path('src/DataExchange/TKDESTEP/STEPControl/STEPControl_ActorRead.cxx')
ASSEMBLY = Path('src/DataExchange/TKDESTEP/STEPConstruct/STEPConstruct_Assembly.cxx')
PRISTINE_HEADER = '856b9a7955d9e8c6a5bd50960ff491d3cc867c678758b27571ea5c7277634e00'
SHARED_ACTOR = 'f75f55b9ab11b8bcbaed82204519de6f34c2ff1ba9ca9ab33bc5a0d096e0d0fd'
PATCHED_HEADER = 'b2c46e645d65db3d0b0316fe7880331588c8fddae4c3cd69394de6b862c749ee'
PATCHED_ACTOR = '2ce5d6b8e407fb7d7b2fac0b0efe23592cdb2254574ff4ffedaa488dde34038a'
PATCH_SHA = '62a9967008baf9e046dbc9899821fb08b1a2d8861d584e3e3fae18ebf9eb8e3c'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def between(text, start, end):
    first = text.index(start)
    return text[first:text.index(end, first + len(start))]


def single_threaded(text):
    """The lines an Emscripten build compiles: drop every `#if !defined(__EMSCRIPTEN__)` branch."""
    kept, stack = [], []
    for line in text.splitlines():
        directive = line.strip()
        if directive.startswith('#if'):
            stack.append('native' if directive == '#if !defined(__EMSCRIPTEN__)' else 'other')
            continue
        if directive.startswith('#else') or directive.startswith('#elif'):
            assert stack[-1] != 'native' or directive == '#else', line
            stack[-1] = 'wasm' if stack[-1] == 'native' else stack[-1]
            continue
        if directive.startswith('#endif'):
            stack.pop()
            continue
        if 'native' not in stack:
            kept.append(line)
    assert not stack
    return '\n'.join(kept)


def main():
    assert digest(PATCH) == PATCH_SHA
    builder = (OCCT / 'build-occt.sh').read_text()
    for pin in (f'expected_deferred_fix_patch_hash="{PATCH_SHA}"',
                f'expected_deferred_fix_header_hash="{PATCHED_HEADER}"',
                f'expected_deferred_fix_source_hash="{PATCHED_ACTOR}"'):
        assert pin in builder, pin
    # Applied after the sharings edit of the same actor and after the last prior patch, before configure.
    apply = builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${deferred_fix_patch_file}"')
    assert builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${sharings_patch_file}"') < apply
    assert builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${mimalloc_patch_file}"') < apply
    assert apply < builder.index('cmake -S "${build_source}"')
    assert '-DGEOSPEC_OCCT_DEFERRED_FIX_PATCH_SHA256:STRING="${expected_deferred_fix_patch_hash}"' in builder
    files = {line.split()[1].split('/', 1)[1] for line in PATCH.read_text().splitlines() if line.startswith('+++ ')}
    assert files == {str(HEADER), str(ACTOR)}
    for other in OCCT.glob('*.patch'):
        if other not in (PATCH, SHARINGS):
            assert str(ACTOR) not in other.read_text() and str(HEADER) not in other.read_text(), other.name

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for rel in (HEADER, ACTOR, ASSEMBLY):
            target = root / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((SOURCE / rel).read_bytes())
        assert digest(root / HEADER) == PRISTINE_HEADER
        subprocess.run(['patch', '-t', '-F', '0', '-p1', '-d', directory, '-i', str(SHARINGS)], check=True)
        assert digest(root / ACTOR) == SHARED_ACTOR
        subprocess.run(['patch', '-t', '-F', '0', '-p1', '-d', directory, '-i', str(PATCH)], check=True)
        assert not list(root.rglob('*.rej')) and not list(root.rglob('*.orig'))
        assert digest(root / HEADER) == PATCHED_HEADER and digest(root / ACTOR) == PATCHED_ACTOR

        actor = (root / ACTOR).read_text()
        # Emscripten compiles no thread, lock or worker: every deferral helper reduces to "in place".
        wasm = single_threaded(actor)
        for native_only in ('std::thread', 'std::mutex', 'std::condition_variable', '<thread>', '<mutex>',
                            'pthread', 'fesetenv', 'workerLoop', 'submitDeferredFix', 'drainDeferredFixes'):
            assert native_only not in wasm, native_only
        # At most two workers; a transfer already under way keeps its setting.
        setter = between(actor, 'void STEPControl_ActorRead::SetDeferredFixWorkers(', '\n}\n')
        assert 'std::max(0, std::min(theNbWorkers, 2))' in setter and 'myTransferDepth != 0' in setter
        # Only a manifold solid brep whose entities no pending completion holds is deferred.
        git = between(actor, 'occ::handle<TransferBRep_ShapeBinder> STEPControl_ActorRead::TransferEntity(\n'
                      '  const occ::handle<StepGeom_GeometricRepresentationItem>&', '\n}\n')
        decide = between(git, 'bool                                   aToDefer = false;', '#endif')
        assert 'occ::down_cast<StepShape_ManifoldSolidBrep>(start)' in decide
        assert 'isManifold && collectSolidEntities(aSolid, aSolidEntities)' in decide
        shared = between(decide, 'PendingEntities.count(anEntity) != 0', 'break;')
        assert 'aToDefer = false;' in shared and '++myDeferredFixes->NbShared;' in shared
        # Everything else that builds a shape waits for the pending completions first,
        # also once the transfer has diverged and defers nothing more.
        drain = 'if (myDeferredFixes && !aToDefer && !myDeferredFixes->Pending.empty())'
        assert decide.index(drain) < decide.index('drainDeferredFixes(*myDeferredFixes, TP);')
        assert 'deferredFixesActive(' not in decide[decide.index(drain):]
        # The in-place completion and merge are unchanged and still run whenever nothing was deferred.
        inplace = git[git.index('if (!aDeferred)'):]
        assert inplace.index('aShapeProcessor.ProcessShape(mappedShape, GetProcessingFlags().first, aPS.Next())') < inplace.index('MergeTransferInfo(TP, nbTPitems)')
        # A deferred completion is merged, bounded to its own transfer items, on the transfer thread.
        final = between(actor, 'void finalizeDeferredFix(STEPControl_ActorReadDeferredFixes&', '\n}\n')
        assert 'mergeTransferInfoRange(' in final and 'Diverged = true' in final
        assert 'theJob.Result.Nullify();' in final and 'theJob.Context.Nullify();' in final
        # A diverged transfer awaits every pending completion and defers nothing more.
        finalize = between(actor, 'void finalizeDeferredFixes(STEPControl_ActorReadDeferredFixes&', '\n}\n')
        assert 'theFixes.Pending.size() <= theToKeep && !theFixes.Diverged' in finalize
        active = between(actor, 'bool deferredFixesActive(', '\n}\n')
        assert '!theFixes->Diverged' in active
        # At most two pending completions per worker; finished ones are finalized as the transfer goes.
        submit = between(actor, 'bool submitDeferredFix(STEPControl_ActorReadDeferredFixes&', '\n}\n')
        assert 'finalizeDeferredFixes(theFixes, theTP, 2 * static_cast<size_t>(theFixes.Workers) - 1);' in submit
        assert 'std::fegetenv(&theJob->FloatEnvironment)' in submit
        # Workers get an 8 MiB stack and the submitting thread's floating-point environment.
        assert 'pthread_attr_setstacksize(&anAttributes, size_t(8) << 20)' in actor
        assert 'std::fesetenv(&aJob->FloatEnvironment);' in between(actor, 'void workerLoop(', '\n}\n')
        # Every lookup of a transfer result goes through settledFind, which waits for
        # the pending completions when one of them bound the entity.
        members = actor[actor.index('// Method  : STEPControl_ActorRead::STEPControl_ActorRead  ()'):]
        assert not re.search(r'\b(?:the)?TP->Find\(', members)
        settled = between(actor, 'occ::handle<Transfer_Binder> settledFind(', '\n}\n')
        assert 'boundByPendingFix(' in settled and 'drainDeferredFixes(' in settled
        # The item loop of a shape representation reads a non-stand-in result after the pending completions.
        loop = between(actor, 'binder  = settledFind(myDeferredFixes, TP, anitem);', 'OneResult = theResult;')
        assert 'theResult = TransferBRep::ShapeResult(binder);' in loop[loop.index('drainDeferredFixes(*myDeferredFixes, TP);'):]
        # The sharing walk refuses the voids it cannot reach, and reaches vertices and poly loops.
        walk = between(actor, 'bool collectSolidEntities(', '\n}\n')
        assert 'StepShape_FacetedBrepAndBrepWithVoids' in walk and 'return false;' in walk
        assert 'StepShape_PolyLoop' in walk and 'LoopVertex()' in walk
        assert 'theEdge->EdgeStart(), theEdge->EdgeEnd()' in between(actor, 'void collectEdgeEntities(', '\n}\n')
        assert re.search(r'TopoDS_Builder\(\)\.Add\(|aBuilder\.Add\(', actor)

    print('STEP deferred solid fix: pinned patch after the sharings edit; hashes and guards pass')


if __name__ == '__main__':
    main()
