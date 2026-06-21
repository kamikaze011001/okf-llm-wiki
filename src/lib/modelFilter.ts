import type { ModelInfo } from "./api";

/** Case-insensitive substring filter over a model's id and display name.
 *  A blank/whitespace query returns the list unchanged. */
export function filterModels(list: ModelInfo[], query: string): ModelInfo[] {
  const q = query.trim().toLowerCase();
  if (!q) return list;
  return list.filter(
    (m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q),
  );
}
