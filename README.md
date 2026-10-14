# Scribe

**Dictée vocale pour Windows et macOS : maintenez une touche, parlez, relâchez. Le texte, propre et ponctué,
s'insère là où vous écriviez.**

Mail, Slack, Word, VS Code, navigateur, terminal : Scribe marche dans n'importe quelle application, sans extension
ni copier-coller. Une IA transcrit votre voix, puis une autre retire les « euh », corrige la ponctuation et
l'orthographe des noms propres que vous lui avez appris.

![Scribe en cours de dictée](docs/screenshots/dictation.png)

[**⬇ Télécharger la dernière version**](https://github.com/ghusse/scribe/releases/latest)

## Pourquoi Scribe

- **Partout.** Le texte arrive dans le champ actif de n'importe quelle application. Sans champ, il est copié dans le
  presse-papier et un toast vous le signale.
- **Rapide à déclencher.** Maintenez Ctrl droit (Cmd droit sur Mac) pour parler, ou tapez deux fois pour dicter
  mains libres. Le raccourci se change en une combinaison de votre choix.
- **Un texte qu'on n'a pas à reprendre.** Trois niveaux de correction : texte brut, nettoyage (ponctuation,
  hésitations, vocabulaire), ou nettoyage + mise en forme adaptée à l'application (un mail ne s'écrit pas comme un
  message Slack).
- **Votre vocabulaire.** L'onglet Vocabulaire apprend à Scribe les noms propres et le jargon de vos domaines
  (« Kubernetes », pas « cube ernetes »).
- **Rien ne se perd.** L'historique garde chaque dictée ; une erreur réseau ? « Retranscrire » relance depuis
  l'enregistrement.
- **Vos clés, vos fournisseurs.** OpenAI, Anthropic, Mistral ou Groq : vous choisissez le modèle et payez
  directement le fournisseur, à l'usage. Pas de compte Scribe, pas d'abonnement, pas de télémétrie.
- **Discret.** Une pastille indique l'enregistrement sans jamais voler le focus. Option : couper le son de
  l'ordinateur et le micro des autres applications pendant la dictée.
- **Libre** (AGPL-3.0) et mis à jour automatiquement.

| Historique | Vocabulaire | Réglages |
|---|---|---|
| ![Historique des dictées](docs/screenshots/history.png) | ![Vocabulaire](docs/screenshots/glossary.png) | ![Réglages](docs/screenshots/settings.png) |

## Installation

Téléchargez l'installeur de votre système depuis la
[dernière release](https://github.com/ghusse/scribe/releases/latest) :

| Système | Fichier |
|---|---|
| Windows 10 / 11 (x64) | `Scribe_<version>_x64-setup.exe` (recommandé) ou `Scribe_<version>_x64_en-US.msi` |
| macOS 11 ou plus récent (Apple Silicon et Intel) | `Scribe_<version>_universal.dmg` |

> **Pourquoi un avertissement à l'installation ?** Scribe est un projet libre et gratuit : ses installeurs ne sont
> pas signés par un certificat payant (Microsoft Authenticode, notarisation Apple). Windows et macOS préviennent
> donc au premier lancement. Les installeurs sont construits par GitHub Actions directement depuis le code de ce
> dépôt (détails dans [PRIVACY.md](PRIVACY.md)). Les mises à jour, elles, sont signées et vérifiées par Scribe.

### Windows

1. Lancez `Scribe_<version>_x64-setup.exe`.
2. SmartScreen affiche « Windows a protégé votre ordinateur » : cliquez sur **Informations complémentaires**, puis
   **Exécuter quand même**.
3. Terminez l'installation. Scribe s'ouvre et s'installe dans la zone de notification (près de l'horloge).

### macOS

1. Ouvrez le `.dmg` et glissez **Scribe** dans **Applications**.
2. Au premier lancement, macOS refuse d'ouvrir l'app (« développeur non identifié » ou « impossible de vérifier »).
   Ouvrez **Réglages Système > Confidentialité et sécurité**, descendez jusqu'au message sur Scribe et cliquez sur
   **Ouvrir quand même**, puis confirmez. Sur les versions plus anciennes de macOS, un clic droit sur Scribe >
   **Ouvrir** suffit.

   Alternative en ligne de commande :

   ```bash
   xattr -dr com.apple.quarantine /Applications/Scribe.app
   ```

3. Scribe demande deux autorisations, à accorder depuis sa fenêtre :
   - **Accessibilité** : détecter le raccourci, coller le texte et vérifier qu'il est bien arrivé ;
   - **Micro** : enregistrer la dictée.

Scribe vit dans la barre de menus, sans icône dans le Dock.

> Après une mise à jour sur macOS, l'autorisation Accessibilité peut ne plus être reconnue même si Scribe reste
> coché. La fenêtre de Scribe le signale : cliquez sur **Autoriser**, puis réactivez Scribe dans Réglages Système.

## Configuration : les clés API

Scribe utilise deux services d'IA, avec vos propres clés :

- **Transcription** (la voix devient du texte) : OpenAI, Mistral ou Groq ;
- **Correction** (le texte devient propre) : OpenAI, Anthropic, Mistral ou Groq. Facultative : le niveau
  « Aucune (texte brut) » s'en passe.

Une seule clé suffit pour commencer : OpenAI, Mistral et Groq font les deux.

### 1. Créer une clé

| Fournisseur | Transcription | Correction | Où créer la clé |
|---|:---:|:---:|---|
| OpenAI | ✓ | ✓ | [platform.openai.com/api-keys](https://platform.openai.com/api-keys) |
| Anthropic | | ✓ | [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys) |
| Mistral | ✓ | ✓ | [console.mistral.ai/api-keys](https://console.mistral.ai/api-keys) |
| Groq | ✓ | ✓ | [console.groq.com/keys](https://console.groq.com/keys) |

Ce sont des API payantes à l'usage (Groq et Mistral proposent un palier gratuit limité) : ajoutez un moyen de
paiement ou du crédit sur le compte du fournisseur, sans quoi la clé est refusée. Une dictée de quelques phrases
coûte une fraction de centime.

### 2. La renseigner dans Scribe

Au premier lancement, Scribe s'ouvre sur l'onglet **Réglages** :

1. **Fournisseurs et modèles** : choisissez le fournisseur et le modèle de transcription, puis ceux de correction.
   Par défaut : OpenAI `gpt-transcribe` pour la transcription, Anthropic `claude-opus-5-5` pour la correction.
2. **Clés API** : collez la clé de chaque fournisseur choisi.
3. Cliquez sur **Tester transcription et correction** : Scribe vérifie les deux étapes et affiche le détail en cas d'erreur.

Les clés sont rangées dans le coffre de votre système (Gestionnaire d'identification Windows, trousseau macOS),
jamais dans un fichier.

![Configuration des fournisseurs et des clés](docs/screenshots/setup.png)

## Utilisation

| Geste | Effet |
|---|---|
| **Maintenir** Ctrl droit (Cmd droit sur Mac) | Enregistre tant que la touche est tenue ; relâcher transcrit et insère. |
| **Double-tap** sur la touche | Mode verrouillé, mains libres ; un nouvel appui arrête. |
| **Espace** pendant le maintien | Verrouille la dictée en cours. |

- **Changer le raccourci** : Réglages > Raccourci > Changer, puis appuyez sur la combinaison voulue (1 à 4 touches,
  par exemple `Ctrl + Maj + A` ou `Win + Alt`) et relâchez.
- **Vocabulaire** : ajoutez noms propres, marques et termes techniques, avec leurs variantes mal entendues. Scribe
  les transmet à la transcription et à la correction.
- **Historique** : retrouvez, copiez ou retranscrivez une dictée. Les enregistrements audio sont supprimés après la
  durée choisie dans les réglages.
- **Démarrage** : Réglages > Démarrage > « Lancer Scribe à l'ouverture de session » le lance en arrière-plan à
  chaque connexion.
- **Pause** : depuis l'icône de la zone de notification (barre de menus sur Mac), pour désactiver le raccourci.

Fermer la fenêtre ne quitte pas Scribe : il reste dans la zone de notification. Pour quitter, passez par son icône.

## Mises à jour

Scribe vérifie s'il existe une nouvelle version 30 secondes après son lancement et vous prévient par un toast.
Réglages > Mises à jour l'installe et redémarre Scribe. Chaque mise à jour est signée et vérifiée avant
installation.

## Confidentialité

Pas de télémétrie, pas d'analytique, pas de compte. L'audio et le texte de vos dictées partent directement de
votre ordinateur vers les fournisseurs que vous avez choisis, avec votre clé ; l'historique, le glossaire et les
réglages restent sur votre machine. Tout ce que Scribe envoie et les accès système qu'il utilise (clavier,
presse-papier, champ actif) sont détaillés dans [PRIVACY.md](PRIVACY.md).

## Contribuer

Bugs et idées : [ouvrez une issue](https://github.com/ghusse/scribe/issues). Pour compiler Scribe, lancer les tests
ou proposer une modification, voir [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Copyright (C) 2026 Guillaume Gautreau.

Scribe est distribué sous la licence [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`). Vous pouvez
l'utiliser, l'étudier, le modifier et le redistribuer ; toute version modifiée distribuée ou mise à disposition
d'utilisateurs, y compris à travers un service en ligne, doit l'être sous la même licence, avec son code source
complet.
