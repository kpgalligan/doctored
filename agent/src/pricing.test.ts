// `bun test`: every case in prices/cases.json through `price`, and the
// committed prices/anthropic.json's shape. The same cases file is run by a
// second reader of the same table; both must answer every case alike.

import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { price } from "./pricing";
import type { Price, PriceTable, UnknownCause } from "./pricing";

type Expect =
  | { kind: "dollars"; dollars: number; rows: string[] }
  | { kind: "unknown"; cause: UnknownCause };
type Case = { name: string; model: string; usage: unknown; expect: Expect };
type Cases = { table: PriceTable; cases: Case[] };

const PRICES = new URL("../prices/", import.meta.url);
const TOLERANCE = 1e-12;
const BASE_KEYS = ["input", "output", "cache_read", "cache_write_5m", "cache_write_1h"] as const;

function readJson(name: string): unknown {
  return JSON.parse(readFileSync(new URL(name, PRICES), "utf8"));
}

/** Empty when `got` answers `want`, else what differs. */
function mismatch(got: Price, want: Expect): string {
  if (got.kind !== want.kind) return `kind ${got.kind}, expected ${want.kind} (${JSON.stringify(got)})`;
  if (got.kind === "dollars" && want.kind === "dollars") {
    if (!(Math.abs(got.dollars - want.dollars) <= TOLERANCE)) return `dollars ${got.dollars}, expected ${want.dollars}`;
    if (JSON.stringify(got.rows) !== JSON.stringify(want.rows)) {
      return `rows ${JSON.stringify(got.rows)}, expected ${JSON.stringify(want.rows)}`;
    }
  }
  if (got.kind === "unknown" && want.kind === "unknown" && got.cause !== want.cause) {
    return `cause ${got.cause} (${got.detail}), expected ${want.cause}`;
  }
  return "";
}

const fixture = readJson("cases.json") as Cases;

for (const c of fixture.cases) {
  test(`case: ${c.name}`, () => {
    expect(mismatch(price(c.usage, c.model, fixture.table), c.expect)).toBe("");
  });
}

test("table: committed", () => {
  const table = readJson("anthropic.json") as PriceTable;
  expect(table.format).toBe(1);
  expect(table.unit).toBe("USD per million tokens");
  expect(table.provider).toBe("anthropic");
  expect(/^[0-9a-f]{40}$/.test(table.litellm_commit ?? "")).toBe(true);
  for (const model of ["claude-opus-5-5", "claude-fable-5-1"]) {
    const row = table.models[model];
    expect(row).toBeDefined();
    for (const key of BASE_KEYS) {
      const rate = row[key];
      expect(`${model} ${key} ${typeof rate === "number" && Number.isFinite(rate) && rate >= 0}`).toBe(
        `${model} ${key} true`,
      );
    }
  }
  const priced = price({ input_tokens: 1000 }, "claude-opus-5-5", table);
  expect(priced.kind === "dollars" && priced.dollars > 0).toBe(true);
});
