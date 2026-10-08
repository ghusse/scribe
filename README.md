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

Les versions sont publiées automatiquement, sans PR de version ni étape manuelle, à partir des
[Conventional Commits](https://www.conventionalcommits.org/) (workflow `.github/workflows/semantic-release.yml`,
configuration `release.config.mjs`, [semantic-release](https://semantic-release.gitbook.io/)).

1. Fusionner les PR en **squash** avec un titre conventionnel : il devient le message du commit sur `main`. Avant
   1.0, `feat:` monte la version mineure (0.3.0 → 0.4.0), `fix:` et `perf:` la version de correctif (0.3.0 →
   0.3.1), un changement cassant (`feat!:` ou `BREAKING CHANGE:`) la version mineure. `chore:`, `docs:`, `ci:`,
   `test:`, `refactor:`, `style:`, `build:` ne déclenchent pas de version (un `revert:` donne un correctif).
2. Quand la CI de ce push sur `main` est verte, le workflow analyse les commits depuis le dernier tag `vX.Y.Z`. S'il
   y trouve un `feat`, `fix` ou `perf`, il écrit la version dans `package.json`, `src-tauri/Cargo.toml` et
   `Cargo.lock` (entrée `scribe-app` seulement), ajoute les notes en tête de `CHANGELOG.md` (Fonctionnalités,
   Corrections, Performances), pousse le commit « chore(release): X.Y.Z » sur `main`, crée le tag `vX.Y.Z` et une
   release en brouillon. Ce commit est poussé avec `GITHUB_TOKEN` : il ne relance ni la CI ni le workflow.
3. Le workflow construit ensuite les installeurs Windows puis le `.dmg` macOS universel depuis ce tag, dans le
   brouillon (avec `latest.json` pour les deux plateformes), et ne publie la release que si les deux builds ont
   réussi.

Chaque merge `feat`/`fix`/`perf` donne donc sa propre version. Pour en grouper plusieurs dans une seule version,
les réunir sur une branche et la fusionner en un seul squash, ou fusionner d'abord des commits qui ne publient
rien (`chore:`, `refactor:`…) : ils partent avec la version suivante. Si plusieurs merges arrivent avant la fin de
la CI, seule la CI du dernier va au bout (`cancel-in-progress`) et sa version contient tout depuis le dernier tag ;
un run lancé pour un commit qui n'est plus la tête de `main` ne publie rien.

Reprise : si un build ou la publication échoue, le brouillon reste privé ; relancer les jobs en échec (« Re-run
failed jobs ») le complète puis le publie. Si le job `release` a échoué après avoir poussé le tag (avant ou après
la création du brouillon), ou pour toute autre reprise : Actions > Semantic Release > « Run workflow » avec le tag
(`v0.4.0`) reconstruit les deux plateformes dans ce brouillon et le publie ; si le tag n'a encore aucune release, ce
run crée d'abord le brouillon, avec les notes de cette version prises dans `CHANGELOG.md` au tag (le lancer une fois
le run automatique terminé, sans quoi il pourrait doubler le brouillon). Une reprise a son propre groupe de
concurrence : un push sur `main` ne l'annule pas pendant qu'elle attend, et elle n'annule pas une publication en
attente. Relancer le job `release` lui-même ne crée rien (le commit de version a déjà fait avancer `main`) ; si la CI
de `main` a échoué ou a été annulée, la relancer (ou pousser un nouveau commit) suffit : rien n'est perdu, la
version suivante part toujours du dernier tag.

La version de l'app est celle de `src-tauri/Cargo.toml` (`tauri.conf.json` n'en a pas : Tauri reprend celle du
crate pour les installeurs, `latest.json` et la version affichée). Ne pas la modifier à la main. Il n'y a plus de
manifeste : ce sont les tags `vX.Y.Z` présents dans l'historique de `main` qui donnent la version de départ.

Réglages du dépôt : « Allow GitHub Actions to create and approve pull requests » n'est plus nécessaire. `main` n'est
pas protégée ; si elle le devient (protection de branche ou ruleset), le bot GitHub Actions doit rester autorisé à y
pousser (le commit « chore(release) ») et à créer des tags `v*`, sans quoi le job `release` échoue avant de créer le
tag.

Repli manuel (`.github/workflows/release.yml`) : dans un même commit, monter la version dans
`src-tauri/Cargo.toml`, `package.json` et `Cargo.lock`, puis pousser ce commit et le tag ensemble
(`git tag v0.4.0 && git push --atomic origin main v0.4.0`). Le workflow vérifie que le tag correspond à
`src-tauri/Cargo.toml`, construit les installeurs et les joint à une release en brouillon, à publier à la main.
semantic-release repart de ce tag : la version automatique suivante ne compte que les commits postérieurs. Le push
du commit de version relance aussi la CI puis semantic-release, qui ne publie rien si ce commit est un `chore:`.

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
