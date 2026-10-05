/** « Mal entendu » field → variants: comma-separated, trimmed, blanks dropped. */
export function splitVariants(s: string): string[] {
  return s.split(",").map((v) => v.trim()).filter(Boolean);
}

/** Variants → the editable field (inverse of splitVariants for clean input). */
export function joinVariants(variants: string[]): string {
  return variants.join(", ");
}

/** An empty note is no note. */
export function noteOrNull(note: string): string | null {
  return note.trim() === "" ? null : note;
}
