# Privacy and release integrity

## Releases

Windows installers (`Scribe_<version>_x64-setup.exe`, recommended, and `Scribe_<version>_x64_en-US.msi`) are built
from this repository's source code by the release workflow
[`.github/workflows/release.yml`](.github/workflows/release.yml) on GitHub-hosted runners, from a `vX.Y.Z` tag.
Third-party libraries are compiled from their published sources (Cargo and bun lock files are committed).

The installers are **not Authenticode-signed**. On first install, Windows SmartScreen shows "Windows protected your
PC": click "More info", then "Run anyway". Updates are not affected: Scribe downloads them itself, so they do not
carry the browser's download mark that triggers SmartScreen.

Every update package is signed with the project's Tauri updater key (minisign). Installed copies of Scribe refuse
an update whose signature does not match the public key embedded in
[`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json).

## Privacy

Scribe has no telemetry, no analytics and no account. It sends data over the network only in the cases below.

**Dictation, at the user's request.** While the user holds the trigger key (or after locking a dictation), Scribe
records the microphone. When the dictation ends, the recording is sent to the speech-to-text provider the user
chose in the settings, with terms from the user's glossary as spelling hints. Unless correction is turned off, the
transcript is then sent to the text-correction provider the user chose, with glossary terms and the name of the
application the text is meant for (for example `WINWORD`). "Retranscribe" in the history does the same for a past
recording; "Test configuration" sends half a second of silence and a fixed test sentence. Requests go directly
from the user's computer to the provider, authenticated with the user's own API key:

- OpenAI: [privacy policy](https://openai.com/policies/privacy-policy/)
- Anthropic: [privacy policy](https://www.anthropic.com/legal/privacy)
- Mistral AI: [privacy policy](https://mistral.ai/terms#privacy-policy)
- Groq: [privacy policy](https://groq.com/privacy-policy/)

**Update checks.** 30 seconds after launch, and when the user clicks "Check for updates", Scribe downloads
`latest.json` from this repository's GitHub releases (GitHub receives the usual request metadata, such as the IP
address). An update is downloaded and installed only when the user clicks "Install and restart".
[GitHub privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement).

**Stays on the computer.** Dictation history, recordings (deleted after the retention period set in the settings),
the glossary and the settings are stored in the user's application data folder. API keys are stored in the
operating system's credential store (Windows Credential Manager), never in a file.

### System access

- **Global keyboard hook.** Scribe installs a low-level keyboard hook to detect its trigger key combination. Key
  events are compared with that combination and never recorded, logged or transmitted. Keys outside the
  trigger combination always pass through unchanged.
- **Clipboard and simulated paste.** To insert the text, Scribe puts it in the clipboard, simulates Ctrl+V, then
  restores the previous clipboard content.
- **Focused field.** To check that the paste landed, Scribe reads the text of the focused field through UI
  Automation just before and just after pasting. This text is compared in memory and discarded; password fields
  are never read.
- **Launch at login.** Off by default. When the user enables it in the settings, Scribe adds itself to the user's
  startup entries, and removes itself when the user disables it.

The installers include an uninstaller (Windows Settings > Apps).

## Reporting a problem

Report a suspicious binary or a privacy issue in a [GitHub issue](https://github.com/ghusse/scribe/issues) (or
privately through [GitHub security advisories](https://github.com/ghusse/scribe/security/advisories/new)).
