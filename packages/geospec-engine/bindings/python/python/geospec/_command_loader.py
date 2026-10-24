"""`command_loader`: the Python client of GeoSpec subprocess protocol 1.

Protocol 1: GeoSpec runs ``<command…> --file <path> --format <glb|step> --project <root> --out <dir>``,
adding ``--parameters-json <json>``, ``--mesh-linear-tolerance <n>`` and
``--mesh-angular-tolerance-degrees <n>`` when the request carries them. The command writes the artifact
under ``--out`` and prints one JSON document on stdout: on success (exit 0)
``{ protocol: 1, format, sourceUnit?, primary, resources?: [{ name, path }], consumedSources }``, on
failure (non-zero exit) ``{ protocol: 1, error: { code, message, suggestion? } }``. TypeScript's
``commandExporter`` in ``geospec/model`` is the same protocol from the host side.
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import tempfile

from ._api import GeoSpecAssertionError, _active_engine, _wire_value
from ._model import (
    GeoSpecLoadModelOptions,
    GeoSpecModelArtifact,
    GeoSpecModelFormat,
    GeoSpecModelLoader,
    GeoSpecModelUnit,
    _active_scope,
)

_PROTOCOL_FORMATS = ("glb", "step")
_UNITS: tuple[GeoSpecModelUnit, ...] = ("mm", "cm", "m", "in", "ft", "auto")
# Request fields protocol 1 cannot carry; the loader refuses them rather than export without them.
_UNCARRIED = ("code", "source", "resources", "mesh", "step_streaming")
_STDERR_TAIL = 2000
_SHA256 = re.compile(r"[0-9a-f]{64}")


class _ExportFailed(Exception):
    """A protocol violation by the command, reported as GEOSPEC_EXPORTER_FAILED."""

    def __init__(self, reason: str, stderr: str = "") -> None:
        super().__init__(reason)
        self.reason = reason
        self.stderr = stderr


def _number(flag: str, value: object) -> str:
    """Format a tolerance as JavaScript's String(n) would, refusing anything but a finite number."""

    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise TypeError(f"{flag} must be a number.")
    if isinstance(value, int):
        return str(value)
    if not math.isfinite(value):
        raise GeoSpecAssertionError(f"{flag} must be a finite number.", code="invalid-field")
    return str(int(value)) if value.is_integer() and abs(value) < 1e21 else repr(value)


def _arguments(options: GeoSpecLoadModelOptions, file: str, format: str, project: Path, out: Path) -> list[str]:
    arguments = ["--file", file, "--format", format, "--project", str(project), "--out", str(out)]
    if "parameters" in options:
        wire = _wire_value(options["parameters"])
        if not isinstance(wire, dict):
            raise TypeError("Model parameters must be a mapping.")
        arguments += ["--parameters-json", json.dumps(wire, separators=(",", ":"), ensure_ascii=False, allow_nan=False)]
    if "mesh_linear_tolerance" in options:
        arguments += ["--mesh-linear-tolerance", _number("mesh_linear_tolerance", options["mesh_linear_tolerance"])]
    if "mesh_angular_tolerance_degrees" in options:
        arguments += [
            "--mesh-angular-tolerance-degrees",
            _number("mesh_angular_tolerance_degrees", options["mesh_angular_tolerance_degrees"]),
        ]
    return arguments


def _text(document: Mapping[str, object], key: str, *, optional: bool = False) -> str | None:
    value = document.get(key)
    if value is None and optional and key not in document:
        return None
    if not isinstance(value, str) or value == "":
        raise _ExportFailed("its JSON document is not a protocol 1 response.")
    return value


def _no_constant(token: str) -> object:
    raise ValueError(f"JSON has no {token}.")


def _is_protocol_1(document: object) -> bool:
    return isinstance(document, dict) and type(document.get("protocol")) is int and document["protocol"] == 1


def _refusal(document: object) -> GeoSpecAssertionError | None:
    """The command's own typed refusal, carried with its code, or None when the document is not one."""

    if not _is_protocol_1(document):
        return None
    assert isinstance(document, dict)
    error = document.get("error")
    if not isinstance(error, dict):
        return None
    code, message, suggestion = error.get("code"), error.get("message"), error.get("suggestion", None)
    if not isinstance(code, str) or code == "" or not isinstance(message, str):
        return None
    if "suggestion" in error and not isinstance(suggestion, str):
        return None
    return GeoSpecAssertionError(message if suggestion is None else f"{message}\nSuggestion: {suggestion}", code=code)


def _under(directory: Path, path: str) -> Path:
    """Resolve a path the command printed under ``directory``, refusing one that leaves it."""

    try:
        target = (directory / path).resolve()
    except ValueError:
        raise _ExportFailed(f"it named {path!r}, which is not a valid path.") from None
    if target == directory or not target.is_relative_to(directory):
        raise _ExportFailed(f"it named '{path}', which is outside its directory.")
    return target


def _read(path: Path, label: str) -> bytes:
    try:
        return path.read_bytes()
    except OSError as error:
        raise _ExportFailed(f"its {label} '{path.name}' could not be read ({error.strerror or error}).") from None
    except ValueError:
        raise _ExportFailed(f"its {label} {path.name!r} is not a valid file name.") from None


def _artifact(document: object, out: Path, project: Path) -> GeoSpecModelArtifact:
    if not _is_protocol_1(document):
        raise _ExportFailed("its JSON document is not a protocol 1 response.")
    assert isinstance(document, dict)
    format = _text(document, "format")
    if format not in _PROTOCOL_FORMATS:
        raise _ExportFailed("its JSON document is not a protocol 1 response.")
    unit = _text(document, "sourceUnit", optional=True)
    source_unit: GeoSpecModelUnit = "mm"
    if unit is not None:
        if unit not in _UNITS:
            raise _ExportFailed(f"it reported source unit '{unit}', which GeoSpec cannot admit.")
        source_unit = next(item for item in _UNITS if item == unit)
    primary_name = _text(document, "primary")
    resource_entries = document.get("resources", [])
    consumed_entries = document.get("consumedSources")
    if not isinstance(resource_entries, list) or not isinstance(consumed_entries, list):
        raise _ExportFailed("its JSON document is not a protocol 1 response.")
    if not all(isinstance(entry, dict) for entry in (*resource_entries, *consumed_entries)):
        raise _ExportFailed("its JSON document is not a protocol 1 response.")
    resource_names = [(_text(entry, "name"), _text(entry, "path")) for entry in resource_entries]
    consumed_names = [(_text(entry, "path"), _text(entry, "sha256")) for entry in consumed_entries]
    assert primary_name is not None
    primary = _read(_under(out, primary_name), "artifact")
    resources = tuple(
        (str(name), _read(_under(out, str(path)), "resource")) for name, path in resource_names
    )
    consumed: list[tuple[str, bytes]] = []
    for path, sha256 in consumed_names:
        assert path is not None and sha256 is not None
        source = _under(project, path)
        if sha256 == "missing":
            # The export looked for this file and did not find it. Python provenance records bytes, so an
            # absence has no entry; one that exists now was created during the export.
            if source.exists():
                raise _ExportFailed(f"consumed source '{path}' was reported missing but exists; it changed during the export.")
            continue
        digest = sha256.removeprefix("sha256:").lower()
        if _SHA256.fullmatch(digest) is None:
            raise _ExportFailed("its JSON document is not a protocol 1 response.")
        content = _read(source, "consumed source")
        if hashlib.sha256(content).hexdigest() != digest:
            raise _ExportFailed(
                f"consumed source '{path}' no longer has the sha256 the command reported; it changed during the export."
            )
        consumed.append((path, content))
    exported: GeoSpecModelFormat = "glb" if format == "glb" else "step"
    return GeoSpecModelArtifact(primary, exported, source_unit, resources, tuple(consumed))


def _project_root() -> Path:
    engine = _active_engine.get()
    if engine is None:
        scope = _active_scope.get()
        engine = None if scope is None else scope.engine
    if engine is None or engine._model_root is None:
        raise GeoSpecAssertionError(
            "command_loader runs inside load_model on a GeoSpec host with a model_root.",
            code="GEOSPEC_MODEL_HOST_UNAVAILABLE",
        )
    return Path(engine._model_root).resolve()


def command_loader(
    command: Sequence[str],
    *,
    cwd: str | None = None,
    env: Mapping[str, str] | None = None,
    timeout: float | None = None,
) -> GeoSpecModelLoader:
    """Return a ``model_loader`` that exports each code model by running a protocol 1 command.

    ``command`` is the executable and its leading arguments, such as ``["tau", "export"]``. Each call
    runs it once with ``--project`` set to the host's ``model_root`` and a fresh ``--out`` directory,
    then returns the artifact it wrote with the bytes of every consumed source, each checked against
    the sha256 the command reported (64 hex digits, optionally prefixed ``sha256:``). A source reported
    ``missing`` must still be absent and has no provenance entry. ``cwd`` defaults to ``model_root``; ``env`` is merged over this
    process's environment; ``timeout`` is the seconds one export may run before the command is killed.

    The command's error document raises ``GeoSpecAssertionError`` with the document's own code. A
    command that cannot start, overruns ``timeout``, prints no protocol 1 document, or names a file
    outside its directories raises ``GEOSPEC_EXPORTER_FAILED``; a request protocol 1 cannot carry
    (inline ``code``, ``resources``, ``mesh``, ``step_streaming``, a format other than ``glb`` and
    ``step``) raises ``GEOSPEC_EXPORTER_UNSUPPORTED`` before the command runs.
    """

    if isinstance(command, (str, bytes)) or not isinstance(command, Sequence):
        raise TypeError("command_loader needs the command as a sequence of arguments.")
    arguments = list(command)
    if not arguments or not all(isinstance(argument, str) for argument in arguments) or arguments[0] == "":
        raise TypeError("command_loader needs a command whose executable and arguments are strings.")
    if env is not None and not all(isinstance(key, str) and isinstance(value, str) for key, value in env.items()):
        raise TypeError("command_loader env must map strings to strings.")
    if timeout is not None and (
        isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or timeout <= 0
    ):
        raise ValueError("command_loader timeout must be a positive finite number of seconds.")
    name = " ".join(arguments)
    environment = None if env is None else {**os.environ, **env}

    def load(options: GeoSpecLoadModelOptions) -> GeoSpecModelArtifact:
        uncarried = sorted(key for key in _UNCARRIED if key in options)
        if uncarried:
            raise GeoSpecAssertionError(
                f"Exporter command '{name}' cannot honor {', '.join(uncarried)}: protocol 1 passes one file and its export options only.",
                code="GEOSPEC_EXPORTER_UNSUPPORTED",
            )
        file = options.get("file")
        if not isinstance(file, str) or file == "":
            raise TypeError("command_loader exports a file; pass file to load_model.")
        format = options.get("format", "glb")
        if format not in _PROTOCOL_FORMATS:
            raise GeoSpecAssertionError(
                f"Exporter command '{name}' cannot export format '{format}': protocol 1 carries glb and step.",
                code="GEOSPEC_EXPORTER_UNSUPPORTED",
            )
        project = _project_root()
        with tempfile.TemporaryDirectory(prefix="geospec-export-", ignore_cleanup_errors=True) as directory:
            out = Path(directory).resolve()
            stderr = ""
            try:
                try:
                    completed = subprocess.run(
                        [*arguments, *_arguments(options, file, format, project, out)],
                        cwd=cwd if cwd is not None else project,
                        env=environment,
                        stdin=subprocess.DEVNULL,
                        capture_output=True,
                        timeout=timeout,
                        check=False,
                    )
                except subprocess.TimeoutExpired:
                    raise _ExportFailed(f"it ran longer than {timeout} s and was killed.") from None
                except OSError as error:
                    raise _ExportFailed(f"the command could not run ({error.strerror or error}).") from None
                stderr = completed.stderr.decode("utf-8", errors="replace")[-_STDERR_TAIL:]
                exit = completed.returncode
                ended = f"exit {exit}" if exit >= 0 else f"signal {-exit}"
                try:
                    document = json.loads(completed.stdout.decode("utf-8"), parse_constant=_no_constant)
                except ValueError:
                    raise _ExportFailed(f"it printed no protocol 1 JSON document ({ended}).", stderr) from None
                refusal = _refusal(document)
                if refusal is not None:
                    raise refusal
                if exit != 0:
                    raise _ExportFailed(f"it ended with {ended} without a protocol 1 error.", stderr)
                return _artifact(document, out, project)
            except _ExportFailed as failure:
                tail = failure.stderr or stderr
                raise GeoSpecAssertionError(
                    f"Exporter command '{name}' failed to export '{file}': {failure.reason}"
                    + (f"\nstderr: {tail}" if tail else "")
                    + "\nSuggestion: Run the exporter command by hand with the same arguments and fix what it reports.",
                    code="GEOSPEC_EXPORTER_FAILED",
                ) from None

    return load
