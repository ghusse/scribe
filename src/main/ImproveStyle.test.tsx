import { describe, expect, it, vi } from "bun:test";
import { fireEvent, render, screen, within } from "@testing-library/preact";
import { calls, commands } from "../../tests/tauri";
import ImproveStyle from "./ImproveStyle";

async function setup() {
  const onAccept = vi.fn();
  const onClose = vi.fn();
  render(<ImproveStyle id="improve-slack" name="Slack" keyword="slack" draft="- Court." onAccept={onAccept} onClose={onClose} />);
  return { onAccept, onClose };
}

const request = () => screen.getByLabelText("Que doit faire ce style ?") as HTMLTextAreaElement;
const improve = () => screen.getByRole("button", { name: /Générer|Génération…/ }) as HTMLButtonElement;

async function ask(text: string) {
  await fireEvent.input(request(), { target: { value: text } });
  await fireEvent.click(improve());
}

describe("ImproveStyle", () => {
  it("focuses the request, and cancel closes", async () => {
    const { onClose } = await setup();
    expect(document.activeElement).toBe(request());
    await fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("needs a request before asking", async () => {
    await setup();
    expect(improve().disabled).toBe(true);
    await fireEvent.input(request(), { target: { value: "  " } });
    expect(improve().disabled).toBe(true);
    await fireEvent.input(request(), { target: { value: "Plus court" } });
    expect(improve().disabled).toBe(false);
  });

  it("sends the style, the draft and the request, then previews the proposal", async () => {
    let answer!: (v: string) => void;
    commands({ improve_style: () => new Promise<string>((res) => (answer = res)) });
    const { onAccept } = await setup();
    await ask("Plus court");
    expect(calls("improve_style")).toEqual([{ request: { name: "Slack", keyword: "slack", draft: "- Court.", instructions: "Plus court" } }]);
    expect(improve().textContent).toBe("Génération…");
    expect((screen.getByRole("button", { name: "Annuler" }) as HTMLButtonElement).disabled).toBe(true);
    expect(improve().disabled).toBe(true);
    answer("- Très court.");
    const proposal = await screen.findByRole("group", { name: "Instructions proposées" });
    expect(within(proposal).getByText("- Très court.")).toBeTruthy();
    expect(onAccept).not.toHaveBeenCalled();
  });

  it("uses the proposal and closes", async () => {
    commands({ improve_style: () => "- Nouveau." });
    const { onAccept, onClose } = await setup();
    await ask("Plus court");
    await fireEvent.click(await screen.findByRole("button", { name: "Utiliser" }));
    expect(onAccept).toHaveBeenCalledWith("- Nouveau.");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("discards the proposal without changing the task", async () => {
    commands({ improve_style: () => "- Nouveau." });
    const { onAccept } = await setup();
    await ask("Plus court");
    await fireEvent.click(await screen.findByRole("button", { name: "Ignorer" }));
    expect(onAccept).not.toHaveBeenCalled();
    expect(screen.queryByRole("group")).toBeNull();
    expect(request().value).toBe("Plus court");
  });

  it("shows a provider error, and a new attempt clears it", async () => {
    commands({ improve_style: () => Promise.reject({ code: "provider", error: { code: "no_output_tag" } }) });
    await setup();
    await ask("Plus court");
    expect((await screen.findByRole("alert")).textContent).toBe("Amélioration impossible : réponse du correcteur sans balise <output>");
    commands({ improve_style: () => "- Ok." });
    await fireEvent.click(improve());
    expect(await screen.findByText("- Ok.")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
