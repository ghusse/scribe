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

La fenêtre principale s'ouvre à chaque lancement ; fermée, Scribe reste dans la zone de notification. Lancé avec
`--minimized`, il démarre directement dans la zone de notification, sauf s'il manque une clé. Réglages > Démarrage >
« Lancer Scribe à l'ouverture de session » l'inscrit au démarrage de la session (Windows et macOS), avec ce mode
discret.

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

## Publier une version

```bash
# Les bundles de mise à jour sont signés : la clé privée et son mot de passe sont lus dans ~/.tauri.
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/scribe.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/scribe.key.password)"   bun tauri build   # target/release/bundle/nsis/Scribe_<version>_x64-setup.exe (+ .sig), et un .msi
```

Pour une release GitHub : monter la version dans `src-tauri/tauri.conf.json`, `package.json` et les `Cargo.toml`,
puis pousser un tag `v<version>` (`git tag v0.2.0 && git push origin v0.2.0`). Le workflow
`.github/workflows/release.yml` vérifie que le tag correspond à la version, construit les installeurs et les joint à
une release en brouillon, à publier à la main. Les installeurs ne sont pas signés : au premier lancement, Windows
SmartScreen demande « Informations complémentaires » puis « Exécuter quand même ».

Mises à jour : la release publiée contient `latest.json`. Les versions installées le consultent 30 s après leur
lancement (un toast annonce une nouvelle version) et depuis Réglages > Mises à jour, qui l'installe et redémarre
Scribe. Chaque mise à jour est vérifiée avec la clé publique de `tauri.conf.json` ; la clé privée et son mot de
passe sont les secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` du dépôt. Les perdre
empêche toute mise à jour des versions installées : gardez-en une copie.

## Confidentialité et installation

Pas de télémétrie ni de compte. Ce que Scribe envoie sur le réseau (l'audio et le texte des dictées aux fournisseurs
choisis, la vérification des mises à jour sur GitHub) et les accès système qu'il utilise (hook clavier,
presse-papier, champ actif) sont détaillés dans [PRIVACY.md](PRIVACY.md).

Les installeurs ne sont pas signés (Authenticode) : à la première installation, SmartScreen affiche « Windows a
protégé votre ordinateur », cliquez sur « Informations complémentaires » puis « Exécuter quand même ». Les mises à
jour, téléchargées par Scribe lui-même, ne déclenchent pas cet avertissement.

## Licence

Copyright (C) 2026 Guillaume Gautreau.

Scribe est distribué sous la licence [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`). Vous pouvez
l'utiliser, l'étudier, le modifier et le redistribuer ; toute version modifiée distribuée ou mise à disposition
d'utilisateurs, y compris à travers un service en ligne, doit l'être sous la même licence, avec son code source
complet.
