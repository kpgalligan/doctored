// The stdio protocol between the app (its helper session) and
// this helper: one JSON object per line in each direction.

import type { Price } from "./pricing";

/** One entry of the app's MCP-format tool catalog. */
export type CatalogTool = {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
};

/** An MCP CallToolResult, as AgentServer.execute returns it. */
export type CallToolResult = {
  // text carries `text`; image carries base64 `data` and `mimeType`.
  content?: Array<{ type: string; text?: string; data?: string; mimeType?: string }>;
  isError?: boolean;
};

/** The Assistant panel's Chat Settings that shape requests. */
export type ChatSettings = {
  /** Compact at the end of a turn once context reaches this many tokens; null never compacts. */
  auto_compact_tokens: number | null;
  /** Let the API clear old tool results past a threshold (off by default). */
  clear_tool_results: boolean;
};

/** The table a price came from: the bundled snapshot (as of its LiteLLM
 *  commit's date), the cached copy or a fresh fetch (as of `fetched_at`), and
 *  why it was not refreshed, if it was not. */
export type PriceSource = { kind: "bundled" | "cached" | "fetched"; as_of: string /* bundled: litellm_commit_date; else fetched_at */; problem?: string };

// App -> helper

export type Configure = {
  type: "configure";
  api_key: string;
  model: string;
  system: string;
  tools: CatalogTool[];
  /** Tools the model sees up front; every other catalog entry is deferred and found through tool search. */
  core_tools: string[];
  /** Appended to the system prompt after a blank line: what tool search can find. */
  search_guide: string;
  /** What the end-of-turn compaction summary must carry. */
  compaction_instructions: string;
  /** The app's base-URL override (an environment variable of its own), for pointing at a test server. */
  api_base?: string;
  settings: ChatSettings;
  /** The absolute path of the price cache; absent = the bundled table only, and no refresh. */
  price_cache?: string;
  /** Where the refresh fetches LiteLLM's table; absent = LITELLM_URL. */
  price_feed?: string;
};

export type HostMessage =
  | Configure
  | { type: "send"; text: string }
  | { type: "cancel" }
  | { type: "reset" }
  /** Chat Settings changed; applies from the next turn. */
  | { type: "settings"; settings: ChatSettings }
  /** The answer to a tool_request; result null means the app could not run it. */
  | { type: "tool_result"; call_id: string; result: CallToolResult | null };

// Helper -> app.

export type HelperEvent =
  | { type: "ready" }
  | { type: "turn_started" }
  | { type: "text_delta"; text: string }
  /** A progress note between tool calls (a thinking block under display "updates"). */
  | { type: "progress_delta"; text: string }
  | { type: "message_done"; stop_reason: string | null; usage: unknown }
  /** Tokens in the conversation after the last response; limit is the model's window (null if unknown). */
  | { type: "context_usage"; used: number; limit: number | null }
  | { type: "tool_call"; name: string; arguments: unknown }
  /** Run a tool and answer with tool_result. The arguments stay a JSON string, as the dispatcher takes them. */
  | { type: "tool_request"; call_id: string; name: string; arguments_json: string }
  | { type: "tool_result"; name: string; is_error: boolean; summary: string }
  | { type: "error"; message: string }
  /** The turn's final reply pushed context past the auto-compact limit; a summary is being written. */
  | { type: "compaction_started"; used: number }
  /** The history is now the summary block; before/after are tokens in context. */
  | { type: "compacted"; before: number; after: number }
  /** No summary came back; the history is unchanged and the next turn end tries again. */
  | { type: "compaction_failed"; reason: string }
  /** The API cleared old tool results from what the model saw on one request. */
  | { type: "tool_results_cleared"; tool_uses: number; tokens: number }
  /** Once per configure: before `ready` if no refresh is due, else when it settles. */
  | { type: "prices"; source: PriceSource }
  /**
   * Per response, at the turn's prices: after message_done; for a summary, before compacted/compaction_failed.
   * `partial`: a response cut short (a cancel or an error), priced from the usage its stream had reported, before any error.
   */
  | { type: "cost"; model: string; usage: unknown; cost: Price; prices: PriceSource; partial?: true }
  | { type: "turn_finished" };
