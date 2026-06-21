import { writable } from "svelte/store";
export type Route = "home" | "capture" | "browse" | "ask" | "settings" | "graph";
export const route = writable<Route>("home");
export const currentPage = writable<string | null>(null);
// Whether the app has been configured (API key + wiki folder). Gates the main UI.
export const configured = writable(false);
// Text to pre-fill the capture input on the next visit to the capture view.
// The shell sets this on drop/paste; Home consumes and clears it.
export const capturePrefill = writable("");
