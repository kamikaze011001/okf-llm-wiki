import { invoke } from "@tauri-apps/api/core";

export interface PageDto { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; }
export interface AnswerDto { text: string; citations: string[]; }
export interface Settings { provider: string; model: string; api_key: string; wiki_path: string; }
export interface Segment { kind: "text" | "link"; text: string; target_path?: string; exists: boolean; }
export interface Ref { path: string; title: string; }
export interface PageView { path: string; title: string; tags: string[]; note?: string; resource?: string; segments: Segment[]; backlinks: Ref[]; }

export const listPages = () => invoke<PageDto[]>("list_pages");
export const getPageView = (path: string) => invoke<PageView>("get_page_view", { path });
export const submitSource = (input: string, note?: string) => invoke<PageDto>("submit_source", { input, note });
export const askQuestion = (question: string) => invoke<AnswerDto>("ask_question", { question });
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });
