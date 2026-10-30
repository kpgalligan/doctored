"""Black-box history gates: retained states, dirty identity, selection and budgets."""

from phase5_support import App, editing_state, fixture, write_report


def history_flows(folder):
    folder.mkdir()
    source = folder / "history.png"
    fixture(source)
    with App(folder / "app", folder / "recovery", interval=60) as app:
        document = app.open(source)
        call = lambda tool_name, **arguments: app.call(tool_name, document_id=document, **arguments)
        baseline = call("get_document")
        initial = call("list_history")
        initial_id = initial["current_state_id"]
        assert initial["undo_count"] == 0, initial
        for index in range(1, 37):
            call("set_layer_properties", index=0, name=f"History {index}")
        history = call("list_history")
        assert len(history["states"]) >= 37 and history["undo_count"] >= 36, history
        assert not history["over_budget"] and "accounting_error" not in history, history
        last_id = history["current_state_id"]
        for _ in range(36):
            call("undo")
        restored = call("get_document")
        assert restored["layers"][0]["name"] == baseline["layers"][0]["name"], restored
        assert not restored["unsaved_changes"], restored
        assert call("list_history")["current_state_id"] == initial_id
        call("restore_history", state_id=last_id)
        assert call("get_document")["layers"][0]["name"] == "History 36"
        assert call("get_document")["unsaved_changes"]
        assert len(call("list_history")["states"]) == len(history["states"])
        call("restore_history", state_id=initial_id)
        assert not call("get_document")["unsaved_changes"]
        print("PASS: more than 24 distinct edits undo to saved state; history navigation preserves saved/dirty identity", flush=True)

        # Starting a new branch after navigation drops the old redo tail.
        call("select_rect", x=7, y=5, width=39, height=28)
        # Three different masks leave an older buffer owned by history, beyond
        # the current/previous masks that belong to the active-state baseline.
        for _ in range(3):
            call("modify_selection", operation="feather", radius=2)
        selection_state = call("get_document")["selection"]
        selected = call("list_history")
        assert selected["redo_count"] == 0, selected
        assert not call("get_document")["unsaved_changes"]
        assert selected["states"][-1]["selection_only"], selected
        assert selected["incremental_history_bytes"] > 0, selected
        for _ in range(3):
            call("undo")
        assert call("get_document")["selection"]["kind"] == "rect"
        call("undo")
        assert "selection" not in call("get_document")
        for _ in range(4):
            call("redo")
        assert call("get_document")["selection"] == selection_state
        assert not call("get_document")["unsaved_changes"]
        before_noop = call("list_history")
        call("set_layer_properties", index=0, name=baseline["layers"][0]["name"])
        assert call("list_history")["current_state_id"] == before_noop["current_state_id"]
        call("restore_history", state_id="not-a-state", fail=True)
        assert call("list_history")["current_state_id"] == before_noop["current_state_id"]
        print("PASS: selection-only states consume history memory, preserve clean status, redo exactly and reject missing IDs", flush=True)

        call("deselect")
        call("set_history_budget", bytes=1)
        call("apply_filter", layer=0, filter="invert")
        call("apply_filter", layer=0, filter="invert")
        limited = call("list_history")
        assert limited["budget_bytes"] == 1 and limited["over_budget"], limited
        assert limited["undo_count"] >= 1 and len(limited["states"]) == 2, limited
        assert limited["evicted_states"] > 0 and limited["single_edit_policy"], limited
        saved = editing_state(app, document)
        call("undo")
        assert editing_state(app, document)["composite"] != saved["composite"]
        call("redo")
        assert editing_state(app, document)["composite"] == saved["composite"]
        call("set_history_budget", bytes=-1, fail=True)
        write_report(folder / "history-metrics.json", {"many_edits": history, "selection": selected, "limited": limited})
        print("PASS: budget eviction retains the nearest undo transition with an explicit over-budget exception", flush=True)


def grouped_action_history(folder):
    folder.mkdir()
    source = folder / "action.png"
    fixture(source)
    with App(folder / "app", folder / "recovery", interval=60) as app:
        document = app.open(source)
        call = lambda tool_name, **arguments: app.call(tool_name, document_id=document, **arguments)
        call("start_recording")
        call("select_rect", x=5, y=7, width=37, height=31)
        call("modify_selection", operation="feather", radius=2)
        call("apply_filter", layer=0, filter="invert")
        call("stop_recording", name="Phase Five Mixed Action")
        replay_source = folder / "replay.png"
        replay_source.write_bytes(source.read_bytes())
        replay = app.open(replay_source)
        initial = app.call("list_history", document_id=replay)
        app.call("run_action", document_id=replay, name="Phase Five Mixed Action")
        final = app.call("list_history", document_id=replay)
        assert final["undo_count"] == initial["undo_count"] + 1, final
        assert editing_state(app, replay) == editing_state(app, document)
        app.call("undo", document_id=replay)
        assert not app.call("get_document", document_id=replay)["unsaved_changes"]
        assert "selection" not in app.call("get_document", document_id=replay)
        app.call("redo", document_id=replay)
        assert editing_state(app, replay) == editing_state(app, document)
        print("PASS: mixed pixel/selection Action replay creates one history transition with exact undo/redo", flush=True)
