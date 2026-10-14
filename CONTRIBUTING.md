# Contributing to Scribe

This guide is for developers. To install and use Scribe, see the [README](README.md).

Everything on GitHub is written in English: code, comments, docs, commit messages, PR titles and descriptions. The
app's interface itself is in French, so UI strings quoted below stay in French.

## Prerequisites

- [Rust](https://rustup.rs/) stable (`rustup update stable`)
- [bun](https://bun.sh/) (version in `package.json` > `packageManager`)
- Windows: the Visual Studio C++ build tools (WebView2 ships with Windows 10/11)
- macOS: Xcode Command Line Tools (`xcode-select --install`)

## Running in development

```bash
bun install      # also installs the pre-commit hook (.githooks)
bun tauri dev
```

On first launch (missing keys), the window opens on the Réglages (settings) tab. Started with `--minimized`, Scribe
goes straight to the notification area, unless a key is missing.

## Code layout

| Folder | Contents |
|---|---|
| `crates/scribe-core` | Pure logic: key combinations and gestures, dictation pipeline, correction prompt, insertion, storage |
| `crates/scribe-providers` | Speech-to-text and correction providers (model catalog: `catalog.rs`) |
| `crates/scribe-platform` | Windows / macOS adapters (keyboard hook, paste, microphone, clipboard, sound) |
| `src-tauri` | Tauri app: commands, dictation controller, overlay, updates |
| `src/main`, `src/overlay` | Preact UI: main window and recording overlay |
| `tests`, `scripts` | Tauri doubles for UI tests, coverage gate, build and screenshot scripts |

Detailed rules (coverage, exclusions, tests, PR titles) are in [`CLAUDE.md`](CLAUDE.md).

## Submitting a change

1. One branch per topic, one PR merged with **squash**.
2. PR title in [Conventional Commits](https://www.conventionalcommits.org/) format: it becomes the commit subject and
   feeds the changelog. A user-visible `feat` or `fix` describes what changes for the user, not how:
   `fix(macos): paste the dictation instead of typing "v"`.
3. Before pushing: `bun run fmt:check`, `cargo test --workspace`, `bun run test`, `bun run check`,
   `bun run build`, `bun run coverage`.

## README screenshots

```bash
bun run screenshots   # regenerates docs/screenshots/*.png (needs Chrome; otherwise CHROME=/path/to/chrome)
```

The script serves the real UI with a fake Tauri backend and fictional data (`scripts/screenshots/tauri-demo.ts`),
then captures it in headless Chrome. To add a screenshot, add an entry to `SHOTS` (`scripts/screenshots.ts`):
`?tab=` opens a tab, `?section=` starts the settings at that section. A new backend command used by the UI is
answered in `routes` of `tauri-demo.ts`.

## Tests and coverage

```bash
bun run fmt        # cargo fmt (rustfmt.toml); checked by CI and by the pre-commit hook installed by `bun install`
cargo test --workspace
bun run test
bun run check
bun run coverage   # gate: >= 95 % of lines in Rust, TypeScript and TSX (Preact)
```

Rule: at least 95 % line coverage in every language, enforced by CI (`.github/workflows/ci.yml`, Windows). Rust via
`cargo-llvm-cov` (`rustup component add llvm-tools-preview`, `cargo install cargo-llvm-cov`), UI via
`bun test --coverage` and `scripts/coverage-gate.ts` (one threshold per language; components tested with
`@testing-library/preact` + happy-dom, patched by `patches/happy-dom@20.14.5.patch` so that `:checked` finds the
selected option of a `<select>`). The only allowed exclusions (OS/framework adapters without logic) and their
justification are listed in [`CLAUDE.md`](CLAUDE.md).

## Manual checklist (Windows)

- [ ] Notepad: hold → text inserted, previous clipboard restored.
- [ ] VS Code, Slack, Chrome (search field, Gmail): text inserted (« Texte inséré ? » toast tolerated).
- [ ] Windows Terminal: text pasted, or a toast with « Copier ».
- [ ] Desktop / Explorer without a field: « Texte copié » toast, Ctrl+V pastes the text.
- [ ] Switch windows during transcription: nothing pasted, « Texte copié » toast.
- [ ] Copy an image, dictate, check the image is restored.
- [ ] Hold the key 3 s without speaking: nothing inserted.
- [ ] Double-tap: « Verrouillé » pill, Ctrl+Space does not reach the application.
- [ ] `Ctrl + Maj + A` shortcut: dictates with either Ctrl; no "a" typed; Ctrl+A alone still selects all.
- [ ] `Win + Alt` shortcut: dictates, and the Start menu does not open on release.
- [ ] During « Changer », Win/Alt/letters have no effect in Windows; Escape cancels.
- [ ] Win+L while holding: the dictation is dropped (nothing pasted after unlocking); the shortcut and Space then work normally.
- [ ] AZERTY: « Alt droit (AltGr) » alone triggers; with `Ctrl + Alt` as the shortcut, typing @ or € triggers nothing.
- [ ] Several layouts installed: `Ctrl + Maj + A` does not switch the keyboard layout.
- [ ] The pill never takes focus away from the field.
- [ ] Invalid Anthropic key: raw text inserted, « Inséré sans correction » toast.
- [ ] Wi-Fi off: error toast, « Erreur » dictation in the history, « Retranscrire » works after reconnecting.
- [ ] Add "Kubernetes" to the vocabulary, dictate "cube ernetes": spelling fixed, usage counter incremented.
- [ ] Tray: Pause disables the shortcut; Quit closes the app; closing the window only hides it.

## Manual checklist (macOS)

- [ ] First launch: the window opens on the permissions panel; after « Autoriser » and enabling it in System
  Settings, the panel disappears and « Cmd droit » dictates without relaunching.
- [ ] New build with Scribe already ticked: the panel stays; « Autoriser » then re-enabling is enough.
- [ ] Microphone: « Autoriser » shows the macOS prompt; when denied, « Ouvrir les Réglages Système » opens the right pane.
- [ ] TextEdit, Notes, Safari, Chrome, VS Code, Slack: text inserted.
- [ ] Terminal, iTerm2: text pasted.
- [ ] Finder without a field: « Texte copié » toast, Cmd+V pastes the text.
- [ ] The pill shows over a full-screen app and never takes focus (clicking it included).
- [ ] Double-tap: « Verrouillé » pill, Space does not reach the application.
- [ ] AZERTY: `Cmd + A` shortcut shown as "Cmd + A" and triggered by the A key; Cmd+A alone still selects all.
- [ ] During « Changer », Cmd/Option/letters have no effect; Escape cancels.
- [ ] No Dock icon; the menu bar icon opens Scribe.
- [ ] API keys in the Keychain: relaunching Scribe does not ask for them again.

## Releasing

Versions are published automatically, with no release PR or manual step, from
[Conventional Commits](https://www.conventionalcommits.org/) (workflow `.github/workflows/semantic-release.yml`,
configuration `release.config.mjs`, [semantic-release](https://semantic-release.gitbook.io/)).

1. Merge PRs with **squash** and a conventional title: it becomes the commit message on `main`. Before 1.0, `feat:`
   bumps the minor version (0.3.0 → 0.4.0), `fix:` and `perf:` the patch version (0.3.0 → 0.3.1), a breaking change
   (`feat!:` or `BREAKING CHANGE:`) the minor version. `chore:`, `docs:`, `ci:`, `test:`, `refactor:`, `style:`,
   `build:` do not trigger a release (a `revert:` gives a patch).
2. When CI for that push to `main` is green, the workflow analyses the commits since the last `vX.Y.Z` tag. If it
   finds a `feat`, `fix` or `perf`, it writes the version to `package.json`, `src-tauri/Cargo.toml` and `Cargo.lock`
   (`scribe-app` entry only), prepends the notes to `CHANGELOG.md`, pushes the "chore(release): X.Y.Z" commit to
   `main`, creates the `vX.Y.Z` tag and a draft release. That commit is pushed with `GITHUB_TOKEN`: it triggers
   neither CI nor the workflow again.
3. The workflow then builds the Windows installers and the universal macOS `.dmg` from that tag, into the draft
   (with `latest.json` for both platforms), and publishes the release only if both builds succeeded.

Each `feat`/`fix`/`perf` merge therefore gets its own version. To group several into one version, gather them on
one branch and merge it as a single squash, or merge commits that publish nothing first (`chore:`, `refactor:`…):
they ship with the next version. If several merges land before CI finishes, only the last one's CI completes
(`cancel-in-progress`) and its version contains everything since the last tag; a run started for a commit that is
no longer the head of `main` publishes nothing.

Recovery: if a build or the publication fails, the draft stays private; re-running the failed jobs ("Re-run failed
jobs") completes and publishes it. If the `release` job failed after pushing the tag (before or after creating the
draft), or for any other recovery: Actions > Semantic Release > "Run workflow" with the tag (`v0.4.0`) rebuilds both
platforms into that draft and publishes it; if the tag has no release yet, that run first creates the draft, with
that version's notes taken from `CHANGELOG.md` at the tag (run it once the automatic run is over, or it could
duplicate the draft). A recovery run has its own concurrency group: a push to `main` does not cancel it while it
waits, and it does not cancel a pending publication. Re-running the `release` job itself creates nothing (the
version commit has already moved `main`); if CI on `main` failed or was cancelled, re-running it (or pushing a new
commit) is enough: nothing is lost, the next version always starts from the last tag.

The app version is the one in `src-tauri/Cargo.toml` (`tauri.conf.json` has none: Tauri takes the crate's for the
installers, `latest.json` and the displayed version). Do not edit it by hand. There is no manifest: the `vX.Y.Z`
tags in `main`'s history give the starting version.

Repository settings: "Allow GitHub Actions to create and approve pull requests" is no longer needed. `main` is not
protected; if it becomes so (branch protection or ruleset), the GitHub Actions bot must still be allowed to push to
it (the "chore(release)" commit) and to create `v*` tags, or the `release` job fails before creating the tag.

Manual fallback (`.github/workflows/release.yml`): in a single commit, bump the version in `src-tauri/Cargo.toml`,
`package.json` and `Cargo.lock`, then push that commit and the tag together
(`git tag v0.4.0 && git push --atomic origin main v0.4.0`). The workflow checks that the tag matches
`src-tauri/Cargo.toml`, builds the installers and attaches them to a draft release, to be published by hand.
semantic-release starts again from that tag: the next automatic version only counts later commits. Pushing the
version commit also re-runs CI then semantic-release, which publishes nothing if that commit is a `chore:`.

Local build:

```bash
# Update bundles are signed: the private key and its password are read from ~/.tauri.
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/scribe.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/scribe.key.password)"   bun tauri build   # target/release/bundle/nsis/Scribe_<version>_x64-setup.exe (+ .sig), and an .msi
```

The installers are not signed (neither Authenticode nor Apple notarization): see the Installation section of the
[README](README.md).

## Updates

The published release contains `latest.json`. Installed copies fetch it 30 s after launch (a toast announces a new
version) and from Réglages > Mises à jour, which installs it and restarts Scribe. Every update is verified against
the public key in `tauri.conf.json`; the private key and its password are the repository secrets
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Losing them prevents any update of installed
copies: keep a backup.
