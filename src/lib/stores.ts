import { writable } from "svelte/store";
export type Route = "home" | "capture" | "browse" | "ask" | "settings" | "graph";
export const route = writable<Route>("home");
export const currentPage = writable<string | null>(null);
