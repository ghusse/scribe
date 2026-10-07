# Scribe

Dictée vocale pour Windows et macOS : maintenez une touche, parlez, le texte corrigé
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

## Checklist manuelle (macOS)

- [ ] Premier lancement : la fenêtre s'ouvre sur le panneau des autorisations ; après « Autoriser » et activation
  dans Réglages Système, le panneau disparaît et « Cmd droit » dicte sans relancer.
- [ ] Nouvelle build avec Scribe déjà coché : le panneau reste affiché ; « Autoriser » puis réactiver suffit.
- [ ] Micro : « Autoriser » affiche la demande de macOS ; refusé, « Ouvrir les Réglages Système » mène au bon panneau.
- [ ] TextEdit, Notes, Safari, Chrome, VS Code, Slack : texte inséré.
- [ ] Terminal, iTerm2 : texte collé.
- [ ] Finder sans champ : toast « Texte copié », Cmd+V colle le texte.
- [ ] La pastille s'affiche par-dessus une app en plein écran et ne prend jamais le focus (clic dessus compris).
- [ ] Double-tap : pastille « Verrouillé », Espace ne parvient pas à l'application.
- [ ] AZERTY : raccourci `Cmd + A` affiché « Cmd + A » et déclenché par la touche A ; Cmd+A seul sélectionne toujours tout.
- [ ] Pendant « Changer », Cmd/Option/lettres n'ont aucun effet ; Échap annule.
- [ ] Pas d'icône dans le Dock ; l'icône de la barre de menus ouvre Scribe.
- [ ] Clés API dans le trousseau : relancer Scribe ne les redemande pas.

## Publier une version

Les versions sont publiées automatiquement à partir des [Conventional Commits](https://www.conventionalcommits.org/)
(workflow `.github/workflows/release-please.yml`, configuration `release-please-config.json` et
`.release-please-manifest.json`).

1. Fusionner les PR en **squash** avec un titre conventionnel : il devient le message du commit sur `main`. Avant
   1.0, `feat:` monte la version mineure (0.3.0 → 0.4.0), `fix:` et `perf:` la version de correctif (0.3.0 →
   0.3.1), un changement cassant (`feat!:` ou `BREAKING CHANGE:`) la version mineure. `chore:`, `docs:`, `ci:`,
   `test:`, `refactor:` ne déclenchent pas de version.
2. À chaque push sur `main`, release-please tient à jour une PR « chore: release X.Y.Z » : version dans
   `package.json`, `src-tauri/Cargo.toml` et `Cargo.lock` (entrée `scribe-app` seulement), et `CHANGELOG.md`
   (Fonctionnalités, Corrections, Performances). La CI ne tourne pas sur cette PR (elle est ouverte par
   `GITHUB_TOKEN`) ; elle ne change que des numéros de version et le changelog.
3. Fusionner cette PR publie la version : le workflow crée le tag `vX.Y.Z` et une release en brouillon, construit
   les installeurs Windows puis le `.dmg` macOS universel dans ce brouillon (avec `latest.json` pour les deux
   plateformes), et ne publie la release que si les deux builds ont réussi. Si un build échoue, le brouillon reste
   privé : relancer les jobs en échec (« Re-run failed jobs ») le complète puis le publie.

La version de l'app est celle de `src-tauri/Cargo.toml` (`tauri.conf.json` n'en a pas : Tauri reprend celle du
crate pour les installeurs, `latest.json` et la version affichée). Ne pas la modifier à la main.

Réglage du dépôt nécessaire : Settings > Actions > General > « Allow GitHub Actions to create and approve pull
requests », sans quoi release-please ne peut pas ouvrir sa PR.

Repli manuel (`.github/workflows/release.yml`) : monter la version dans `src-tauri/Cargo.toml`, `package.json` et
`Cargo.lock`, puis pousser un tag `v<version>` (`git tag v0.4.0 && git push origin v0.4.0`). Le workflow vérifie que
le tag correspond à `src-tauri/Cargo.toml`, construit les installeurs et les joint à une release en brouillon, à
publier à la main. Mettre ensuite `.release-please-manifest.json` à cette version pour que release-please reparte de
là.

Build local :

```bash
# Les bundles de mise à jour sont signés : la clé privée et son mot de passe sont lus dans ~/.tauri.
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/scribe.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/scribe.key.password)"   bun tauri build   # target/release/bundle/nsis/Scribe_<version>_x64-setup.exe (+ .sig), et un .msi
```

Les installeurs ne sont pas signés : au premier lancement, Windows SmartScreen demande « Informations
complémentaires » puis « Exécuter quand même » ; macOS demande un clic droit sur Scribe > Ouvrir.

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

Sur macOS, l'app est signée ad hoc mais pas notarisée : au premier lancement, clic droit sur Scribe > Ouvrir. Scribe
demande ensuite l'accès Accessibilité (raccourci global, collage, lecture du champ actif) puis le micro. La signature
ad hoc change à chaque version : après une mise à jour, l'autorisation Accessibilité ne vaut plus même si Scribe
reste coché. La fenêtre de Scribe le signale ; « Autoriser » remet l'autorisation à zéro, il suffit de réactiver Scribe.

## Licence

Copyright (C) 2026 Guillaume Gautreau.

Scribe est distribué sous la licence [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`). Vous pouvez
l'utiliser, l'étudier, le modifier et le redistribuer ; toute version modifiée distribuée ou mise à disposition
d'utilisateurs, y compris à travers un service en ligne, doit l'être sous la même licence, avec son code source
complet.
