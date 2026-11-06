"""Where a Python engine keeps its persisted facts store (``geospec-facts-v1``)."""

from __future__ import annotations

from collections.abc import Mapping
from importlib.metadata import PackageNotFoundError, version
import os

FACTS_STORE_VARIABLE = "GEOSPEC_FACTS_STORE"
"""Unset uses the default root, ``off`` opens no store, an absolute path replaces the root."""


def _engine_version() -> str:
    try:
        return version("geospec-engine")
    except PackageNotFoundError:
        return "0.0.0"


def default_facts_store_root(environ: Mapping[str, str] | None = None) -> str:
    """Return ``$XDG_CACHE_HOME/geospec/<engine version>``, with ``~/.cache`` when unset or relative."""

    environment = os.environ if environ is None else environ
    configured = environment.get("XDG_CACHE_HOME", "")
    base = configured if os.path.isabs(configured) else os.path.join(os.path.expanduser("~"), ".cache")
    return os.path.join(base, "geospec", _engine_version())


def resolve_facts_store(environ: Mapping[str, str] | None = None) -> tuple[str, bool] | None:
    """Return the store root and whether it was named explicitly, or ``None`` when the store is off."""

    environment = os.environ if environ is None else environ
    selected = environment.get(FACTS_STORE_VARIABLE, "")
    if selected == "off":
        return None
    if selected == "":
        return default_facts_store_root(environment), False
    if not os.path.isabs(selected):
        raise ValueError(f"{FACTS_STORE_VARIABLE} must be 'off' or an absolute directory, not {selected!r}")
    return selected, True
