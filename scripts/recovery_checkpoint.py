"""A live document's recovery checkpoint as a native file (13C-3).

A recovery generation stores its state as a tile-pool skeleton (`image.drskel`) beside the
recovery's `pool`, not as an `image.rz`, and an open document's recovery is leased, so
`recover_document` refuses it. `write_recovery`'s `native_copy` rebuilds the checkpoint it
just wrote into a native file: the same rebuild `recover_document` runs.
"""

from pathlib import Path


def checkpoint_native(app, document, path):
    """Checkpoints `document` and answers the checkpoint rebuilt at `path`."""
    reply = app.call('write_recovery', document_id=document, wait=True, native_copy=str(path))
    assert Path(reply.get('native_copy', '')).resolve() == Path(path).resolve(), reply
    assert Path(path).is_file(), ('write_recovery wrote no native copy', reply)
    return Path(path)
