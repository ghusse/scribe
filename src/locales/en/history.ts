import type { Outcome } from "../../lib/api";

export const history = {
  search: "Search the history…",
  copiedAnnouncement: "Text copied",
  empty: "No dictations yet. Hold the trigger key and speak.",
  outcome: { pasted: "Inserted", pasted_uncertain: "Inserted?", clipboard: "Copied", error: "Error" } satisfies Record<Outcome, string>,
  saveEdit: "Save correction",
  cancel: "Cancel",
  raw: "Raw",
  corrected: "Corrected",
  yourCorrection: "Your correction",
  models: "Models",
  copy: "Copy",
  copied: "✓ Copied",
  edit: "Correct",
  retranscribe: "Transcribe again",
  less: "Less",
  details: "Details",
  confirmDelete: "Confirm deletion",
  delete: "Delete",
  more: "More",
};
