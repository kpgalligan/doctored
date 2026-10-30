"""The model store's folder rule, the Python twin of `ModelStore.modelDirectory`.

First-party ids were `rasterize/<name>` before the Doctored rename and are `doctored/<name>`
now. A store's `rasterize--<name>` folder is read in place, never moved: the folder is the new
id's when it exists, else the legacy id's when that exists, else the new id's. The drives and
admission measures read the read-only fixture store through this, so it matches the app.
"""

from pathlib import Path

PREFIX = 'doctored/'
LEGACY_PREFIX = 'rasterize/'


def legacy_id(model_id: str) -> str | None:
    """`rasterize/<name>` for `doctored/<name>`; None for every other id (`ModelCatalog.legacyID`)."""
    if not model_id.startswith(PREFIX) or len(model_id) == len(PREFIX):
        return None
    return LEGACY_PREFIX + model_id[len(PREFIX):]


def _folder(store: Path, model_id: str) -> Path:
    return Path(store) / model_id.replace('/', '--')


def model_folder(store: Path, model_id: str) -> Path:
    """The id's folder in `store`, by `ModelStore.modelDirectory`'s rule."""
    current = _folder(store, model_id)
    if current.exists():
        return current
    legacy = legacy_id(model_id)
    if legacy is not None and _folder(store, legacy).exists():
        return _folder(store, legacy)
    return current


def is_ready(store: Path, model_id: str) -> bool:
    """Any `<version>/verified.json` under the id's folder."""
    return any(path.is_file() for path in model_folder(store, model_id).glob('*/verified.json'))
