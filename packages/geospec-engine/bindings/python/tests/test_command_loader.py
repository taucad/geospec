"""Subprocess protocol 1 client: a fake exporter command drives every success, protocol error and refusal."""

import hashlib
import json
from pathlib import Path
import sys

import pytest

import geospec
from geospec._model import _ModelScope, _active_scope
from test_canonical_loading import HostEngine

EXPORTER = [sys.executable, "-I", str(Path(__file__).with_name("protocol_exporter.py"))]


@pytest.fixture
def project(tmp_path):
    root = tmp_path / "project"
    (root / "lib").mkdir(parents=True)
    (root / "main.ts").write_text("export default model;")
    (root / "lib" / "jaw.ts").write_text("export const jaw = 1;")
    return root


def host(project, mode, **options):
    """An engine whose model_loader runs the fixture exporter in `mode`; the record lands beside the project."""

    record = project.parent / "record.json"
    env = {"GEOSPEC_FIXTURE_MODE": mode, "GEOSPEC_FIXTURE_RECORD": str(record), **options.pop("env", {})}
    native = HostEngine()
    engine = geospec.GeoSpecEngine(
        native_engine=native,
        model_root=str(project),
        model_loader=geospec.command_loader(EXPORTER, env=env, **options),
    )
    return engine, native, record


def refusal(project, mode, **request):
    engine, native, _ = host(project, mode)
    with engine, pytest.raises(geospec.GeoSpecAssertionError) as raised:
        geospec.load_model(file="main.ts", **request)
    assert native.requests == [], "a refused export must never reach the engine"
    return raised.value


def test_success_passes_every_request_field_as_a_protocol_flag_and_reads_the_artifact(project):
    engine, native, record = host(project, "success")
    with engine:
        subject = geospec.load_model(
            file="main.ts",
            parameters={"jawWidth": 125, "label": "vise"},
            mesh_linear_tolerance=0.01,
            mesh_angular_tolerance_degrees=0.5,
            ingest_options={"name": "vise"},
        )
    received = json.loads(record.read_text())
    args = received["args"]
    out = args[args.index("--out") + 1]
    assert args == [
        "--file", "main.ts", "--format", "glb", "--project", str(project.resolve()), "--out", out,
        "--parameters-json", '{"jawWidth":125,"label":"vise"}',
        "--mesh-linear-tolerance", "0.01", "--mesh-angular-tolerance-degrees", "0.5",
    ]
    assert received["cwd"] == str(project.resolve())
    assert not Path(out).exists(), "each export's output directory is removed afterwards"
    request, primary, resources = native.requests[0]
    assert primary == b"\x01\x02\x03"
    assert resources == [b"\x09"]
    assert request["frame"]["sourceUnit"] == "cm"
    assert request["ingestOptions"] == {"name": "vise"}
    sources = subject.provenance["sources"]
    assert [source["name"] for source in sources] == ["main.ts", "lib/jaw.ts"]
    assert sources[0]["sha256"] == hashlib.sha256(b"export default model;").hexdigest()
    assert subject.provenance["resources"][0]["name"] == "texture.png"


def test_bare_document_defaults_to_millimetres_and_omits_absent_flags(project):
    engine, native, record = host(project, "bare", cwd=str(project.parent), env={"GEOSPEC_FIXTURE_ENV": "merged"})
    with engine:
        geospec.load_model(file="main.ts", format="step")
    received = json.loads(record.read_text())
    assert received["args"][:6] == ["--file", "main.ts", "--format", "step", "--project", str(project.resolve())]
    assert len(received["args"]) == 8
    assert received["cwd"] == str(project.parent)
    assert received["env"] == "merged"
    assert native.requests[0][0]["frame"]["sourceUnit"] == "mm"
    assert native.requests[0][1] == b"step"


def test_whole_float_tolerances_print_as_javascript_numbers(project):
    engine, _, record = host(project, "success")
    with engine:
        geospec.load_model(file="main.ts", mesh_linear_tolerance=1.0, mesh_angular_tolerance_degrees=15)
    args = json.loads(record.read_text())["args"]
    assert args[-4:] == ["--mesh-linear-tolerance", "1", "--mesh-angular-tolerance-degrees", "15"]


def test_prefixed_digests_verify_and_missing_sources_stay_absent(project):
    engine, _, _ = host(project, "prefixed-and-missing")
    with engine:
        subject = geospec.load_model(file="main.ts")
    assert [source["name"] for source in subject.provenance["sources"]] == ["main.ts"]


def test_pytest_scope_supplies_the_project_root(project):
    engine, native, record = host(project, "success")
    scope = _ModelScope(lambda: engine)
    token = _active_scope.set(scope)
    try:
        geospec.load_model(file="main.ts")
    finally:
        _active_scope.reset(token)
        scope.close()
    args = json.loads(record.read_text())["args"]
    assert args[args.index("--project") + 1] == str(project.resolve())
    assert len(native.requests) == 1


@pytest.mark.parametrize("mode", ["refuse", "refuse-exit-0"])
def test_error_document_raises_with_the_command_code(project, mode):
    error = refusal(project, mode)
    if mode == "refuse":
        assert error.code == "TAU_KERNEL_MISSING"
        assert str(error) == "No kernel for main.kcl.\nSuggestion: Add zoo()."
    else:
        assert error.code == "FIXTURE_REFUSED"
        assert str(error) == "The fixture refused."


@pytest.mark.parametrize(
    ("mode", "reason"),
    [
        ("garbage", "printed no protocol 1 JSON document (exit 3)"),
        ("nan", "printed no protocol 1 JSON document (exit 0)"),
        ("exit-1-success", "ended with exit 1 without a protocol 1 error"),
        ("bad-error", "ended with exit 1 without a protocol 1 error"),
        ("signal", "printed no protocol 1 JSON document (signal 9)"),
        ("protocol-2", "is not a protocol 1 response"),
        ("protocol-true", "is not a protocol 1 response"),
        ("unknown-format", "is not a protocol 1 response"),
        ("null-unit", "is not a protocol 1 response"),
        ("bad-resources", "is not a protocol 1 response"),
        ("bad-unit", "source unit 'furlong'"),
        ("escape-primary", "'../outside.glb', which is outside its directory"),
        ("absolute-primary", "which is outside its directory"),
        ("symlink-primary", "'link.glb', which is outside its directory"),
        ("escape-resource", "which is outside its directory"),
        ("missing-primary", "artifact 'absent.glb' could not be read"),
        ("escape-source", "'../outside.ts', which is outside its directory"),
        ("stale-source", "consumed source 'main.ts' no longer has the sha256"),
        ("missing-exists", "consumed source 'main.ts' was reported missing but exists"),
        ("bad-digest", "is not a protocol 1 response"),
    ],
)
def test_protocol_violations_raise_exporter_failed(project, mode, reason):
    error = refusal(project, mode)
    assert error.code == "GEOSPEC_EXPORTER_FAILED"
    assert "failed to export 'main.ts'" in str(error)
    assert reason in str(error)


def test_exporter_failure_carries_the_stderr_tail(project):
    assert "stderr: fixture stderr" in str(refusal(project, "garbage"))


def test_command_that_cannot_start_raises_exporter_failed(project):
    native = HostEngine()
    loader = geospec.command_loader([str(project / "no-such-exporter")])
    with geospec.GeoSpecEngine(native_engine=native, model_root=str(project), model_loader=loader):
        with pytest.raises(geospec.GeoSpecAssertionError) as raised:
            geospec.load_model(file="main.ts")
    assert raised.value.code == "GEOSPEC_EXPORTER_FAILED"
    assert "could not run" in str(raised.value)


def test_command_that_overruns_its_timeout_is_killed(project):
    engine, native, _ = host(project, "hang", timeout=0.5)
    with engine, pytest.raises(geospec.GeoSpecAssertionError) as raised:
        geospec.load_model(file="main.ts")
    assert raised.value.code == "GEOSPEC_EXPORTER_FAILED"
    assert "ran longer than 0.5 s" in str(raised.value)
    assert native.requests == []


def test_engine_still_refuses_an_export_without_source_identity_or_of_another_format(project):
    assert refusal(project, "no-sources").code == "GEOSPEC_MODEL_SOURCE_IDENTITY_MISSING"
    assert refusal(project, "other-format").code == "GEOSPEC_MODEL_EXPORT_INTENT_MISMATCH"


@pytest.mark.parametrize(
    "request_options",
    [
        {"code": {"main.ts": "model"}},
        {"mesh": False},
        {"step_streaming": "filesystem"},
        {"resources": [{"name": "texture.png", "source": b"\x09"}]},
        {"format": "gltf"},
        {"format": "stp"},
    ],
)
def test_requests_protocol_1_cannot_carry_refuse_before_the_command_runs(project, request_options):
    engine, native, record = host(project, "success")
    with engine, pytest.raises(geospec.GeoSpecAssertionError) as raised:
        geospec.load_model(file="main.ts", **request_options)
    assert raised.value.code == "GEOSPEC_EXPORTER_UNSUPPORTED"
    assert not record.exists(), "the command must not run"
    assert native.requests == []


def test_loader_outside_a_rooted_host_refuses():
    loader = geospec.command_loader(EXPORTER)
    with pytest.raises(geospec.GeoSpecAssertionError) as raised:
        loader({"file": "main.ts"})
    assert raised.value.code == "GEOSPEC_MODEL_HOST_UNAVAILABLE"


@pytest.mark.parametrize(
    ("command", "options", "error"),
    [
        ("tau export", {}, TypeError),
        ([], {}, TypeError),
        ([""], {}, TypeError),
        (["tau", 1], {}, TypeError),
        (["tau"], {"env": {"KEY": 1}}, TypeError),
        (["tau"], {"timeout": 0}, ValueError),
        (["tau"], {"timeout": float("inf")}, ValueError),
        (["tau"], {"timeout": True}, ValueError),
    ],
)
def test_incomplete_options_raise_at_construction(command, options, error):
    with pytest.raises(error):
        geospec.command_loader(command, **options)
