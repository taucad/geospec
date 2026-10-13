"""Session checks shared by the binding tests.

`--geospec-installed` qualifies an installed wheel: the session refuses to
start unless `geospec` and `geospec_engine_native` both import from the
active environment's site-packages, so a test run can never pass against the
source tree by accident.
"""

from pathlib import Path
import sysconfig

import pytest

SOURCE_ROOT = (Path(__file__).resolve().parents[1] / "python").resolve()
INSTALLED_LOCATIONS = pytest.StashKey[dict]()


def pytest_addoption(parser):
    parser.addoption(
        "--geospec-installed",
        action="store_true",
        default=False,
        help="Require geospec and geospec_engine_native from site-packages, never from the source tree.",
    )


def installed_module_problems(modules, site_directories, source_root):
    """Return one message per module that is not imported from an installed location."""
    problems = []
    for module in modules:
        location = Path(module.__file__).resolve()
        if location.is_relative_to(source_root):
            problems.append(f"{module.__name__} imports from the source tree: {location}")
        elif not any(location.is_relative_to(site) for site in site_directories):
            problems.append(f"{module.__name__} is not under site-packages: {location}")
    return problems


def pytest_configure(config):
    if not config.getoption("--geospec-installed"):
        return
    import geospec
    import geospec_engine_native

    site_directories = {
        Path(sysconfig.get_path(name)).resolve() for name in ("purelib", "platlib")
    }
    problems = installed_module_problems(
        (geospec, geospec_engine_native), site_directories, SOURCE_ROOT
    )
    if problems:
        raise pytest.UsageError("; ".join(problems))
    config.stash[INSTALLED_LOCATIONS] = {
        module.__name__: str(Path(module.__file__).resolve())
        for module in (geospec, geospec_engine_native)
    }


def pytest_report_header(config):
    locations = config.stash.get(INSTALLED_LOCATIONS, None)
    if locations is None:
        return None
    return [f"installed {name}: {path}" for name, path in sorted(locations.items())]
