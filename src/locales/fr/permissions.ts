import type { permissions as En } from "../en/permissions";

export const permissions: typeof En = {
  panel: "Autorisations manquantes",
  heading: "Scribe a besoin de votre autorisation",
  allow: "Autoriser",
  openSettings: "Ouvrir les Réglages Système",
  requestFailed: (detail) => `Demande impossible : ${detail}`,
  openFailed: (detail) => `Ouverture des Réglages impossible : ${detail}`,
  copy: {
    accessibility: {
      title: "Accessibilité",
      why: "Sans elle, le raccourci ne répond pas et Scribe ne peut ni coller le texte ni vérifier qu'il est arrivé.",
      how: "Cliquez sur « Autoriser », puis activez Scribe dans Réglages Système : le raccourci marche aussitôt, sans relancer Scribe. Si Scribe y est déjà coché (après une mise à jour, l'autorisation ne vaut plus), « Autoriser » la remet à zéro : il suffit de le réactiver.",
    },
    microphone: {
      title: "Micro",
      why: "Sans lui, Scribe n'entend rien : les dictées sont vides.",
      how: "Cliquez sur « Autoriser » et acceptez la demande de macOS. Si vous l'avez refusée, activez Scribe dans Réglages Système > Micro.",
    },
  },
};
