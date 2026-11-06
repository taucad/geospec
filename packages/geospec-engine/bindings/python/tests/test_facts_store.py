"""The persisted facts store the pytest plugin opens by default."""

from __future__ import annotations

import os

import pytest

from geospec import _facts_store, pytest_plugin


def test_default_root_is_keyed_on_the_engine_version(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(_facts_store, "_engine_version", lambda: "9.8.7")
    assert _facts_store.default_facts_store_root({"XDG_CACHE_HOME": "/xdg"}) == os.path.join(
        "/xdg", "geospec", "9.8.7"
    )
    fallback = os.path.join(os.path.expanduser("~"), ".cache", "geospec", "9.8.7")
    assert _facts_store.default_facts_store_root({}) == fallback
    assert _facts_store.default_facts_store_root({"XDG_CACHE_HOME": "relative"}) == fallback


def test_no_home_directory_means_no_default_store(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(_facts_store, "_engine_version", lambda: "1.0.0")
    monkeypatch.setattr(os.path, "expanduser", lambda path: path)
    assert _facts_store.resolve_facts_store({}) is None
    assert _facts_store.resolve_facts_store({"XDG_CACHE_HOME": "relative"}) is None
    assert _facts_store.resolve_facts_store({"XDG_CACHE_HOME": "/xdg"}) == (
        os.path.join("/xdg", "geospec", "1.0.0"),
        False,
    )


def test_test_sessions_never_use_the_home_store() -> None:
    assert os.environ["GEOSPEC_FACTS_STORE"] == "off"


def test_variable_selects_default_none_or_an_absolute_root(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(_facts_store, "_engine_version", lambda: "1.0.0")
    assert _facts_store.resolve_facts_store({"XDG_CACHE_HOME": "/xdg"}) == (
        os.path.join("/xdg", "geospec", "1.0.0"),
        False,
    )
    assert _facts_store.resolve_facts_store({"GEOSPEC_FACTS_STORE": "off"}) is None
    assert _facts_store.resolve_facts_store({"GEOSPEC_FACTS_STORE": "/store"}) == ("/store", True)
    with pytest.raises(ValueError, match="absolute directory"):
        _facts_store.resolve_facts_store({"GEOSPEC_FACTS_STORE": "store"})


class _Recorder:
    """Stands in for GeoSpecEngine; refuses every store when ``refuse`` is set."""

    def __init__(self, refuse: bool) -> None:
        self.refuse = refuse
        self.calls: list[dict[str, object]] = []

    def __call__(self, **options: object) -> dict[str, object]:
        self.calls.append(options)
        if self.refuse and "cache_root" in options:
            raise ValueError("Persistent evidence requires the verified release producer profile.")
        return options


def test_plugin_opens_the_default_store_and_runs_without_it_when_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("XDG_CACHE_HOME", "/xdg")
    monkeypatch.delenv("GEOSPEC_FACTS_STORE", raising=False)
    monkeypatch.setattr(_facts_store, "_engine_version", lambda: "1.0.0")
    opened = _Recorder(refuse=False)
    monkeypatch.setattr(pytest_plugin, "GeoSpecEngine", opened)
    assert pytest_plugin._default_engine("/project") == {
        "model_root": "/project",
        "cache_root": os.path.join("/xdg", "geospec", "1.0.0"),
        "project_root": "/project",
        "facts_only": True,
    }
    refused = _Recorder(refuse=True)
    monkeypatch.setattr(pytest_plugin, "GeoSpecEngine", refused)
    assert pytest_plugin._default_engine("/project") == {"model_root": "/project"}
    assert len(refused.calls) == 2


def test_plugin_raises_for_a_named_store_and_skips_an_off_store(monkeypatch: pytest.MonkeyPatch) -> None:
    refused = _Recorder(refuse=True)
    monkeypatch.setattr(pytest_plugin, "GeoSpecEngine", refused)
    monkeypatch.setenv("GEOSPEC_FACTS_STORE", "/named")
    with pytest.raises(ValueError, match="verified release producer profile"):
        pytest_plugin._default_engine("/project")
    monkeypatch.setenv("GEOSPEC_FACTS_STORE", "off")
    assert pytest_plugin._default_engine("/project") == {"model_root": "/project"}
