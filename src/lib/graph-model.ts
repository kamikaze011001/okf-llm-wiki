import type { SimulationNodeDatum, SimulationLinkDatum } from "d3-force";
import type { GraphData } from "./api";

/** A graph node positioned by the d3-force simulation. x/y/fx/fy are managed by d3. */
export interface SimNode extends SimulationNodeDatum {
  path: string;
  title: string;
  degree: number;
  r: number;
}

/** A graph edge. d3-force resolves the string `path` endpoints to SimNode objects in place. */
export type SimLink = SimulationLinkDatum<SimNode>;

export interface GraphModel {
  nodes: SimNode[];
  links: SimLink[];
}

const BASE_R = 14;
const R_PER_DEGREE = 3;
const MAX_R = 34;

/** Node radius grows with degree, clamped so hubs do not dominate the canvas. */
export function nodeRadius(degree: number): number {
  return Math.min(BASE_R + degree * R_PER_DEGREE, MAX_R);
}

/** Shape backend GraphData into d3-force simulation inputs. Pure — no positions assigned
 *  (d3 initializes x/y); links reference nodes by `path` for forceLink's id accessor. */
export function buildGraphModel(data: GraphData): GraphModel {
  const nodes: SimNode[] = data.nodes.map((n) => ({
    path: n.path,
    title: n.title,
    degree: n.degree,
    r: nodeRadius(n.degree),
  }));
  const links: SimLink[] = data.edges.map((e) => ({ source: e.source, target: e.target }));
  return { nodes, links };
}
