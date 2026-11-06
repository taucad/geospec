"""Small pytest integration for the normal GeoSpec assertion API."""

from collections.abc import Callable, Iterator

import pytest

from ._api import GeoSpecEngine
from ._facts_store import resolve_facts_store
from ._model import _ModelScope, _active_scope


def _default_engine(root: str) -> GeoSpecEngine:
    """An engine rooted at ``root`` with the persisted facts store ``GEOSPEC_FACTS_STORE`` selects.

    The store serves the facts family only. The default store is an optimisation: a build without the verified producer
    profile, or a root that overlaps the project, runs without one. A store
    named in ``GEOSPEC_FACTS_STORE`` must open.
    """

    selected = resolve_facts_store()
    if selected is not None:
        store, explicit = selected
        try:
            return GeoSpecEngine(model_root=root, cache_root=store, project_root=root, facts_only=True)
        except ValueError:
            if explicit:
                raise
    return GeoSpecEngine(model_root=root)


@pytest.fixture
def geospec_engine_factory(pytestconfig: pytest.Config) -> Callable[[], GeoSpecEngine]:
    """Override this fixture once in conftest.py to configure the rooted host."""

    return lambda: _default_engine(str(pytestconfig.rootpath))


@pytest.fixture(autouse=True)
def _geospec_scope(geospec_engine_factory: Callable[[], GeoSpecEngine]) -> Iterator[_ModelScope]:
    """Bind a lazy test scope; tests without geometry never initialize an engine."""

    scope = _ModelScope(geospec_engine_factory)
    token = _active_scope.set(scope)
    try:
        yield scope
    finally:
        _active_scope.reset(token)
        scope.close()


@pytest.fixture
def geospec_engine(_geospec_scope: _ModelScope) -> GeoSpecEngine:
    """The same synchronous engine used by this test's canonical load_model."""

    return _geospec_scope.get_engine()
