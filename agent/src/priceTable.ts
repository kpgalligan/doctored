// The price table in force (spec §11 P-1, P-5): the snapshot bundled into the
// helper (prices/anthropic.json), a cached copy of LiteLLM's table that the
// helper refreshes at most once a day, and the source it priced from, which
// is always reported so a stale table is never silent. The cache path comes
// from `configure`; without one the helper prices from the bundled table and
// fetches nothing. `reduceLiteLLM` is prices/update.py's `reduce_table`, step
// for step, with the feed and the fetch time in place of a LiteLLM commit.

import { mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute } from "node:path";
import bundled from "../prices/anthropic.json";
import type { PriceRow, PriceTable, Rates } from "./pricing";
import type { PriceSource } from "./protocol";

export const LITELLM_URL = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
export const REFRESH_AFTER_MS = 86_400_000, FETCH_TIMEOUT_MS = 30_000;

export type LoadedPrices = { table: PriceTable; source: PriceSource };
export type PriceOptions = { cache?: string; feed?: string; now?: () => Date };

type JsonObject = { [key: string]: unknown };

const REQUIRED = ["claude-opus-5-5", "claude-fable-5-1"] as const;
const FIELDS: ReadonlyArray<[string, keyof Rates]> = [
  ["input_cost_per_token", "input"],
  ["output_cost_per_token", "output"],
  ["cache_read_input_token_cost", "cache_read"],
  ["cache_creation_input_token_cost", "cache_write_5m"],
  ["cache_creation_input_token_cost_above_1hr", "cache_write_1h"],
];
const BASE_KEYS = FIELDS.map(([, key]) => key);
const LOOPBACK = new Set(["127.0.0.1", "localhost", "[::1]"]);

function isObject(value: unknown): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function own(object: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(object, key);
}

function isRate(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Why `rates` is not a set of rates, or null. */
function ratesProblem(rates: JsonObject, where: string): string | null {
  for (const key of BASE_KEYS) {
    if (own(rates, key) && !isRate(rates[key])) return `${where} ${key} is not a finite non-negative number`;
  }
  return null;
}

/** A `PriceTable` read from JSON, or why it is not one. */
export function parseTable(value: unknown): PriceTable | string {
  if (!isObject(value)) return "not a JSON object";
  if (value["format"] !== 1) return `format ${JSON.stringify(value["format"])}, not 1`;
  if (value["provider"] !== "anthropic") return `provider ${JSON.stringify(value["provider"])}`;
  if (value["unit"] !== "USD per million tokens") return `unit ${JSON.stringify(value["unit"])}`;
  if (typeof value["source"] !== "string") return "no source";
  for (const key of ["litellm_commit", "litellm_commit_date", "fetched_at"]) {
    if (own(value, key) && typeof value[key] !== "string") return `${key} is not a string`;
  }
  const models = value["models"];
  if (!isObject(models)) return "no models";
  for (const [model, row] of Object.entries(models)) {
    if (!isObject(row)) return `${model} is not a row`;
    const base = ratesProblem(row, model);
    if (base) return base;
    const above = row["above_200k"];
    if (above !== undefined) {
      if (!isObject(above)) return `${model} above_200k is not a row`;
      const tier = ratesProblem(above, `${model} above_200k`);
      if (tier) return tier;
    }
  }
  for (const model of REQUIRED) {
    const row = models[model];
    if (!isObject(row)) return `no ${model} row`;
    const missing = BASE_KEYS.filter((key) => !own(row, key));
    if (missing.length > 0) return `${model} lacks ${missing.join(", ")}`;
  }
  return value as unknown as PriceTable;
}

/** update.py's `per_million`: dollars per token to dollars per million, to 6 places. */
function perMillion(value: unknown, model: string, field: string): number | string {
  if (!isRate(value)) return `${model}.${field} is not a finite non-negative number: ${JSON.stringify(value)}`;
  return Number((value * 1_000_000).toFixed(6));
}

/** update.py's `reduce_row`. */
function reduceRow(model: string, entry: JsonObject): PriceRow | string {
  const row: PriceRow = {};
  const above: Rates = {};
  for (const [field, key] of FIELDS) {
    if (own(entry, field)) {
      const rate = perMillion(entry[field], model, field);
      if (typeof rate === "string") return rate;
      row[key] = rate;
    }
    const tier = field + "_above_200k_tokens";
    if (own(entry, tier)) {
      const rate = perMillion(entry[tier], model, tier);
      if (typeof rate === "string") return rate;
      above[key] = rate;
    }
  }
  if (Object.keys(above).length > 0) row.above_200k = above;
  return row;
}

/** update.py's `reduce_table`: LiteLLM's anthropic rows, or why the document is refused. */
export function reduceLiteLLM(data: unknown, source: string, fetchedAt: string): PriceTable | string {
  if (!isObject(data)) return "the document is not a JSON object";
  const models: Record<string, PriceRow> = {};
  for (const [model, entry] of Object.entries(data)) {
    if (isObject(entry) && entry["litellm_provider"] === "anthropic") {
      const row = reduceRow(model, entry);
      if (typeof row === "string") return row;
      models[model] = row;
    }
  }
  if (Object.keys(models).length === 0) return 'no entry\'s litellm_provider is "anthropic"';
  for (const model of REQUIRED) {
    const row = models[model];
    if (row === undefined) return `no ${model} row`;
    const missing = BASE_KEYS.filter((key) => !own(row, key));
    if (missing.length > 0) return `${model} lacks ${missing.join(", ")}`;
  }
  return {
    format: 1,
    provider: "anthropic",
    unit: "USD per million tokens",
    source,
    fetched_at: fetchedAt,
    models,
  };
}

/** Why the helper will not fetch `url`, or null: https, or http to a loopback host. */
export function feedProblem(url: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return `the price feed ${url} is neither https nor a loopback http URL`;
  }
  if (parsed.protocol === "https:") return null;
  if (parsed.protocol === "http:" && LOOPBACK.has(parsed.hostname)) return null;
  return `the price feed ${url} is neither https nor a loopback http URL`;
}

/** The table bundled into the helper, as of its LiteLLM commit's date. */
export function bundledPrices(): LoadedPrices {
  const table = parseTable(bundled as unknown);
  if (typeof table === "string") throw new Error(`the bundled price table is invalid: ${table}`);
  return { table, source: { kind: "bundled", as_of: table.litellm_commit_date ?? "" } };
}

function withProblem(prices: LoadedPrices, problem: string): LoadedPrices {
  return { table: prices.table, source: { ...prices.source, problem } };
}

/** The prices in force at `configure`, from the cache (read synchronously)
 *  or the bundled table, and whether a refresh is due. */
export function initialPrices(o: PriceOptions): { prices: LoadedPrices; refreshDue: boolean } {
  const now = (o.now ?? (() => new Date()))();
  const fallback = bundledPrices();
  if (!o.cache) return { prices: withProblem(fallback, "no price cache is configured"), refreshDue: false };
  if (!isAbsolute(o.cache)) {
    return { prices: withProblem(fallback, "the price cache path is not absolute"), refreshDue: false };
  }
  let text: string;
  try {
    text = readFileSync(o.cache, "utf8");
  } catch (error) {
    if ((error as { code?: string }).code === "ENOENT") return { prices: fallback, refreshDue: true };
    return { prices: withProblem(fallback, `the price cache is unreadable: ${message(error)}`), refreshDue: true };
  }
  let table: PriceTable | string;
  try {
    table = parseTable(JSON.parse(text));
  } catch (error) {
    table = `not JSON: ${message(error)}`;
  }
  if (typeof table === "string") {
    return { prices: withProblem(fallback, `the price cache is unreadable: ${table}`), refreshDue: true };
  }
  const fetchedAt = table.fetched_at ?? "";
  const at = Date.parse(fetchedAt);
  const age = now.getTime() - at;
  const refreshDue = Number.isNaN(at) || age >= REFRESH_AFTER_MS || age < 0;
  return { prices: { table, source: { kind: "cached", as_of: fetchedAt } }, refreshDue };
}

/** A fresh table from the feed, written to the cache as the last step; on any
 *  failure, `current` with the problem and nothing written. Never rejects. */
export async function refreshPrices(current: LoadedPrices, o: PriceOptions): Promise<LoadedPrices> {
  try {
    if (!o.cache) return withProblem(current, "no price cache is configured");
    if (!isAbsolute(o.cache)) return withProblem(current, "the price cache path is not absolute");
    const feed = o.feed ?? LITELLM_URL;
    const refused = feedProblem(feed);
    if (refused) return withProblem(current, refused);
    let data: unknown;
    try {
      const response = await fetch(feed, {
        headers: { "user-agent": "doctored-assistant" },
        signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
      });
      if (!response.ok) return withProblem(current, `fetching ${feed}: HTTP ${response.status}`);
      const body = await response.text();
      try {
        data = JSON.parse(body);
      } catch (error) {
        return withProblem(current, `${feed} was rejected: not JSON: ${message(error)}`);
      }
    } catch (error) {
      return withProblem(current, `fetching ${feed}: ${message(error)}`);
    }
    const fetchedAt = (o.now ?? (() => new Date()))().toISOString();
    const table = reduceLiteLLM(data, feed, fetchedAt);
    if (typeof table === "string") return withProblem(current, `${feed} was rejected: ${table}`);
    const fetched: LoadedPrices = { table, source: { kind: "fetched", as_of: fetchedAt } };
    const temporary = `${o.cache}.tmp-${process.pid}`;
    try {
      mkdirSync(dirname(o.cache), { recursive: true, mode: 0o700 });
      writeFileSync(temporary, JSON.stringify(table, null, 2) + "\n");
      renameSync(temporary, o.cache);
    } catch (error) {
      try {
        unlinkSync(temporary);
      } catch {
        // nothing was left behind
      }
      return withProblem(fetched, `the price cache could not be written: ${message(error)}`);
    }
    return fetched;
  } catch (error) {
    return withProblem(current, `fetching ${o.feed ?? LITELLM_URL}: ${message(error)}`);
  }
}
