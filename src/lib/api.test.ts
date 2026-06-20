import { describe, it, expect, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (_cmd, args) => ({ echoed: args })) }));
import { listPages, submitSource, setSettings, getPageView } from "./api";
import { invoke } from "@tauri-apps/api/core";

describe("api", () => {
  it("submitSource passes input and note", async () => {
    const r: any = await submitSource("https://x", "my note");
    expect(r.echoed).toEqual({ input: "https://x", note: "my note" });
  });
  it("listPages calls through", async () => {
    await expect(listPages()).resolves.toBeDefined();
  });
  it("setSettings rejects when the backend errors", async () => {
    (invoke as any).mockRejectedValueOnce("keychain failure");
    await expect(
      setSettings({ provider: "claude", model: "m", api_key: "k", wiki_path: "/w" })
    ).rejects.toBe("keychain failure");
  });
  it("getPageView passes the path", async () => {
    const r: any = await getPageView("concepts/x.md");
    expect(r.echoed).toEqual({ path: "concepts/x.md" });
  });
});
