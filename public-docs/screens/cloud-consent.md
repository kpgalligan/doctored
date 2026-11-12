```json screen
{"id": "cloud-consent", "title": "The cloud consent card", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "run_modal_card", "args": {"card": "cloud_consent"}}],
 "captures": [{"name": "card", "of": "card"}]}
```

The Send to OpenAI? card. It says Generative Fill sends the area around your selection, the
selection itself and your prompt to OpenAI (gpt-image-2.5-sunburst) with your own API key, and puts
the picture it answers on a new layer. Three lines follow. Training: OpenAI does not train its models
on API inputs or outputs. Retention: OpenAI keeps API requests in its abuse-monitoring logs for up to
30 days. Billing: Each Generative Fill is billed by OpenAI to your own API key. A link to
developers.openai.com/api/docs/guides/your-data comes next, then a line saying that accepting records
your consent for these terms and that Settings ▸ Cloud revokes it. The buttons are Cancel and Accept
and Send.
