import { describe, expect, it } from "bun:test";
import type { Style } from "./api";
import { lang } from "./i18n";
import {
  canDelete,
  canReset,
  displayOrder,
  editTask,
  missingBuiltins,
  namePlaceholder,
  newStyle,
  resetTask,
  styleName,
  taskView,
  triggerOwner,
} from "./styles";
import { PRESETS, STYLES } from "../../tests/fixtures";

const [def, clean, slack] = STYLES;
const user = (over: Partial<Style> = {}): Style => ({ ...newStyle("u1"), ...over });

describe("newStyle", () => {
  it("is a user style with an empty keyword and no task", () => {
    expect(newStyle("abc")).toEqual({ id: "abc", preset: null, name: null, keyword: "", aliases: [], custom_task: null });
  });
});

describe("styleName", () => {
  it("prefers the name, then the preset's, then the keyword, then the position", () => {
    expect(styleName({ ...slack, name: "  Équipe " }, PRESETS, 2)).toBe("Équipe");
    expect(styleName({ ...clean, name: "  " }, PRESETS, 1)).toBe("Propre");
    expect(styleName(user({ keyword: " cr " }), PRESETS, 4)).toBe("cr");
    expect(styleName(user(), PRESETS, 4)).toBe("Style 5");
    expect(styleName(undefined, [], 0)).toBe("Style 1");
  });
  it("names a built-in by its preset id until the catalog is loaded", () => {
    expect(styleName(def, [], 0)).toBe("Style 1");
    expect(styleName(slack, [], 2)).toBe("slack");
  });
  it("follows the UI language", () => {
    lang.value = "en";
    expect(styleName(user(), [], 0)).toBe("Style 1");
  });
});

describe("namePlaceholder", () => {
  it("is the preset's name for a built-in, empty for a user style", () => {
    expect(namePlaceholder(slack, PRESETS)).toBe("Slack");
    expect(namePlaceholder(user(), PRESETS)).toBe("");
  });
});

describe("taskView / editTask / resetTask", () => {
  it("shows the built-in text, which follows the app language", () => {
    expect(taskView(def, PRESETS)).toEqual({ text: "- Corrige et mets en forme.", language: null });
    expect(taskView(user(), PRESETS)).toEqual({ text: "", language: null });
  });

  it("the first edit makes the task custom, in the UI language", () => {
    const edited = editTask(def, "- Autre", "fr", PRESETS);
    expect(edited.custom_task).toEqual({ text: "- Autre", language: "fr" });
    expect(taskView(edited, PRESETS)).toEqual({ text: "- Autre", language: "fr" });
    expect(editTask(edited, "- Other", "en", PRESETS).custom_task).toEqual({ text: "- Other", language: "en" });
    expect(def.custom_task).toBeNull();
  });

  it("committing the built-in text unchanged keeps it built-in", () => {
    expect(editTask(def, "- Corrige et mets en forme.", "fr", PRESETS)).toBe(def);
  });

  it("a custom task set back to the built-in text stays custom (reset is explicit)", () => {
    const edited = editTask(def, "- Autre", "fr", PRESETS);
    expect(editTask(edited, "- Corrige et mets en forme.", "fr", PRESETS).custom_task).toEqual({ text: "- Corrige et mets en forme.", language: "fr" });
  });

  it("before the presets are loaded, any committed text makes the task custom", () => {
    expect(editTask(def, "- Corrige et mets en forme.", "fr", []).custom_task).toEqual({ text: "- Corrige et mets en forme.", language: "fr" });
  });

  it("reset drops the custom task", () => {
    const edited = editTask(slack, "- x", "fr", PRESETS);
    expect(canReset(edited)).toBe(true);
    expect(resetTask(edited)).toEqual(slack);
    expect(canReset(slack)).toBe(false);
    expect(canReset(user({ custom_task: { text: "x", language: "fr" } }))).toBe(false);
  });
});

describe("canDelete", () => {
  it("every style but the default", () => {
    expect(STYLES.map(canDelete)).toEqual([false, true, true, true]);
    expect(canDelete(user())).toBe(true);
  });
});

describe("displayOrder", () => {
  it("puts the default first and keeps the others in order", () => {
    expect(displayOrder([slack, def, clean])).toEqual([1, 0, 2]);
    expect(displayOrder(STYLES)).toEqual([0, 1, 2, 3]);
    expect(displayOrder([])).toEqual([]);
  });
});

describe("missingBuiltins", () => {
  it("lists the presets with no style of that id", () => {
    expect(missingBuiltins(STYLES, PRESETS)).toEqual([]);
    expect(missingBuiltins([def, slack], PRESETS)).toEqual(["clean", "mail"]);
    expect(missingBuiltins([def], [])).toEqual([]);
  });
});

describe("triggerOwner", () => {
  it("finds the last style using the trigger as typed", () => {
    const styles = [...STYLES, user({ keyword: "slack" })];
    expect(triggerOwner(styles, "slack")).toBe(4);
    expect(triggerOwner(styles, "courriel")).toBe(3);
    expect(triggerOwner(styles, "Slack")).toBeNull();
  });
});
