import { describe, it, expect, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (_cmd, args) => ({ echoed: args })) }));
import { listPages, submitSource, setSettings, getPageView, reindex, updatePage, deletePage } from "./api";
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
      setSettings({ provider: "claude", model: "m", api_key: "k", wiki_path: "/w", embed_provider: "hash", embed_model: "nomic-embed-text", ollama_url: "http://localhost:11434" })
    ).rejects.toBe("keychain failure");
  });
  it("getPageView passes the path", async () => {
    const r: any = await getPageView("concepts/x.md");
    expect(r.echoed).toEqual({ path: "concepts/x.md" });
  });
  it("reindex invokes the reindex command", async () => {
    await reindex();
    expect(invoke).toHaveBeenCalledWith("reindex");
  });
  it("updatePage invokes the update_page command", async () => {
    await updatePage("concepts/a.md", "New Title", ["x", "y"], "note", "new body");
    expect(invoke).toHaveBeenCalledWith("update_page", {
      path: "concepts/a.md",
      title: "New Title",
      tags: ["x", "y"],
      note: "note",
      body: "new body",
    });
  });
  it("deletePage invokes the delete_page command", async () => {
    await deletePage("concepts/a.md");
    expect(invoke).toHaveBeenCalledWith("delete_page", { path: "concepts/a.md" });
  });
});
