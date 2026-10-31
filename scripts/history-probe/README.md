# Recovery, history and snapshot AppKit probe

Run `make rust project`, then:

```sh
python3 scripts/history-probe/verify.py --keep
```

The runner compiles the current app sources with its own entry point and private preferences domain. It uses synthetic documents, a dummy API key, and temporary Actions/recovery directories. The printed scratch path contains build/runtime logs and generated fixtures. macOS file coordination may require running outside a restricted tool sandbox; no user documents or existing app processes are used.

The probe exercises actual History, Snapshots and Recovery controls; full-state undo/redo and saved markers; long edit/selection sequences; allocation-owner release and budget exceptions; bookkeeping limits; recovery capacity and publication refusal; the Recovery off notice a refused capture shows in the editor and the recovery panel; real `NSDocument` Save races; save/close cleanup; late-worker invalidation; and discovery beside inaccessible/incomplete entries.

For forced termination, portable file corruption and the default recovery interval, build a test app and run:

```sh
DR_VERIFY_APP=/path/to/test/Doctored.app/Contents/MacOS/Doctored \
  python3 scripts/verify_phase5.py --keep
```

Use a copy of the app with a private bundle identifier to isolate its preference/recent-document domain. The MCP runner gives each instance a private scratch socket and temporary files and signals only the processes it launches.
