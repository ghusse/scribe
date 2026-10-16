// Fake Tauri backend for the screenshot pages: answers every command of src/lib/api.ts with fictional data.
import type { Dictation, PresetView, Provider, Settings, Style, Term } from "../../src/lib/api";

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

const settings: Settings = {
  language: "system",
  trigger_keys: [0x5c],
  lock_vk: 0x20,
  gesture: { hold_threshold_ms: 300, double_tap_window_ms: 350, double_tap_enabled: true, lock_key_enabled: true },
  correction_enabled: true,
  styles: [
    { id: "default", preset: "default", name: null, keyword: null, aliases: [], custom_task: null },
    { id: "slack", preset: "slack", name: null, keyword: "slack", aliases: [], custom_task: null },
    { id: "mail", preset: "mail", name: null, keyword: "mail", aliases: ["email"], custom_task: null },
  ] satisfies Style[],
  offered_builtins: ["default", "clean", "slack", "mail"],
  stt_provider: "openai",
  stt_model: "gpt-transcribe",
  llm_provider: "anthropic",
  llm_model: "claude-opus-5-5",
  llm_effort: "low",
  restore_delay_ms: 150,
  min_recording_ms: 300,
  max_recording_ms: 600000,
  silence_threshold_dbfs: -50,
  llm_timeout_base_ms: 5000,
  llm_timeout_per_char_ms: 10,
  hint_budget_chars: 2000,
  audio_retention_days: 30,
  mute_audio_during_dictation: true,
};

const presets: PresetView[] = [
  { preset: "default", name: "Default", task: "- Fix misrecognised words, using the glossary above first.\n- Restore punctuation and capitalisation." },
  { preset: "clean", name: "Clean", task: "- Fix misrecognised words.\n- Restore punctuation." },
  { preset: "slack", name: "Slack", task: "- Write a Slack message ready to send, concise and direct." },
  { preset: "mail", name: "Email", task: "- Lay the text out as an email: a greeting, short paragraphs, then a closing." },
];

const efforts = ["low", "medium", "high"];
const providers: Provider[] = [
  {
    id: "openai", label: "OpenAI", base_url: "", stt_models: ["gpt-transcribe", "gpt-4o-transcribe"], llm_api: "open_ai_chat",
    llm_models: [{ id: "gpt-6.1-sol", efforts }, { id: "gpt-6-astra", efforts }],
  },
  {
    id: "anthropic", label: "Anthropic", base_url: "", stt_models: [], llm_api: "anthropic",
    llm_models: [{ id: "claude-opus-5-5", efforts }, { id: "claude-sonnet-5-5", efforts }, { id: "claude-haiku-4-5", efforts: [] }],
  },
  {
    id: "mistral", label: "Mistral", base_url: "", stt_models: ["voxtral-mini-latest"], llm_api: "open_ai_chat",
    llm_models: [{ id: "mistral-medium-latest", efforts: [] }],
  },
  {
    id: "groq", label: "Groq", base_url: "", stt_models: ["whisper-large-v3-turbo"], llm_api: "open_ai_chat",
    llm_models: [{ id: "openai/gpt-oss-120b", efforts }],
  },
];

function dictation(id: number, over: Partial<Dictation>): Dictation {
  return {
    id, created_at: minutesAgo(id * 17), mode: "hold", app_name: null, audio_path: "demo.wav", duration_ms: 8000,
    raw_text: null, final_text: null, edited_text: null, style: "default", body_start: null, transcriber: "openai/gpt-transcribe",
    corrector: "anthropic/claude-opus-5-5", stt_ms: 620, llm_ms: 840, outcome: "pasted", error: null, ...over,
  };
}

const dictations: Dictation[] = [
  dictation(1, {
    app_name: "Mail",
    raw_text: "hi claire uh just confirming thursday's meeting at ten with the kubernetes team",
    final_text: "Hi Claire,\n\nJust confirming Thursday's meeting at 10 a.m. with the Kubernetes team.\n\nBest,",
  }),
  dictation(2, {
    app_name: "Slack", mode: "locked", duration_ms: 14500,
    raw_text: "the release went out everything is green on grafana i'll check the logs tomorrow morning",
    final_text: "The release went out, everything is green on Grafana. I'll check the logs tomorrow morning 👍",
  }),
  dictation(3, {
    app_name: "Code",
    raw_text: "return an error if the list is empty instead of crashing",
    final_text: "Return an error if the list is empty instead of crashing.",
  }),
  dictation(4, {
    app_name: "Notes", outcome: "clipboard",
    raw_text: "weekend ideas hike to the lake buy bread remember leah's present",
    final_text: "Weekend ideas:\n- hike to the lake\n- buy bread\n- remember Leah's present",
  }),
];

const term = (id: number, t: string, variants: string[], note: string | null, use_count: number): Term => ({
  id, term: t, variants, note, source: "manual", use_count, last_used_at: minutesAgo(id * 30), created_at: minutesAgo(10_000),
});
const terms: Term[] = [
  term(1, "Kubernetes", ["cube ernetes", "cubernetis"], "Container orchestrator", 42),
  term(2, "Grafana", ["grafanna"], null, 17),
  term(3, "PostgreSQL", ["postgres q l", "post grass"], "Database", 9),
  term(4, "Leah", ["lea", "leia"], "First name", 6),
  term(5, "OAuth", ["o auth", "oh auth"], null, 3),
];

const routes: Record<string, (args: Record<string, unknown>) => unknown> = {
  list_dictations: ({ offset }) => (offset ? [] : dictations),
  list_terms: () => terms,
  get_settings: () => settings,
  language: () => "en",
  key_status: () => ({ openai: true, anthropic: true, mistral: false, groq: false }),
  providers: () => providers,
  style_catalog: () => ({ presets, styles: presets.map((p) => ({ id: p.preset, name: p.name })), correction_enabled: true }),
  get_autostart: () => true,
  check_update: () => ({ current: "1.0.0", available: null }),
  permissions: () => [
    { permission: "accessibility", state: "granted" },
    { permission: "microphone", state: "granted" },
  ],
};

const listeners = new Map<string, number[]>();
const callbacks = new Map<number, (e: unknown) => void>();
let nextId = 1;

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  transformCallback(cb: (e: unknown) => void) {
    const id = nextId++;
    callbacks.set(id, cb);
    return id;
  },
  unregisterCallback(id: number) {
    callbacks.delete(id);
  },
  async invoke(cmd: string, args: Record<string, unknown> = {}) {
    if (cmd === "plugin:event|listen") {
      const name = args.event as string;
      listeners.set(name, [...(listeners.get(name) ?? []), args.handler as number]);
      return args.handler;
    }
    if (cmd === "plugin:event|unlisten") return;
    const route = routes[cmd];
    return route ? route(args) : null;
  },
};

/** Fires `name` to the page's listen() handlers. */
export function emit(name: string, payload: unknown) {
  for (const id of listeners.get(name) ?? []) callbacks.get(id)?.({ event: name, id, payload });
}
