import type { Settings } from "./api";

/** The app is usable only once a key and a wiki folder are set. */
export function isConfigured(s: Settings): boolean {
  return s.api_key.trim() !== "" && s.wiki_path.trim() !== "";
}

/** Extract a droppable link/text from a DataTransfer. Prefers a URL, falls back to text. */
export function extractDropText(dt: Pick<DataTransfer, "getData"> | null): string {
  if (!dt) return "";
  const uri = dt.getData("text/uri-list");
  if (uri) {
    // text/uri-list may contain comment lines starting with '#'; take the first real URL.
    const first = uri.split(/\r?\n/).find((l) => l && !l.startsWith("#"));
    if (first) return first.trim();
  }
  return dt.getData("text/plain").trim();
}

/** Text carried by a paste event, if any. */
export function extractPasteText(e: Pick<ClipboardEvent, "clipboardData">): string {
  return e.clipboardData?.getData("text/plain").trim() ?? "";
}

/** True when a global paste should be hijacked into capture (the user is NOT typing in a field). */
export function shouldInterceptPaste(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return true;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return false;
  if (target.isContentEditable) return false;
  return true;
}
