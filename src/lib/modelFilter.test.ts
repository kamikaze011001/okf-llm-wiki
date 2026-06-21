import { describe, it, expect } from "vitest";
import { filterModels } from "./modelFilter";
import type { ModelInfo } from "./api";

const models: ModelInfo[] = [
  { id: "openai/gpt-4o", name: "GPT-4o" },
  { id: "anthropic/claude-3.5-sonnet", name: "Claude 3.5 Sonnet" },
  { id: "meta-llama/llama-3-70b", name: "Llama 3 70B" },
];

describe("filterModels", () => {
  it("returns all when query is blank", () => expect(filterModels(models, "")).toEqual(models));
  it("returns all when query is whitespace", () => expect(filterModels(models, "   ")).toEqual(models));
  it("matches on id case-insensitively", () =>
    expect(filterModels(models, "OPENAI").map((m) => m.id)).toEqual(["openai/gpt-4o"]));
  it("matches on name case-insensitively", () =>
    expect(filterModels(models, "claude").map((m) => m.id)).toEqual(["anthropic/claude-3.5-sonnet"]));
  it("returns empty when nothing matches", () => expect(filterModels(models, "zzz")).toEqual([]));
});
