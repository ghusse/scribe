import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import ModelPicker from "./ModelPicker.svelte";

const CUSTOM_LABEL = "Identifiant du modèle personnalisé";

/**
 * Renders the picker with an observable `bind:value`: Svelte 5 writes a bound prop through the setter of
 * the props object, so `props.value` is what the parent would see. `seen` records the value at each
 * `onchange` call (the parent saves the settings from there).
 */
function setup(models: string[], initial: string) {
  const seen: string[] = [];
  let value = initial;
  const props = {
    models,
    get value() {
      return value;
    },
    set value(v: string) {
      value = v;
    },
    onchange: vi.fn(() => seen.push(value)),
  };
  render(ModelPicker, { props });
  return { props, seen, select: screen.getByRole("combobox") as HTMLSelectElement };
}

describe("ModelPicker", () => {
  it("labels the first (newest) model as the default and offers a custom entry", () => {
    setup(["a-2", "a-1"], "a-2");
    const labels = screen.getAllByRole("option").map((o) => o.textContent);
    expect(labels).toEqual(["a-2 (dernier, par défaut)", "a-1", "Autre…"]);
  });

  it("selects the current known model without the custom input", () => {
    const { select } = setup(["a-2", "a-1"], "a-1");
    expect(select.value).toBe("a-1");
    expect(screen.queryByLabelText(CUSTOM_LABEL)).toBeNull();
  });

  it("binds a picked known model and notifies once, after the value is updated", async () => {
    const { props, seen, select } = setup(["a-2", "a-1"], "a-2");
    await fireEvent.change(select, { target: { value: "a-1" } });
    expect(props.value).toBe("a-1");
    expect(props.onchange).toHaveBeenCalledTimes(1);
    expect(seen).toEqual(["a-1"]);
  });

  it("shows the custom input for an unknown model id", () => {
    const { select } = setup(["a-2"], "my-model");
    expect(select.value).toBe("__custom__");
    expect((screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement).value).toBe("my-model");
  });

  it("switching to « Autre… » keeps the current model until an id is committed", async () => {
    const { props, select } = setup(["a-2", "a-1"], "a-2");
    await fireEvent.change(select, { target: { value: "__custom__" } });
    const input = (await screen.findByLabelText(CUSTOM_LABEL)) as HTMLInputElement;
    expect(input.value).toBe("a-2");
    await vi.waitFor(() => expect(document.activeElement).toBe(input));
    expect(props.value).toBe("a-2");
    expect(props.onchange).not.toHaveBeenCalled();
  });

  it("rejects an empty custom id: error shown, value and settings untouched", async () => {
    const { props } = setup(["a-2"], "my-model");
    const input = screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: "   " } });
    expect(screen.getByRole("alert").textContent).toBe("Identifiant du modèle : ne peut pas être vide");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(props.value).toBe("my-model");
    expect(props.onchange).not.toHaveBeenCalled();
  });

  it("commits a valid custom id trimmed, clears the error and notifies once", async () => {
    const { props, seen } = setup(["a-2"], "my-model");
    const input = screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: "" } });
    expect(screen.getByRole("alert")).toBeTruthy();

    await fireEvent.change(input, { target: { value: "  other-model  " } });
    expect(props.value).toBe("other-model");
    expect(input.value).toBe("other-model");
    expect(seen).toEqual(["other-model"]);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(input.getAttribute("aria-invalid")).toBe("false");
  });

  it("committing the current id again is not a change", async () => {
    const { props } = setup(["a-2"], "my-model");
    const input = screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: " my-model " } });
    expect(props.value).toBe("my-model");
    expect(props.onchange).not.toHaveBeenCalled();
  });
});
