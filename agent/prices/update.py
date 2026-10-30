#!/usr/bin/env python3
"""Regenerate agent/prices/anthropic.json from LiteLLM's price table — the
ONLY writer of that file (`make prices`). Standard library only.

    python3 agent/prices/update.py [--api-base URL]

It asks GitHub for the newest commit that touched LiteLLM's
`model_prices_and_context_window.json`, fetches the file AT that commit, and
keeps the entries whose `litellm_provider` is "anthropic", reduced to the
five rates pricing needs, in US dollars per million tokens:

    input_cost_per_token                      -> input
    output_cost_per_token                     -> output
    cache_read_input_token_cost               -> cache_read
    cache_creation_input_token_cost           -> cache_write_5m
    cache_creation_input_token_cost_above_1hr -> cache_write_1h

and the same fields suffixed `_above_200k_tokens` under `above_200k`. A field
LiteLLM lacks is absent, never 0. Every other field (batch, search, …) is
dropped. The output is `json.dumps(sort_keys=True, indent=2)` plus a newline,
so a rerun at the same LiteLLM commit is byte-identical.

Any failure prints `error: …` on stderr and exits 1, and leaves the committed
file's bytes untouched: the new file is written beside it and moved over it
only once it is complete. `--api-base` replaces https://api.github.com, and
exists only so the failure can be exercised.
"""

import argparse
import json
import math
import os
import sys
import tempfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
TABLE = os.path.join(HERE, "anthropic.json")
FILE = "model_prices_and_context_window.json"
REPO = "BerriAI/litellm"
SOURCE = f"https://raw.githubusercontent.com/{REPO}/main/{FILE}"
AGENT = "doctored-make-prices"
TIMEOUT = 60
REQUIRED = ("claude-opus-5-5", "claude-fable-5-1")
FIELDS = {
    "input_cost_per_token": "input",
    "output_cost_per_token": "output",
    "cache_read_input_token_cost": "cache_read",
    "cache_creation_input_token_cost": "cache_write_5m",
    "cache_creation_input_token_cost_above_1hr": "cache_write_1h",
}
BASE_KEYS = tuple(FIELDS.values())


class Failure(Exception):
    pass


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            return response.read()
    except Exception as error:  # noqa: BLE001 — every fetch failure is one loud answer
        raise Failure(f"fetching {url}: {type(error).__name__}: {error}") from None


def fetch_json(url):
    body = fetch(url)
    try:
        return json.loads(body)
    except ValueError as error:
        raise Failure(f"{url} did not answer JSON: {error}") from None


def newest_commit(api_base):
    url = f"{api_base.rstrip('/')}/repos/{REPO}/commits?path={FILE}&per_page=1"
    answer = fetch_json(url)
    try:
        sha = answer[0]["sha"]
        date = answer[0]["commit"]["committer"]["date"]
    except (KeyError, IndexError, TypeError):
        raise Failure(f"{url} did not name a commit") from None
    if not (isinstance(sha, str) and len(sha) == 40
            and all(c in "0123456789abcdef" for c in sha)):
        raise Failure(f"{url} answered a commit that is not 40 hex digits: {sha!r}")
    if not isinstance(date, str) or not date:
        raise Failure(f"{url} answered no committer date")
    return sha, date


def per_million(value, model, field):
    if (isinstance(value, bool) or not isinstance(value, (int, float))
            or not math.isfinite(value) or value < 0):
        raise Failure(f"{model}.{field} is not a finite non-negative number: {value!r}")
    return round(value * 1_000_000, 6)


def reduce_row(model, entry):
    row, above = {}, {}
    for field, key in FIELDS.items():
        if field in entry:
            row[key] = per_million(entry[field], model, field)
        tier = field + "_above_200k_tokens"
        if tier in entry:
            above[key] = per_million(entry[tier], model, tier)
    if above:
        row["above_200k"] = above
    return row


def reduce_table(data, sha, date):
    if not isinstance(data, dict):
        raise Failure(f"{FILE} at {sha} is not a JSON object")
    models = {}
    for model, entry in data.items():
        if isinstance(entry, dict) and entry.get("litellm_provider") == "anthropic":
            models[model] = reduce_row(model, entry)
    if not models:
        raise Failure(f"{FILE} at {sha} has no entry whose litellm_provider is \"anthropic\"")
    for model in REQUIRED:
        row = models.get(model)
        if row is None:
            raise Failure(f"{FILE} at {sha} has no {model} row")
        missing = [key for key in BASE_KEYS if key not in row]
        if missing:
            raise Failure(f"{FILE} at {sha}: {model} lacks {', '.join(missing)}")
    return {
        "format": 1,
        "provider": "anthropic",
        "unit": "USD per million tokens",
        "source": SOURCE,
        "litellm_commit": sha,
        "litellm_commit_date": date,
        "models": models,
    }


def previous_models():
    try:
        with open(TABLE) as f:
            old = json.load(f)
        models = old.get("models")
        return models if isinstance(models, dict) else {}
    except (OSError, ValueError, AttributeError):
        return None


def write(table):
    text = json.dumps(table, sort_keys=True, indent=2) + "\n"
    handle, temp = tempfile.mkstemp(prefix=".anthropic.", suffix=".json", dir=HERE)
    try:
        with os.fdopen(handle, "w") as f:
            f.write(text)
        os.chmod(temp, 0o644)
        os.replace(temp, TABLE)
    except BaseException:
        try:
            os.unlink(temp)
        except OSError:
            pass
        raise


def main():
    parser = argparse.ArgumentParser(description="Regenerate agent/prices/anthropic.json.")
    parser.add_argument("--api-base", default="https://api.github.com",
                        help="the GitHub API base (the failure control points it nowhere)")
    args = parser.parse_args()
    try:
        sha, date = newest_commit(args.api_base)
        data = fetch_json(f"https://raw.githubusercontent.com/{REPO}/{sha}/{FILE}")
        table = reduce_table(data, sha, date)
        before = previous_models()
        write(table)
    except Failure as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except OSError as error:
        print(f"error: writing {TABLE}: {error}", file=sys.stderr)
        return 1
    print(f"commit {sha} {date}")
    print(f"rows {len(table['models'])}")
    after = table["models"]
    if before is None:
        before = {}
    added = sorted(set(after) - set(before))
    removed = sorted(set(before) - set(after))
    changed = sorted(m for m in set(after) & set(before) if after[m] != before[m])
    if not (added or removed or changed):
        print("unchanged")
    for label, ids in (("added", added), ("removed", removed), ("changed", changed)):
        for model in ids:
            print(f"{label} {model}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
