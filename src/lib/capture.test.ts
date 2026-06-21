import { describe, it, expect } from "vitest";
import { isConfigured, extractDropText, extractPasteText, shouldInterceptPaste } from "./capture";
import type { Settings } from "./api";

const base: Settings = {
  provider: "claude", model: "m", api_key: "", wiki_path: "",
  embed_provider: "hash", embed_model: "e", ollama_url: "u",
};

describe("isConfigured", () => {
  it("false when key blank", () => expect(isConfigured({ ...base, wiki_path: "/w" })).toBe(false));
  it("false when folder blank", () => expect(isConfigured({ ...base, api_key: "k" })).toBe(false));
  it("false when whitespace only", () => expect(isConfigured({ ...base, api_key: "  ", wiki_path: "  " })).toBe(false));
  it("true when both set", () => expect(isConfigured({ ...base, api_key: "k", wiki_path: "/w" })).toBe(true));
});

describe("extractDropText", () => {
  const dt = (m: Record<string, string>) => ({ getData: (t: string) => m[t] ?? "" });
  it("prefers first non-comment uri-list line", () =>
    expect(extractDropText(dt({ "text/uri-list": "# comment\r\nhttps://x.com\r\nhttps://y.com" }))).toBe("https://x.com"));
  it("falls back to text/plain when no uri-list", () =>
    expect(extractDropText(dt({ "text/plain": "  hello  " }))).toBe("hello"));
  it("returns empty for null", () => expect(extractDropText(null)).toBe(""));
  it("returns empty when no data", () => expect(extractDropText(dt({}))).toBe(""));
});

describe("extractPasteText", () => {
  it("returns trimmed text/plain", () =>
    expect(extractPasteText({ clipboardData: { getData: () => "  hi  " } as unknown as DataTransfer })).toBe("hi"));
  it("empty when clipboardData null", () =>
    expect(extractPasteText({ clipboardData: null })).toBe(""));
});

describe("shouldInterceptPaste", () => {
  it("false for input", () => expect(shouldInterceptPaste(document.createElement("input"))).toBe(false));
  it("false for textarea", () => expect(shouldInterceptPaste(document.createElement("textarea"))).toBe(false));
  it("false for select", () => expect(shouldInterceptPaste(document.createElement("select"))).toBe(false));
  it("true for div", () => expect(shouldInterceptPaste(document.createElement("div"))).toBe(true));
  it("true for null", () => expect(shouldInterceptPaste(null)).toBe(true));
});
