```json screen
{"id": "settings-models", "title": "Settings ▸ Models", "fixture": "sky", "window": "editor", "needs": ["models"],
 "steps": [{"ui": "run_modal_card", "args": {"card": "settings"}}],
 "captures": [{"name": "card", "of": "card", "tab": "Models"}]}
```

The Settings window on its Models tab. A hint says models download on demand from their official host
into ~/Library/Application Support/Doctored/Models and that each is checked against its published
SHA-256 before use. A scrolling list shows each model: Depth Anything V2 Small (Core ML, F16) reads
Ready with its size on disk, a Licence: Apache-2.0 link, its tier (clean code and weights, training
data encumbered) and a note on its training data, above Download and Cancel, greyed, and Delete. Sky
Segmentation, U²-Net small (Core ML) follows, Ready under an MIT licence. Close and Done close the
window.
