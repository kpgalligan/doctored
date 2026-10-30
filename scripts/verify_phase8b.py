"""`source_call`: an MCP tool call that waits through the shared worker's busy refusal.

A helper module, not a drive. The Phase 8B linked-source drive that lived here
went with linked placed images on 2026-10-10
(`designs/convert-to-smart-object-status.md`); the RAW and retouch drives still
import `source_call`, and the drives' provenance lists still name this file.
"""

import json
import time


def source_call(app, document, tool, fail=False, **arguments):
    """Wait through the shared worker's explicit pre-scheduling busy refusal.

    Periodic recovery legitimately owns the worker between MCP calls. Other
    failures are never retried, and busy cannot satisfy a missing-file test.
    """
    deadline = time.monotonic() + 15
    while True:
        result = app.client.rpc('tools/call', {'name': tool, 'arguments': {
            'document_id': document, **arguments}})
        text = next((block['text'] for block in result.get('content', []) if block['type'] == 'text'), '')
        if result.get('isError') and 'Another background operation is running.' in text:
            app.source_busy_retries = getattr(app, 'source_busy_retries', 0) + 1
            assert time.monotonic() < deadline, (tool, 'worker stayed busy', text)
            time.sleep(0.03)
            continue
        assert bool(result.get('isError')) == fail, (tool, result)
        return text if fail else json.loads(text)
