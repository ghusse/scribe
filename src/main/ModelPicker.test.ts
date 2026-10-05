import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import ModelPicker from "./ModelPicker.svelte";

// Smoke test for the component-testing harness (jsdom + @testing-library/svelte).
describe("ModelPicker", () => {
  it("selects a known model and notifies the change", async () => {
    const onchange = vi.fn();
    render(ModelPicker, { models: ["a-2", "a-1"], value: "a-2", onchange });
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("a-2");
    expect(screen.queryByLabelText("Identifiant du modèle personnalisé")).toBeNull();

    await fireEvent.change(select, { target: { value: "a-1" } });
    expect(onchange).toHaveBeenCalledTimes(1);
    expect(select.value).toBe("a-1");
  });

  it("shows the custom input for an unknown model id", () => {
    render(ModelPicker, { models: ["a-2"], value: "my-model" });
    const input = screen.getByLabelText("Identifiant du modèle personnalisé") as HTMLInputElement;
    expect(input.value).toBe("my-model");
  });
});
