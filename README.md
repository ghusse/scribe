# Scribe

Dictée vocale pour Windows (macOS à venir) : maintenez une touche, parlez, le texte corrigé
s'insère dans le champ actif.

## Lancer en développement

```bash
rustup update stable
npm install
npx tauri dev
```

Au premier lancement (clés manquantes), la fenêtre principale s'ouvre sur l'onglet Réglages : choisissez un fournisseur
et un modèle pour la transcription et pour la correction (par défaut OpenAI `gpt-transcribe` et Anthropic
`claude-opus-5-5`), renseignez les clés API correspondantes, puis « Tester la configuration ».

## Utilisation

- **Maintenir** Ctrl droit : push-to-talk, relâcher pour transcrire.
- **Double-tap** Ctrl droit : mode verrouillé, un nouvel appui arrête.
- **Maintenir Ctrl droit + Espace** : verrouille en cours de dictée.
- Si aucun champ texte n'est actif, le texte est copié et un toast propose de le voir.

## Tests

```bash
cargo test --workspace
npm run check
```

## Checklist manuelle (Windows)

- [ ] Bloc-notes : maintien → texte inséré, presse-papier précédent restauré.
- [ ] VS Code, Slack, Chrome (champ de recherche, Gmail) : texte inséré (toast « Texte inséré ? » toléré).
- [ ] Windows Terminal : texte collé ou toast avec « Copier ».
- [ ] Bureau / Explorateur sans champ : toast « Texte copié », Ctrl+V colle le texte.
- [ ] Changer de fenêtre pendant la transcription : pas de collage, toast « Texte copié ».
- [ ] Copier une image, dicter, vérifier que l'image est restaurée.
- [ ] Tenir la touche 3 s sans parler : rien n'est inséré.
- [ ] Double-tap : pastille « Verrouillé », Ctrl+Espace ne parvient pas à l'application.
- [ ] La pastille n'enlève jamais le focus au champ.
- [ ] Clé Anthropic invalide : texte brut inséré, toast « Inséré sans correction ».
- [ ] Wi-Fi coupé : toast d'erreur, dictée « Erreur » dans l'historique, « Retranscrire » fonctionne après reconnexion.
- [ ] Ajouter « Kubernetes » au glossaire, dicter « cube ernetes » : orthographe corrigée, compteur d'usages incrémenté.
- [ ] Tray : Pause désactive le raccourci ; Quitter ferme l'app ; fermer la fenêtre la masque seulement.
