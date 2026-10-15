import { describe, expect, test } from "bun:test";
import type { AppError, Notice, ProviderError, SettingsError } from "./api";
import { errorText, noticeText, providerErrorDetail, providerErrorText, settingsErrorText, storedErrorText } from "./errors";
import { lang } from "./i18n";
import { PRESETS, STYLES } from "../../tests/fixtures";

describe("providerErrorText", () => {
  test.each<[ProviderError, string | null, string]>([
    [{ code: "missing_key", provider: "OpenAI" }, null, "aucune clé OpenAI enregistrée, ajoutez-la dans Clés API"],
    [{ code: "auth" }, "Anthropic", "clé refusée par Anthropic, vérifiez la clé"],
    [{ code: "auth" }, null, "clé refusée, vérifiez la clé"],
    [{ code: "http", status: 401, body: "" }, "Groq", "clé refusée par Groq, vérifiez la clé"],
    [{ code: "http", status: 403, body: "" }, null, "clé refusée, vérifiez la clé"],
    [{ code: "http", status: 404, body: "" }, null, "modèle inconnu chez ce fournisseur"],
    [{ code: "http", status: 400, body: "" }, null, "requête refusée par le fournisseur (HTTP 400), voir Détails"],
    [{ code: "http", status: 429, body: "" }, null, "trop de requêtes, réessayez dans un instant"],
    [{ code: "http", status: 503, body: "" }, null, "erreur du fournisseur (HTTP 503), réessayez plus tard"],
    [{ code: "http", status: 418, body: "" }, null, "erreur du fournisseur (HTTP 418), voir Détails"],
    [{ code: "network", detail: "reset" }, null, "fournisseur injoignable"],
    [{ code: "timeout" }, null, "fournisseur injoignable"],
    [{ code: "unknown_provider", role: "correction", id: "x" }, null, "fournisseur inconnu"],
    [{ code: "refusal" }, null, "le modèle a refusé la requête"],
    [{ code: "malformed", detail: "{}" }, null, "réponse invalide du fournisseur, voir Détails"],
    [{ code: "correction_disabled" }, null, "correction désactivée"],
    [{ code: "correction_timeout" }, null, "délai du correcteur dépassé"],
    [{ code: "no_output_tag" }, null, "réponse du correcteur sans balise <output>"],
    [{ code: "config", detail: "tls" }, null, "configuration du fournisseur invalide, voir Détails"],
  ])("%o", (e, provider, text) => {
    expect(providerErrorText(e, provider)).toBe(text);
  });

  test("follows the current language", () => {
    lang.value = "en";
    expect(providerErrorText({ code: "auth" }, "Mistral")).toBe("key refused by Mistral, check the key");
  });
});

test("providerErrorDetail keeps the technical text only", () => {
  expect(providerErrorDetail({ code: "network", detail: "dns" })).toBe("dns");
  expect(providerErrorDetail({ code: "malformed", detail: "{}" })).toBe("{}");
  expect(providerErrorDetail({ code: "config", detail: "tls" })).toBe("tls");
  expect(providerErrorDetail({ code: "http", status: 500, body: "boom" })).toBe("HTTP 500: boom");
  expect(providerErrorDetail({ code: "auth" })).toBeNull();
});

test.each<[SettingsError, string]>([
  [{ code: "invalid_hotkey" }, "raccourci invalide"],
  [{ code: "escape_in_hotkey" }, "Échap ne peut pas faire partie du raccourci"],
  [{ code: "lock_key_in_hotkey" }, "la touche de verrouillage ne peut pas faire partie du raccourci"],
  [{ code: "max_recording_too_short", min_seconds: 10 }, "la durée maximale doit être d'au moins 10 secondes"],
  [{ code: "max_recording_too_long", max_minutes: 30 }, "la durée maximale ne peut pas dépasser 30 minutes"],
  [{ code: "min_recording_not_below_max" }, "la durée minimale doit être inférieure à la durée maximale"],
  [{ code: "silence_threshold_out_of_range", min_dbfs: -90, max_dbfs: -10 }, "le seuil de silence doit être compris entre -90 et -10 dBFS"],
  [{ code: "hotkey_delay_zero" }, "les délais du raccourci doivent être supérieurs à 0 ms"],
  [{ code: "audio_retention_too_long", max_days: 365 }, "la durée de conservation de l'audio ne peut pas dépasser 365 jours"],
  [{ code: "restore_delay_too_long", max_ms: 2000 }, "le délai de restauration du presse-papiers ne peut pas dépasser 2000 ms"],
  [{ code: "correction_timeout_too_long" }, "le délai de correction est trop élevé"],
  [{ code: "unknown_provider", role: "transcription", id: "x" }, "fournisseur de transcription inconnu : x"],
  [{ code: "model_required" }, "choisissez un modèle de transcription et un modèle de correction"],
])("settingsErrorText %o", (e, text) => {
  expect(settingsErrorText(e)).toBe(text);
});

describe("settingsErrorText: style errors name the style", () => {
  const custom = { id: "u1", preset: null, name: "Compte rendu", keyword: "cr", aliases: ["c r"], custom_task: null };
  const ctx = { styles: [...STYLES, custom], presets: PRESETS };
  test.each<[SettingsError, string]>([
    [{ code: "default_style_missing" }, "le style par défaut est obligatoire, et chaque autre style a besoin d'un mot-clé"],
    [{ code: "duplicate_style_id" }, "deux styles ont le même identifiant"],
    [{ code: "too_many_styles", max: 10 }, "10 styles au maximum"],
    [{ code: "too_many_aliases", index: 3, max: 8 }, "Mail : 8 autres formes entendues au maximum"],
    [{ code: "style_name_required", index: 4 }, "Compte rendu : donnez un nom au style"],
    [{ code: "trigger_empty", index: 2 }, "Slack : un mot-clé ou une forme entendue doit contenir au moins une lettre ou un chiffre"],
    [{ code: "duplicate_trigger", trigger: "c r" }, "Compte rendu : « c r » est déjà utilisé par un autre style"],
    [{ code: "duplicate_trigger", trigger: "zz" }, "« zz » est utilisé par deux styles"],
    [{ code: "task_empty", index: 0 }, "Par défaut : rédigez ses instructions"],
    [{ code: "task_too_long", index: 1, max_chars: 4000 }, "Propre : instructions de plus de 4000 caractères"],
  ])("%o", (e, text) => {
    expect(settingsErrorText(e, ctx)).toBe(text);
    expect(errorText({ code: "invalid_settings", error: e }, ctx)).toBe(text);
  });

  test("without the styles, a style is named by its position", () => {
    expect(settingsErrorText({ code: "task_empty", index: 4 })).toBe("Style 5 : rédigez ses instructions");
    lang.value = "en";
    expect(settingsErrorText({ code: "duplicate_trigger", trigger: "x" })).toBe('"x" is used by two styles');
  });
});

describe("errorText", () => {
  test.each<[AppError, string]>([
    [{ code: "system", detail: "disk full" }, "disk full"],
    [{ code: "dictation_not_found" }, "dictée introuvable"],
    [{ code: "no_text" }, "cette dictée n'a pas de texte"],
    [{ code: "audio_purged" }, "l'audio de cette dictée a été purgé"],
    [{ code: "no_speech" }, "aucune parole détectée"],
    [{ code: "duplicate_term", term: "Tauri" }, "terme déjà présent dans le glossaire : Tauri"],
    [{ code: "invalid_settings", error: { code: "model_required" } }, "choisissez un modèle de transcription et un modèle de correction"],
    [{ code: "provider", error: { code: "timeout" } }, "fournisseur injoignable"],
    [{ code: "no_update" }, "aucune mise à jour disponible"],
    [{ code: "style_not_found" }, "ce style n'existe plus"],
    [{ code: "nothing_to_restyle" }, "cette dictée n'a pas de transcription à réécrire"],
  ])("%o", (e, text) => {
    expect(errorText(e)).toBe(text);
  });

  test("anything else is shown as is", () => {
    expect(errorText("boom")).toBe("boom");
    expect(errorText(new Error("bad"))).toBe("Error: bad");
    expect(errorText(null)).toBe("null");
  });
});

test("storedErrorText reads a JSON provider error, else the legacy text", () => {
  expect(storedErrorText('{"code":"timeout"}')).toBe("fournisseur injoignable");
  expect(storedErrorText("délai dépassé")).toBe("délai dépassé");
  expect(storedErrorText("42")).toBe("42");
});

describe("noticeText", () => {
  const timeout: ProviderError = { code: "timeout" };
  test.each<[Notice, string, string | null]>([
    [{ code: "pasted" }, "Texte inséré", null],
    [{ code: "pasted_without_correction", correction: timeout }, "Inséré sans correction", "fournisseur injoignable"],
    [{ code: "pasted_uncertain", correction: null }, "Texte inséré ?", null],
    [{ code: "pasted_uncertain", correction: timeout }, "Texte inséré ?", "Non corrigé : fournisseur injoignable"],
    [{ code: "copied", correction: null }, "Texte copié dans le presse-papier", null],
    [{ code: "not_pasted", correction: null }, "Non inséré : texte copié, collez-le avec Ctrl+V", null],
    [{ code: "clipboard_unavailable", correction: null }, "Presse-papier indisponible : texte dans l'historique", null],
    [{ code: "transcription_failed", error: { code: "auth" } }, "Échec de la transcription : clé refusée, vérifiez la clé. Réessayez depuis l'historique.", null],
    [{ code: "audio_encoding_failed", detail: "x" }, "Encodage audio impossible : x", null],
    [{ code: "microphone_unavailable", detail: "x" }, "Micro indisponible : x", null],
    [{ code: "recording_lost", detail: "x" }, "Enregistrement perdu : x", null],
    [{ code: "hook_unavailable", detail: "x" }, "Raccourci indisponible : x", null],
    [{ code: "update_available", version: "1.2.0" }, "Scribe 1.2.0 est disponible : Réglages > Général", null],
    [{ code: "copy_failed", detail: "occupé" }, "Copie impossible : occupé", null],
    [{ code: "restyled" }, "Texte remplacé", null],
    [{ code: "restyle_copied" }, "Nouveau texte copié", null],
    [{ code: "restyle_failed", error: { code: "timeout" } }, "Réécriture impossible : fournisseur injoignable", null],
    [{ code: "restyle_refused", detail: "ce style n'existe plus" }, "Réécriture impossible : ce style n'existe plus", null],
  ])("%o", (n, title, detail) => {
    expect(noticeText(n, "windows")).toEqual({ title, detail });
  });

  test("names the macOS paste shortcut on a Mac", () => {
    expect(noticeText({ code: "not_pasted", correction: null }, "mac").title).toBe("Non inséré : texte copié, collez-le avec ⌘V");
  });
});
