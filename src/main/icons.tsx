import type { ComponentChildren } from "preact";

/** 16 px stroke icon in the current text colour; decorative, the button label carries the meaning. */
function Icon({ children }: { children: ComponentChildren }) {
  return (
    <svg
      class="icon"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const Sparkles = () => (
  <Icon>
    <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" />
    <path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" />
  </Icon>
);

export const Undo = () => (
  <Icon>
    <path d="M3 7v6h6" />
    <path d="M21 17a9 9 0 0 0-15-6.7L3 13" />
  </Icon>
);

export const Trash = () => (
  <Icon>
    <path d="M3 6h18" />
    <path d="M8 6V4h8v2" />
    <path d="M6 6l1 14h10l1-14" />
  </Icon>
);

export const Plus = () => (
  <Icon>
    <path d="M12 5v14M5 12h14" />
  </Icon>
);

export const Check = () => (
  <Icon>
    <path d="M5 12l5 5 9-10" />
  </Icon>
);
