import { describe, it, expect } from "vitest";
import { buildGraphModel, nodeRadius } from "./graph-model";
import type { GraphData } from "./api";

describe("graph-model", () => {
  it("maps nodes to sim nodes with radius growing with degree", () => {
    const data: GraphData = {
      nodes: [
        { path: "concepts/a.md", title: "A", degree: 0 },
        { path: "concepts/b.md", title: "B", degree: 5 },
      ],
      edges: [{ source: "concepts/a.md", target: "concepts/b.md" }],
    };
    const model = buildGraphModel(data);
    expect(model.nodes.map((n) => n.path)).toEqual(["concepts/a.md", "concepts/b.md"]);
    expect(model.nodes[1].r).toBeGreaterThan(model.nodes[0].r);
    expect(model.nodes[0].title).toBe("A");
  });

  it("maps edges to links by path", () => {
    const data: GraphData = {
      nodes: [
        { path: "concepts/a.md", title: "A", degree: 1 },
        { path: "concepts/b.md", title: "B", degree: 1 },
      ],
      edges: [{ source: "concepts/a.md", target: "concepts/b.md" }],
    };
    const model = buildGraphModel(data);
    expect(model.links).toEqual([{ source: "concepts/a.md", target: "concepts/b.md" }]);
  });

  it("returns an empty model for empty data", () => {
    const model = buildGraphModel({ nodes: [], edges: [] });
    expect(model.nodes).toEqual([]);
    expect(model.links).toEqual([]);
  });

  it("nodeRadius clamps at the maximum", () => {
    expect(nodeRadius(0)).toBeLessThan(nodeRadius(100));
    expect(nodeRadius(100)).toBe(nodeRadius(1000));
  });
});
