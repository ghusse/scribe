# Scribe

Dictée vocale pour Windows (macOS à venir) : maintenez une touche, parlez, le texte corrigé
s'insère dans le champ actif.

## Lancer en développement

```bash
rustup update stable
bun install
bun tauri dev
```

Au premier lancement (clés manquantes), la fenêtre principale s'ouvre sur l'onglet Réglages : choisissez un fournisseur
et un modèle pour la transcription et pour la correction (par défaut OpenAI `gpt-transcribe` et Anthropic
`claude-opus-5-5`), renseignez les clés API correspondantes, puis « Tester la configuration ».

## Utilisation

- **Maintenir** Ctrl droit : push-to-talk, relâcher pour transcrire.
- **Double-tap** Ctrl droit : mode verrouillé, un nouvel appui arrête.
- **Maintenir Ctrl droit + Espace** : verrouille en cours de dictée.
- Le raccourci peut être une combinaison de 1 à 4 touches (Réglages > Raccourci > Changer, puis appuyer sur la
  combinaison et relâcher), par exemple `Ctrl + Maj + A` ou `Win + Alt`. Elle se déclenche quand toutes ses
  touches sont tenues et aucune autre ; dans une combinaison, Ctrl/Maj/Alt/Win valent des deux côtés. La touche
  non modificatrice (le A) est avalée ; les modificateurs passent, et Alt/Win n'ouvrent pas de menu.
- Si aucun champ texte n'est actif, le texte est copié et un toast propose de le voir.

## Tests et couverture

```bash
cargo test --workspace
bun run test
bun run check
bun run coverage   # porte : >= 95 % des lignes en Rust, TypeScript et Svelte
```

Règle : au moins 95 % de couverture de lignes dans tous les langages, vérifiée par le CI
(`.github/workflows/ci.yml`, Windows). Rust via `cargo-llvm-cov` (`rustup component add llvm-tools-preview`,
`cargo install cargo-llvm-cov`), UI via vitest exécuté par bun + `@vitest/coverage-istanbul` (composants testés avec
`@testing-library/svelte` + happy-dom, corrigé par `patches/happy-dom@20.14.5.patch` pour que `:checked` trouve
l'option choisie d'un `<select>`). Les seules exclusions autorisées (adaptateurs OS/framework sans logique) et
leur justification sont listées dans [`CLAUDE.md`](CLAUDE.md).

## Checklist manuelle (Windows)

- [ ] Bloc-notes : maintien → texte inséré, presse-papier précédent restauré.
- [ ] VS Code, Slack, Chrome (champ de recherche, Gmail) : texte inséré (toast « Texte inséré ? » toléré).
- [ ] Windows Terminal : texte collé ou toast avec « Copier ».
- [ ] Bureau / Explorateur sans champ : toast « Texte copié », Ctrl+V colle le texte.
- [ ] Changer de fenêtre pendant la transcription : pas de collage, toast « Texte copié ».
- [ ] Copier une image, dicter, vérifier que l'image est restaurée.
- [ ] Tenir la touche 3 s sans parler : rien n'est inséré.
- [ ] Double-tap : pastille « Verrouillé », Ctrl+Espace ne parvient pas à l'application.
- [ ] Raccourci `Ctrl + Maj + A` : dicte avec l'un ou l'autre Ctrl ; aucun « a » tapé ; Ctrl+A seul sélectionne toujours tout.
- [ ] Raccourci `Win + Alt` : dicte, et le menu Démarrer ne s'ouvre pas au relâchement.
- [ ] Pendant « Changer », Win/Alt/lettres n'ont aucun effet dans Windows ; Échap annule.
- [ ] Win+L pendant le maintien : la dictée est abandonnée (rien n'est collé après déverrouillage) ; le raccourci et Espace fonctionnent ensuite normalement.
- [ ] AZERTY : raccourci « Alt droit (AltGr) » seul déclenche ; avec `Ctrl + Alt` comme raccourci, taper @ ou € ne déclenche rien.
- [ ] Plusieurs dispositions installées : `Ctrl + Maj + A` ne change pas la disposition du clavier.
- [ ] La pastille n'enlève jamais le focus au champ.
- [ ] Clé Anthropic invalide : texte brut inséré, toast « Inséré sans correction ».
- [ ] Wi-Fi coupé : toast d'erreur, dictée « Erreur » dans l'historique, « Retranscrire » fonctionne après reconnexion.
- [ ] Ajouter « Kubernetes » au glossaire, dicter « cube ernetes » : orthographe corrigée, compteur d'usages incrémenté.
- [ ] Tray : Pause désactive le raccourci ; Quitter ferme l'app ; fermer la fenêtre la masque seulement.
