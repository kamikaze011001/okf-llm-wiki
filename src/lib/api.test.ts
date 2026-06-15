import { describe, it, expect, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (_cmd, args) => ({ echoed: args })) }));
import { listPages, submitSource } from "./api";

describe("api", () => {
  it("submitSource passes input and note", async () => {
    const r: any = await submitSource("https://x", "my note");
    expect(r.echoed).toEqual({ input: "https://x", note: "my note" });
  });
  it("listPages calls through", async () => {
    await expect(listPages()).resolves.toBeDefined();
  });
});
