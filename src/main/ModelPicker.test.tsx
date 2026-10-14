import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/preact";
import { useState } from "preact/hooks";
import ModelPicker from "./ModelPicker";

const CUSTOM_LABEL = "Identifiant du modèle personnalisé";

/**
 * Renders the picker under a stateful parent, as App does: `props.value` is what the parent holds, and
 * `seen` records it at each `onChange` call (the parent saves the settings from there).
 */
function setup(models: string[], initial: string) {
  const seen: string[] = [];
  const props = { value: initial, onChange: vi.fn(() => seen.push(props.value)) };
  function Parent() {
    const [value, setValue] = useState(initial);
    return (
      <ModelPicker
        models={models}
        value={value}
        onValueChange={(v) => {
          props.value = v;
          setValue(v);
        }}
        onChange={props.onChange}
      />
    );
  }
  render(<Parent />);
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
    expect(props.onChange).toHaveBeenCalledTimes(1);
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
    expect(props.onChange).not.toHaveBeenCalled();
  });

  it("rejects an empty custom id: error shown, value and settings untouched", async () => {
    const { props } = setup(["a-2"], "my-model");
    const input = screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: "   " } });
    expect(screen.getByRole("alert").textContent).toBe("Identifiant du modèle : ne peut pas être vide");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(props.value).toBe("my-model");
    expect(props.onChange).not.toHaveBeenCalled();
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
    expect(props.onChange).not.toHaveBeenCalled();
  });

  it("keeps a rejected id as typed in the input", async () => {
    setup(["a-2"], "my-model");
    const input = screen.getByLabelText(CUSTOM_LABEL) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: "   " } });
    expect(input.value).toBe("   ");
  });
});
