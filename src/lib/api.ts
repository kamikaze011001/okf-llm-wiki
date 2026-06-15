import { invoke } from "@tauri-apps/api/core";

export interface PageDto { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; }
export interface AnswerDto { text: string; citations: string[]; }
export interface Settings { provider: string; model: string; api_key: string; wiki_path: string; }

export const listPages = () => invoke<PageDto[]>("list_pages");
export const submitSource = (input: string, note?: string) => invoke<PageDto>("submit_source", { input, note });
export const askQuestion = (question: string) => invoke<AnswerDto>("ask_question", { question });
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });
