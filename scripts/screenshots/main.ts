// Main window with demo data; `?tab=` opens a tab, `?section=` hides the sections above that heading.
import "./tauri-demo";
import "../../src/main/main";

const params = new URLSearchParams(location.search);

function whenFound<T>(find: () => T | null | undefined, then: (found: T) => void) {
  const found = find();
  if (found) then(found);
  else setTimeout(() => whenFound(find, then), 20);
}

const byText = (selector: string, text: string | null) =>
  [...document.querySelectorAll<HTMLElement>(selector)].find((e) => e.textContent?.trim() === text);

const tab = params.get("tab");
const section = params.get("section");
if (tab) whenFound(() => byText("nav button", tab), (b) => b.click());
if (section)
  whenFound(
    () => byText("h2", section)?.closest("section"),
    (s) => {
      for (let e = s.previousElementSibling; e; e = e.previousElementSibling) (e as HTMLElement).hidden = true;
    },
  );
