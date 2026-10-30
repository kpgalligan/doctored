// What one Messages API response cost, from its own `usage` and `model` and a
// price table of LiteLLM's Anthropic rows in US dollars per million tokens
// (prices/anthropic.json). Pure: no I/O, and `price` never throws and never
// answers NaN, whatever JSON value `usage` is. A model the table does not
// hold, and usage the table cannot price (fast mode, a batch or priority
// tier, a US-only request, a malformed count, a class whose rate the row
// lacks), answers `unknown` with its cause, never $0.
//
// When `usage.iterations` is a non-empty array, each element is priced at
// its own `model` (or the response's), because the top-level counts describe
// only the attempt that returned the message and leave out compaction and a
// refused attempt before a fallback.

export type Rates = {
  input?: number;
  output?: number;
  cache_read?: number;
  cache_write_5m?: number;
  cache_write_1h?: number;
};
export type PriceRow = Rates & { above_200k?: Rates };
export type PriceTable = {
  format: 1;
  provider: "anthropic";
  unit: "USD per million tokens";
  source: string;
  litellm_commit?: string;
  litellm_commit_date?: string;
  fetched_at?: string;
  models: Record<string, PriceRow>;
};
export type UnknownCause = "model" | "rate" | "usage" | "speed" | "service_tier" | "inference_geo";
export type Price =
  | { kind: "dollars"; dollars: number; rows: string[] }
  | { kind: "unknown"; cause: UnknownCause; detail: string };

type JsonObject = { [key: string]: unknown };
type Counts = { input: number; write5m: number; write1h: number; read: number; output: number; total: number };
type Entry = { value: JsonObject; model: string };
type CountKey = "input" | "write5m" | "write1h" | "read" | "output";

const TIER_THRESHOLD = 200_000;
const DATED = /-[0-9]{8}$/;
/** The order a class's cost is added in; the Python reader uses the same. */
const CLASSES: ReadonlyArray<[keyof Rates, CountKey]> = [
  ["input", "input"],
  ["cache_write_5m", "write5m"],
  ["cache_write_1h", "write1h"],
  ["cache_read", "read"],
  ["output", "output"],
];

function isObject(value: unknown): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function own(object: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(object, key);
}

function unknown(cause: UnknownCause, detail: string): Price {
  return { kind: "unknown", cause, detail };
}

/** The table key a response's model id prices at: the exact id, or the id
 *  less a trailing `-YYYYMMDD` when that is a key; nothing else is stripped. */
export function matchModel(model: string, table: PriceTable): string | null {
  const models = table.models;
  if (own(models, model)) return model;
  if (DATED.test(model)) {
    const bare = model.slice(0, -9);
    if (own(models, bare)) return bare;
  }
  return null;
}

/** A count: absent or null is 0, else a non-negative integer, else null. */
function count(object: JsonObject, key: string): number | null {
  const value = object[key];
  if (value === undefined || value === null) return 0;
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) return null;
  return value;
}

function counts(entry: JsonObject): Counts | string {
  const input = count(entry, "input_tokens");
  const creation = count(entry, "cache_creation_input_tokens");
  const read = count(entry, "cache_read_input_tokens");
  const output = count(entry, "output_tokens");
  if (input === null) return "input_tokens is not a non-negative integer";
  if (creation === null) return "cache_creation_input_tokens is not a non-negative integer";
  if (read === null) return "cache_read_input_tokens is not a non-negative integer";
  if (output === null) return "output_tokens is not a non-negative integer";
  let write5m = creation;
  let write1h = 0;
  const split = entry["cache_creation"];
  if (isObject(split)) {
    const five = count(split, "ephemeral_5m_input_tokens");
    const hour = count(split, "ephemeral_1h_input_tokens");
    if (five === null) return "cache_creation.ephemeral_5m_input_tokens is not a non-negative integer";
    if (hour === null) return "cache_creation.ephemeral_1h_input_tokens is not a non-negative integer";
    write5m = five;
    write1h = hour;
  }
  return { input, write5m, write1h, read, output, total: input + creation + read };
}

function usable(rate: unknown): rate is number {
  return typeof rate === "number" && Number.isFinite(rate) && rate >= 0;
}

function allowed(usage: JsonObject, key: string, ok: string): boolean {
  const value = usage[key];
  return value === undefined || value === null || value === ok;
}

export function price(usage: unknown, model: string, table: PriceTable): Price {
  // 1. The shapes.
  if (!isObject(usage)) return unknown("usage", "usage is not an object");
  if (typeof model !== "string" || model === "") return unknown("model", "no model id");

  // 2. Modes the table has no rate for.
  if (!allowed(usage, "speed", "standard")) {
    return unknown("speed", `speed ${JSON.stringify(usage["speed"])} is not priced`);
  }
  if (!allowed(usage, "service_tier", "standard")) {
    return unknown("service_tier", `service_tier ${JSON.stringify(usage["service_tier"])} is not priced`);
  }
  if (!allowed(usage, "inference_geo", "global")) {
    return unknown("inference_geo", `inference_geo ${JSON.stringify(usage["inference_geo"])} is not priced`);
  }

  // 3. The entries.
  const entries: Entry[] = [];
  const iterations = usage["iterations"];
  if (iterations !== undefined && iterations !== null && !Array.isArray(iterations)) {
    return unknown("usage", "iterations is not an array");
  }
  if (Array.isArray(iterations) && iterations.length > 0) {
    for (let i = 0; i < iterations.length; i++) {
      const element: unknown = iterations[i];
      if (!isObject(element)) return unknown("usage", `iterations[${i}] is not an object`);
      const named = element["model"];
      entries.push({ value: element, model: typeof named === "string" && named !== "" ? named : model });
    }
  } else {
    entries.push({ value: usage, model });
  }

  // 4. The counts.
  const tallies: Counts[] = [];
  for (let i = 0; i < entries.length; i++) {
    const answer = counts(entries[i].value);
    if (typeof answer === "string") return unknown("usage", entries.length > 1 ? `iterations[${i}].${answer}` : answer);
    tallies.push(answer);
  }

  // 5. The rows.
  const keys: string[] = [];
  for (const entry of entries) {
    const key = matchModel(entry.model, table);
    if (key === null) return unknown("model", `${entry.model} is not in the price table`);
    keys.push(key);
  }

  // 6. The tier and the rates.
  const rates: Rates[] = [];
  for (let i = 0; i < entries.length; i++) {
    const row: unknown = table.models[keys[i]];
    if (!isObject(row)) return unknown("rate", `${keys[i]} has no rates`);
    const above = row["above_200k"];
    const tiered = isObject(above) && tallies[i].total > TIER_THRESHOLD;
    const chosen: Rates = (tiered ? above : row) as Rates;
    for (const [rateKey, countKey] of CLASSES) {
      if (tallies[i][countKey] > 0 && !usable(chosen[rateKey])) {
        return unknown("rate", `${keys[i]} has no ${tiered ? "above_200k " : ""}${rateKey} rate`);
      }
    }
    rates.push(chosen);
  }

  // 7. The arithmetic.
  let dollars = 0;
  for (let i = 0; i < entries.length; i++) {
    let s = 0;
    for (const [rateKey, countKey] of CLASSES) {
      const tokens = tallies[i][countKey];
      if (tokens === 0) continue;
      s += tokens * (rates[i][rateKey] as number);
    }
    dollars += s / 1_000_000;
  }
  const rows: string[] = [];
  for (const key of keys) if (!rows.includes(key)) rows.push(key);
  return { kind: "dollars", dollars, rows };
}
