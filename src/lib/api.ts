import { invoke } from "@tauri-apps/api/core";

export interface PageDto { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; }
export interface AnswerDto { text: string; citations: string[]; }
export interface Settings { provider: string; model: string; api_key: string; wiki_path: string; embed_provider: string; embed_model: string; ollama_url: string; }
export interface Segment { kind: "text" | "link"; text: string; target_path?: string; exists: boolean; }
export interface Ref { path: string; title: string; }
export interface PageView { path: string; title: string; body: string; tags: string[]; note?: string; resource?: string; segments: Segment[]; backlinks: Ref[]; }
export interface GraphNode { path: string; title: string; degree: number; }
export interface GraphEdge { source: string; target: string; }
export interface GraphData { nodes: GraphNode[]; edges: GraphEdge[]; }
export interface ModelInfo { id: string; name: string; }

export const listPages = () => invoke<PageDto[]>("list_pages");
export const getPageView = (path: string) => invoke<PageView>("get_page_view", { path });
export const submitSource = (input: string, note?: string) => invoke<PageDto>("submit_source", { input, note });
export const askQuestion = (question: string) => invoke<AnswerDto>("ask_question", { question });
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });
export const reindex = () => invoke<void>("reindex");
export const updatePage = (path: string, title: string | undefined, tags: string[], note: string | undefined, body: string) =>
  invoke<PageDto>("update_page", { path, title, tags, note, body });
export const deletePage = (path: string) => invoke<void>("delete_page", { path });
export const createPage = (title: string) => invoke<PageDto>("create_page", { title });
export const getGraph = () => invoke<GraphData>("get_graph");
