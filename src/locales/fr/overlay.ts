import type { overlay as En } from "../en/overlay";

export const overlay: typeof En = {
  recording: "Enregistrement",
  locked: "Verrouillé",
  processing: "Transcription…",
  copy: "Copier",
  view: "Voir",
  style: (name) => `Style : ${name}`,
  close: "Fermer",
  changeStyle: "Changer de style",
  styles: "Styles",
  rewriting: (name) => `Réécriture en style ${name}…`,
  warnings: { shared_microphone: "Micro non exclusif : les autres applications vous entendent" },
};
