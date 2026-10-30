// The assistant helper: the agent loop, run by the Anthropic SDK's Tool
// Runner, in a separate process the app launches from Contents/Helpers. It
// speaks JSON lines over stdio (protocol.ts) and owns no tools itself: every
// tool call goes back to the app, which runs it through the same dispatcher
// its MCP server uses. Two apps carry agent/src/ byte for byte, Rasterize
// and Frame Job: a change is one diff applied to both, and whatever differs
// between them arrives in `configure`.

import Anthropic from "@anthropic-ai/sdk";
import type {
  BetaMessage,
  BetaTool,
  BetaMessageParam,
  BetaToolResultContentBlockParam,
} from "@anthropic-ai/sdk/resources/beta/messages";
import type { BetaRunnableTool } from "@anthropic-ai/sdk/lib/tools/BetaRunnableTool";
import { ToolError } from "@anthropic-ai/sdk/lib/tools/ToolError";
import { createInterface } from "node:readline";
import { price } from "./pricing";
import { initialPrices, refreshPrices } from "./priceTable";
import type { LoadedPrices, PriceOptions } from "./priceTable";
import type {
  CallToolResult,
  CatalogTool,
  ChatSettings,
  Configure,
  HostMessage,
  HelperEvent,
} from "./protocol";

/** Hard ceiling on API rounds within one turn. */
const MAX_ITERATIONS = 30;
/** Thinking counts toward this as well as the reply. */
const MAX_TOKENS = 32000;

const TOOL_SEARCH = {
  type: "tool_search_tool_bm25_20251119",
  name: "tool_search_tool_bm25",
} as const;

/**
 * Old tool results are cleared, when the user turns it on, only past this
 * many tokens in context and in chunks of at least CLEAR_AT_LEAST, so the
 * prompt cache breaks rarely. The API applies it per request, so it can
 * fire mid-turn; it never touches tool search results (server-side).
 */
const CLEAR_TRIGGER_TOKENS = 150_000;
const CLEAR_AT_LEAST_TOKENS = 50_000;
const CLEAR_KEEP_TOOL_USES = 5;

/** Betas every request carries: progress notes, refusal fallback, and compaction blocks. */
const BASE_BETAS = [
  "thinking-display-updates-2026-08-18",
  "server-side-fallback-2026-07-01",
  "compact-2026-09-04",
];

let client: Anthropic | undefined;
let config: Configure | undefined;
/** Chat Settings; configure sets them and a settings message replaces them between turns. */
let settings: ChatSettings = { auto_compact_tokens: 300_000, clear_tool_results: false };
let tools: BetaRunnableTool[] = [];
let history: BetaMessageParam[] = [];
let turn: AbortController | undefined;
let nextCallID = 0;
/** Tokens in the conversation as of the last response, and the model's window. */
let contextUsed = 0;
let contextLimit: number | null = null;
const pendingCalls = new Map<string, (result: CallToolResult | null) => void>();
/** The price table in force; a turn takes it once, at its start (spec §11 P-5). */
let prices: LoadedPrices | undefined;

function emit(event: HelperEvent) {
  process.stdout.write(JSON.stringify(event) + "\n");
}

function configure(message: Configure) {
  config = message;
  client = new Anthropic({
    apiKey: message.api_key,
    baseURL: message.api_base ?? "https://api.anthropic.com",
    maxRetries: 3,
  });
  const core = new Set(message.core_tools);
  tools = message.tools.map((entry) => runnableTool(entry, core));
  settings = message.settings;
  void lookUpContextLimit(message.model);
  setUpPrices({ cache: message.price_cache, feed: message.price_feed });
}

/** The prices at `configure`: reported now when no refresh is due, else once
 *  the refresh settles, in the same continuation that puts it in force. */
function setUpPrices(options: PriceOptions) {
  const initial = initialPrices(options);
  prices = initial.prices;
  if (!initial.refreshDue) {
    emit({ type: "prices", source: initial.prices.source });
    return;
  }
  void refreshPrices(initial.prices, options).then((loaded) => {
    prices = loaded;
    emit({ type: "prices", source: loaded.source });
  });
}

/** What one response cost, at the prices its turn started with; `partial` for one cut short. */
function emitCost(message: BetaMessage, turnPrices: LoadedPrices, partial?: true) {
  emit({
    type: "cost",
    model: message.model,
    usage: message.usage,
    cost: price(message.usage, message.model, turnPrices.table),
    prices: turnPrices.source,
    ...(partial ? { partial } : {}),
  });
}

/** The model's context window, from the Models API, for the panel's meter. */
async function lookUpContextLimit(model: string) {
  try {
    const info = await client!.models.retrieve(model);
    contextLimit = info.max_input_tokens ?? null;
  } catch {
    contextLimit = null; // the meter shows usage without a limit
  }
  emitContextUsage();
}

function emitContextUsage() {
  emit({ type: "context_usage", used: contextUsed, limit: contextLimit });
}

/** Wraps one catalog entry: running it asks the app and waits for the answer. */
function runnableTool(entry: CatalogTool, core: ReadonlySet<string>): BetaRunnableTool {
  return {
    name: entry.name,
    description: entry.description,
    input_schema: entry.inputSchema as BetaTool["input_schema"],
    defer_loading: core.has(entry.name) ? undefined : true,
    parse: (input: unknown) => input,
    run: async (input: unknown) => {
      emit({ type: "tool_call", name: entry.name, arguments: input });
      const callID = String(++nextCallID);
      const result = await new Promise<CallToolResult | null>((resolve) => {
        pendingCalls.set(callID, resolve);
        emit({ type: "tool_request", call_id: callID, name: entry.name, arguments_json: JSON.stringify(input) });
      });
      const { blocks, isError, summary } = convertToolResult(result);
      emit({ type: "tool_result", name: entry.name, is_error: isError, summary });
      if (isError) throw new ToolError(blocks);
      return blocks;
    },
  };
}

/** MCP CallToolResult -> Messages API tool_result content, plus a one-line summary for the panel. */
function convertToolResult(result: CallToolResult | null): {
  blocks: BetaToolResultContentBlockParam[];
  isError: boolean;
  summary: string;
} {
  const failure = (message: string) => ({
    blocks: [{ type: "text" as const, text: message }],
    isError: true,
    summary: message,
  });
  if (!result) return failure("The tool failed to execute.");
  const blocks: BetaToolResultContentBlockParam[] = [];
  let summary = "";
  for (const item of result.content ?? []) {
    if (item.type === "text") {
      const text = item.text ?? "";
      if (!summary) summary = text.slice(0, 200);
      blocks.push({ type: "text", text });
    } else if (item.type === "image") {
      if (!summary) summary = "(image)";
      blocks.push({
        type: "image",
        source: {
          type: "base64",
          media_type: (item.mimeType ?? "image/png") as "image/png",
          data: item.data ?? "",
        },
      });
    }
  }
  if (blocks.length === 0) return failure("The tool returned no content.");
  return { blocks, isError: result.isError === true, summary };
}

/** Appends user text, merging into a trailing user message (e.g. cancelled tool results). */
function pushUserText(text: string) {
  const last = history.at(-1);
  if (last?.role === "user") {
    const content = typeof last.content === "string" ? [{ type: "text" as const, text: last.content }] : last.content;
    last.content = [...content, { type: "text", text }];
  } else {
    history.push({ role: "user", content: [{ type: "text", text }] });
  }
}

// History is append-only: nothing already sent is ever rewritten. Opus 5.5
// replays its earlier thinking blocks only while everything before them is
// byte-identical (and rejects the request outright on newer accounts), and
// an unchanged prefix is also what keeps the prompt cache warm. Old canvas
// renders therefore stay in history. The one sanctioned rewrite is the
// API's own on-demand compaction, which swaps the history for its signed
// summary block at the end of a turn (compactIfDue).

/** Whether the conversation has outgrown the auto-compact limit. */
function compactionDue(): boolean {
  const limit = settings.auto_compact_tokens;
  return limit !== null && contextUsed >= limit;
}

/** Tool result clearing, when on; see CLEAR_TRIGGER_TOKENS. */
function contextManagement() {
  if (!settings.clear_tool_results) return undefined;
  return {
    edits: [
      {
        type: "clear_tool_uses_20250919" as const,
        trigger: { type: "input_tokens" as const, value: CLEAR_TRIGGER_TOKENS },
        keep: { type: "tool_uses" as const, value: CLEAR_KEEP_TOOL_USES },
        clear_at_least: { type: "input_tokens" as const, value: CLEAR_AT_LEAST_TOKENS },
      },
    ],
  };
}

async function runTurn(text: string) {
  if (!client || !config || !prices) {
    emit({ type: "error", message: "The assistant helper was not configured." });
    return;
  }
  if (turn) {
    emit({ type: "error", message: "The assistant is busy." });
    return;
  }
  const controller = new AbortController();
  turn = controller;
  const turnPrices = prices;
  emit({ type: "turn_started" });
  pushUserText(text);

  const runner = client.beta.messages.toolRunner(
    {
      model: config.model,
      max_tokens: MAX_TOKENS,
      max_iterations: MAX_ITERATIONS,
      system: `${config.system}\n\n${config.search_guide}`,
      tools: [TOOL_SEARCH, ...tools],
      messages: history,
      // Automatic caching: the breakpoint moves to the newest block each
      // round, so the tool catalog, system prompt and history are reused.
      cache_control: { type: "ephemeral" },
      // Thinking is always on for Opus 5.5; effort is the dial (its API
      // default is also medium, set here so it doesn't drift).
      output_config: { effort: "medium" },
      // The model's notes between tool calls arrive as thinking blocks;
      // "updates" returns them as short readable summaries.
      thinking: { type: "adaptive", display: "updates" },
      // A classifier refusal is retried on the model Anthropic picks for it.
      fallbacks: "default",
      context_management: contextManagement(),
      betas: settings.clear_tool_results
        ? [...BASE_BETAS, "context-management-2025-06-27"]
        : BASE_BETAS,
      stream: true,
    },
    { signal: controller.signal },
  );

  let last: BetaMessage | undefined;
  let compacting = false;
  // The response in flight, as its stream has reported it so far: priced as
  // partial if the turn throws before `finalMessage` resolves (an object, so
  // tsc does not narrow the property to `never` in the catch).
  const inFlight: { message?: BetaMessage } = {};
  try {
    for await (const stream of runner) {
      stream.on("streamEvent", (_event, snapshot) => {
        inFlight.message = snapshot;
      });
      stream.on("text", (delta) => emit({ type: "text_delta", text: delta }));
      stream.on("thinking", (delta) => emit({ type: "progress_delta", text: delta }));
      const message = await stream.finalMessage();
      inFlight.message = undefined;

      if (compacting) {
        // The summary request the runner sent after the final reply.
        compacting = false;
        emitCost(message, turnPrices);
        reportCompaction(message);
        continue;
      }

      last = message;
      emit({ type: "message_done", stop_reason: message.stop_reason, usage: message.usage });
      emitCost(message, turnPrices);
      // Everything this request sent (cached or not) plus the reply, which
      // becomes part of the next request.
      const usage = message.usage;
      contextUsed =
        usage.input_tokens +
        (usage.cache_read_input_tokens ?? 0) +
        (usage.cache_creation_input_tokens ?? 0) +
        usage.output_tokens;
      emitContextUsage();
      reportClearedToolResults(message);

      // Compaction only ever runs when a turn ends: after the final reply,
      // never between tool calls. The runner sends the summary request next
      // and swaps its history for the returned block.
      const final = message.stop_reason !== "tool_use" && message.stop_reason !== "pause_turn";
      if (final && compactionDue() && !controller.signal.aborted) {
        compacting = true;
        emit({ type: "compaction_started", used: contextUsed });
        runner.compactBeforeNextTurn({ type: "summarize", instructions: config.compaction_instructions });
      }
    }
    if (compacting) {
      // The runner ended without handing back a summary.
      emit({ type: "compaction_failed", reason: "no summary came back" });
    }
    if (last?.stop_reason === "max_tokens") {
      emit({ type: "error", message: "The reply was cut off at the output limit." });
    } else if (last?.stop_reason === "refusal") {
      emit({ type: "error", message: "The model declined this request." });
    }
  } catch (error) {
    if (inFlight.message) {
      emitCost(inFlight.message, turnPrices, true);
      inFlight.message = undefined;
    }
    if (!controller.signal.aborted) {
      emit({ type: "error", message: errorMessage(error) });
    }
  } finally {
    // The runner keeps its own copy of the conversation, including
    // tool results that landed before a cancel or an error.
    history = [...runner.params.messages];
    turn = undefined;
    emit({ type: "turn_finished" });
  }
}

/** A compaction response: the summary block replaced the history, or nothing did. */
function reportCompaction(message: BetaMessage) {
  if (message.stop_reason !== "compaction") {
    // The history is unchanged; the next turn end tries again.
    emit({ type: "compaction_failed", reason: `the summary stopped with ${message.stop_reason}` });
    return;
  }
  const before = contextUsed;
  const summary = message.usage.iterations?.find((entry) => entry.type === "compaction");
  // The summary is what remains of the conversation; the next response's
  // usage replaces this estimate with the real figure.
  contextUsed = summary?.output_tokens ?? 0;
  emit({ type: "compacted", before, after: contextUsed });
  emitContextUsage();
}

/** Tells the panel when the API cleared old tool results from this request. */
function reportClearedToolResults(message: BetaMessage) {
  for (const edit of message.context_management?.applied_edits ?? []) {
    if (edit.type === "clear_tool_uses_20250919") {
      emit({
        type: "tool_results_cleared",
        tool_uses: edit.cleared_tool_uses,
        tokens: edit.cleared_input_tokens,
      });
    }
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Anthropic.APIError) {
    const detail = (error.error as { error?: { message?: string } } | undefined)?.error?.message;
    return `API error ${error.status ?? ""}: ${detail ?? error.message}`.trim();
  }
  return error instanceof Error ? error.message : String(error);
}

function handle(message: HostMessage) {
  switch (message.type) {
    case "configure":
      configure(message);
      emit({ type: "ready" });
      break;
    case "send":
      void runTurn(message.text);
      break;
    case "cancel":
      turn?.abort();
      break;
    case "tool_result": {
      const resolve = pendingCalls.get(message.call_id);
      pendingCalls.delete(message.call_id);
      resolve?.(message.result);
      break;
    }
    case "settings":
      // Applies from the next turn; a running turn keeps what it started with.
      settings = message.settings;
      break;
    case "reset":
      turn?.abort();
      history = [];
      contextUsed = 0;
      emitContextUsage();
      break;
  }
}

const input = createInterface({ input: process.stdin });
input.on("line", (line) => {
  if (!line.trim()) return;
  try {
    handle(JSON.parse(line) as HostMessage);
  } catch (error) {
    emit({ type: "error", message: `Bad message from the app: ${errorMessage(error)}` });
  }
});
// The app closing our stdin means it is gone or closed the session.
input.on("close", () => process.exit(0));
