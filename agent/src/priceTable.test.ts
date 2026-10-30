// `bun test`: the LiteLLM reducer against prices/reduce_cases.json (the
// fixture tools/proof/prices_proof.py's R row runs update.py's reduce_table
// against), and the refresh in each case, against `Bun.serve` on
// 127.0.0.1 only. No test reaches the network.

import { afterAll, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  LITELLM_URL,
  REFRESH_AFTER_MS,
  bundledPrices,
  feedProblem,
  initialPrices,
  parseTable,
  reduceLiteLLM,
  refreshPrices,
} from "./priceTable";

type Fixture = { litellm: Record<string, Record<string, unknown>>; models: Record<string, unknown> };

const fixture = JSON.parse(
  readFileSync(new URL("../prices/reduce_cases.json", import.meta.url), "utf8"),
) as Fixture;
const bundledDate = (JSON.parse(readFileSync(new URL("../prices/anthropic.json", import.meta.url), "utf8")) as {
  litellm_commit_date: string;
}).litellm_commit_date;
const scratch = mkdtempSync(join(tmpdir(), "framejob-prices-"));
afterAll(() => rmSync(scratch, { recursive: true, force: true }));

let folders = 0;
/** A new empty folder under the scratch folder. */
function folder(): string {
  const path = join(scratch, `case-${++folders}`);
  mkdirSync(path);
  return path;
}

/** A feed on 127.0.0.1 answering `body` with `status`, counting its GETs. */
function feed(body: string, status = 200) {
  let gets = 0;
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch() {
      gets += 1;
      return new Response(body, { status, headers: { "content-type": "application/json" } });
    },
  });
  return { url: `http://127.0.0.1:${server.port}/prices.json`, gets: () => gets, stop: () => server.stop(true) };
}

function withoutFable(): string {
  const litellm = { ...fixture.litellm };
  delete litellm["claude-fable-5-1"];
  return JSON.stringify(litellm);
}

test("reduce: the fixture's anthropic rows, per million, batch and bedrock dropped", () => {
  const table = reduceLiteLLM(fixture.litellm, "https://feed.example/p.json", "2026-09-28T12:00:00.000Z");
  if (typeof table === "string") throw new Error(table);
  expect(table.models).toEqual(fixture.models as typeof table.models);
  expect(table.source).toBe("https://feed.example/p.json");
  expect(table.fetched_at).toBe("2026-09-28T12:00:00.000Z");
  expect(table.format).toBe(1);
  expect(table.litellm_commit).toBeUndefined();
  expect(typeof parseTable(JSON.parse(JSON.stringify(table)))).toBe("object");
});

test("reduce: a document without claude-fable-5-1 is refused by name", () => {
  const answer = reduceLiteLLM(JSON.parse(withoutFable()), "s", "t");
  expect(answer).toBe("no claude-fable-5-1 row");
});

test("reduce: not an object, no anthropic entry, a bad rate", () => {
  expect(typeof reduceLiteLLM([], "s", "t")).toBe("string");
  expect(typeof reduceLiteLLM({ x: { litellm_provider: "bedrock" } }, "s", "t")).toBe("string");
  const bad = JSON.parse(JSON.stringify(fixture.litellm));
  bad["claude-opus-5-5"]["input_cost_per_token"] = -1;
  expect(String(reduceLiteLLM(bad, "s", "t"))).toContain("claude-opus-5-5.input_cost_per_token");
});

test("refresh: the bundled table parses, as of its LiteLLM commit date", () => {
  const loaded = bundledPrices();
  expect(loaded.source).toEqual({ kind: "bundled", as_of: bundledDate });
});

test("refresh: no cache configured is the bundled table and nothing due", () => {
  const { prices, refreshDue } = initialPrices({});
  expect(prices.source).toEqual({ kind: "bundled", as_of: bundledDate, problem: "no price cache is configured" });
  expect(refreshDue).toBe(false);
});

test("refresh: a path that is not absolute is refused", async () => {
  const { prices, refreshDue } = initialPrices({ cache: "relative/anthropic-prices.json" });
  expect(prices.source.problem).toBe("the price cache path is not absolute");
  expect(refreshDue).toBe(false);
  const again = await refreshPrices(prices, { cache: "relative/anthropic-prices.json" });
  expect(again.source.problem).toBe("the price cache path is not absolute");
});

test("refresh: no cache fetches once and writes the cache", async () => {
  const served = feed(JSON.stringify(fixture.litellm));
  try {
    const cache = join(folder(), "nested", "anthropic-prices.json");
    const now = () => new Date("2026-09-28T12:00:00.000Z");
    const initial = initialPrices({ cache, feed: served.url, now });
    expect(initial.refreshDue).toBe(true);
    expect(initial.prices.source.kind).toBe("bundled");
    const loaded = await refreshPrices(initial.prices, { cache, feed: served.url, now });
    expect(loaded.source).toEqual({ kind: "fetched", as_of: "2026-09-28T12:00:00.000Z" });
    expect(served.gets()).toBe(1);
    expect(existsSync(cache)).toBe(true);
    const written = parseTable(JSON.parse(readFileSync(cache, "utf8")));
    if (typeof written === "string") throw new Error(written);
    expect(written.source).toBe(served.url);
    expect(written.fetched_at).toBe("2026-09-28T12:00:00.000Z");
    expect(written.models).toEqual(fixture.models as typeof written.models);
  } finally {
    served.stop();
  }
});

test("refresh: the cache's missing folders are made 0700; an existing folder keeps its mode", async () => {
  const served = feed(JSON.stringify(fixture.litellm));
  try {
    const base = folder();
    chmodSync(base, 0o755);
    const cache = join(base, "made", "deeper", "anthropic-prices.json");
    const initial = initialPrices({ cache, feed: served.url });
    const loaded = await refreshPrices(initial.prices, { cache, feed: served.url });
    expect(loaded.source.kind).toBe("fetched");
    expect(existsSync(cache)).toBe(true);
    expect(statSync(join(base, "made")).mode & 0o777).toBe(0o700);
    expect(statSync(join(base, "made", "deeper")).mode & 0o777).toBe(0o700);
    expect(statSync(base).mode & 0o777).toBe(0o755);
  } finally {
    served.stop();
  }
});

test("refresh: a fresh cache is in force and no refresh is due", () => {
  const url = "http://127.0.0.1:1/prices.json";
  const cache = join(folder(), "anthropic-prices.json");
  const table = reduceLiteLLM(fixture.litellm, url, "2026-09-28T12:00:00.000Z");
  writeFileSync(cache, JSON.stringify(table));
  const later = () => new Date(Date.parse("2026-09-28T12:00:00.000Z") + REFRESH_AFTER_MS - 1);
  const { prices, refreshDue } = initialPrices({ cache, feed: url, now: later });
  expect(prices.source).toEqual({ kind: "cached", as_of: "2026-09-28T12:00:00.000Z" });
  expect(refreshDue).toBe(false);
  const future = () => new Date("2026-09-27T12:00:00.000Z");
  expect(initialPrices({ cache, feed: url, now: future }).refreshDue).toBe(true);
});

test("refresh: a stale cache and a dead feed keep the cache, with the problem", async () => {
  const dead = feed("");
  const url = dead.url;
  dead.stop();
  const cache = join(folder(), "anthropic-prices.json");
  const table = reduceLiteLLM(fixture.litellm, url, "2026-09-27T11:00:00.000Z");
  writeFileSync(cache, JSON.stringify(table));
  const before = readFileSync(cache, "utf8");
  const now = () => new Date("2026-09-28T12:00:00.000Z");
  const initial = initialPrices({ cache, feed: url, now });
  expect(initial.refreshDue).toBe(true);
  expect(initial.prices.source.kind).toBe("cached");
  const loaded = await refreshPrices(initial.prices, { cache, feed: url, now });
  expect(loaded.source.kind).toBe("cached");
  expect(loaded.source.as_of).toBe("2026-09-27T11:00:00.000Z");
  expect(loaded.source.problem?.startsWith(`fetching ${url}: `)).toBe(true);
  expect(readFileSync(cache, "utf8")).toBe(before);
});

test("refresh: a rejected document keeps the bundled table and writes nothing", async () => {
  const served = feed(withoutFable());
  try {
    const cache = join(folder(), "anthropic-prices.json");
    const initial = initialPrices({ cache, feed: served.url });
    const loaded = await refreshPrices(initial.prices, { cache, feed: served.url });
    expect(loaded.source.kind).toBe("bundled");
    expect(loaded.source.problem).toBe(`${served.url} was rejected: no claude-fable-5-1 row`);
    expect(existsSync(cache)).toBe(false);
  } finally {
    served.stop();
  }
});

test("refresh: an insecure feed is refused before any request", async () => {
  expect(feedProblem(LITELLM_URL)).toBeNull();
  expect(feedProblem("http://127.0.0.1:9/x")).toBeNull();
  expect(feedProblem("http://localhost:9/x")).toBeNull();
  expect(feedProblem("http://[::1]:9/x")).toBeNull();
  const insecure = "http://example.invalid/";
  expect(feedProblem(insecure)).toBe(`the price feed ${insecure} is neither https nor a loopback http URL`);
  const cache = join(folder(), "anthropic-prices.json");
  const initial = initialPrices({ cache, feed: insecure });
  const loaded = await refreshPrices(initial.prices, { cache, feed: insecure });
  expect(loaded.source.problem).toBe(`the price feed ${insecure} is neither https nor a loopback http URL`);
  expect(existsSync(cache)).toBe(false);
});

test("refresh: an unreadable cache is due and says why", () => {
  const cache = join(folder(), "anthropic-prices.json");
  writeFileSync(cache, "{ not json");
  const { prices, refreshDue } = initialPrices({ cache });
  expect(refreshDue).toBe(true);
  expect(prices.source.kind).toBe("bundled");
  expect(prices.source.problem?.startsWith("the price cache is unreadable: ")).toBe(true);
});
