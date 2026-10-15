# Scribe: rules for agents

## Coverage rule (mandatory)

At least **95 % line coverage on every language**: Rust, TypeScript and Preact components (TSX). Each language has
its own gate: the Rust workspace total (cargo-llvm-cov), and for the UI one threshold per glob (`src/**/*.ts`,
`src/**/*.tsx`) on top of the UI total (`scripts/coverage-gate.ts`), so a well-covered language cannot hide a poorly
covered one. CI fails below any of them.

```bash
bun run coverage:ui     # bun test --coverage (lcov, bunfig.toml), then scripts/coverage-gate.ts (lines: 95, per glob)
bun run coverage:rust   # cargo llvm-cov --workspace --fail-under-lines 95 --ignore-filename-regex ...
bun run coverage        # both, locally (CI runs them as two independent steps)
```

Reports: UI in the terminal and `coverage/ui/lcov.info`; Rust in the terminal (`cargo llvm-cov --workspace --html`
for html). bun only reports the files a test loaded: the gate fails on any measured source missing from the report.
Prerequisites: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`.

UI tests run with `bun test` (`bunfig.toml` preloads, in order: `tests/dom.ts` happy-dom globals, `tests/css-modules.ts`
real CSS modules compiled by bun's bundler, `tests/setup.ts`). Every test file shares one process, so module mocks
are global: `tests/setup.ts` installs the Tauri doubles of `tests/tauri.ts` for `@tauri-apps/api/core` (`invoke`)
and `@tauri-apps/api/event` (`listen`) once, resets them (implementations included) before each test and unmounts
every render after it; never `mock.module` / `vi.mock` them in a test file. `commands({...})` answers each command,
`emit` fires an event, `calls` lists a command's arguments; settings and providers come from `tests/fixtures.ts`.
Preact components are covered through component tests (`@testing-library/preact`, see
`src/main/ModelPicker.test.tsx`, which also shows how to observe a controlled `value` / `onValueChange` prop pair).
Wait with `@testing-library/preact`'s `waitFor` / `findBy*` (bun has no `vi.waitFor`); under fake timers it advances
them 50 ms per retry. Bun has no `vi.advanceTimersByTimeAsync`: use `advanceTimers` from `tests/timers.ts`, and
restore real timers in an `afterEach` so a failing test cannot leak them.
Shared component hooks (`useTick`, `useSyncEffect`) and `cx` live in `src/lib/ui.ts`. Decision logic
stays out of components (`src/overlay/model.ts`, `src/lib/history.ts`, `src/lib/glossary.ts`, ...) and is tested
there as plain functions.

## Test rules

- Prefer small seams (traits, injected functions, pure helpers) over mocking frameworks.
- Tests assert behaviour (outputs, persisted rows, emitted events), not just executed lines.
- Never weaken an existing test. When a test reveals a real bug, fix the code and keep the test.

## Exclusion policy

Excluding a file is allowed **only** for thin OS/framework adapters with no decision logic (raw Windows/macOS FFI,
device/clipboard glue, Tauri bootstrap, UI entry files). Any logic in such a file must first be extracted into a
tested module. Every exclusion is listed below and must match the tooling **exactly**.
`tests/coverage-policy.test.ts` (run by `bun run test`) fails when the lines below and the tooling disagree: the
threshold, the Rust regex and the set of Rust files it actually excludes, the UI gate's excludes and per-glob
thresholds, and the two CI coverage steps.

- Threshold (lines, every language): `95`
- Rust ignore regex (`package.json` > `coverage:rust`):
  `crates.scribe-platform.src.windows.|crates.scribe-platform.src.macos.|crates.scribe-platform.src.device.|src-tauri.src.main[.]rs|src-tauri.src.tray[.]rs|src-tauri.src.menu[.]rs|src-tauri.src.adapters[.]rs`
  (`.` instead of a path separator so the regex works with both `\` and `/`).
- Rust files excluded by that regex: `crates/scribe-platform/src/device/clipboard.rs`,
  `crates/scribe-platform/src/macos/audio_input.rs`, `crates/scribe-platform/src/macos/audio_output.rs`, `crates/scribe-platform/src/macos/focus.rs`, `crates/scribe-platform/src/macos/hook.rs`,
  `crates/scribe-platform/src/macos/keys.rs`, `crates/scribe-platform/src/macos/mod.rs`,
  `crates/scribe-platform/src/macos/permissions.rs`, `crates/scribe-platform/src/macos/window.rs`,
  `crates/scribe-platform/src/device/microphone.rs`, `crates/scribe-platform/src/device/mod.rs`,
  `crates/scribe-platform/src/windows/audio_output.rs`, `crates/scribe-platform/src/windows/exclusive_capture.rs`,
  `crates/scribe-platform/src/windows/focus.rs`,
  `crates/scribe-platform/src/windows/hook.rs`, `crates/scribe-platform/src/windows/keys.rs`,
  `crates/scribe-platform/src/windows/mod.rs`, `crates/scribe-platform/src/windows/window.rs`, `src-tauri/src/main.rs`,
  `src-tauri/src/tray.rs`, `src-tauri/src/menu.rs`, `src-tauri/src/adapters.rs`
- UI excludes (`scripts/coverage-gate.ts` > `EXCLUDES`): `src/**/*.d.ts`, `src/**/*.test.{ts,tsx}`,
  `src/main/main.tsx`, `src/overlay/main.tsx`, `preview/**`
- UI per-language thresholds (`scripts/coverage-gate.ts` > `GLOBS`): `src/**/*.ts`, `src/**/*.tsx`

| Excluded | Why |
|---|---|
| `crates/scribe-platform/src/windows/` (`audio_output.rs`, `exclusive_capture.rs`, `focus.rs`, `hook.rs`, `keys.rs`, `window.rs`, `mod.rs`) | Raw Win32/UIA/WASAPI FFI (`SetWindowsHookExW`, `SendInput`, `SetWindowPos`, UI Automation, `IAudioEndpointVolume`, exclusive-mode `IAudioClient` capture); needs a live desktop session. `mod.rs` re-exports the Windows entry points (`lib.rs` re-exports them, or `fallback.rs` off Windows). |
| `crates/scribe-platform/src/macos/` (`audio_input.rs`, `audio_output.rs`, `focus.rs`, `hook.rs`, `keys.rs`, `permissions.rs`, `window.rs`, `mod.rs`) | Raw macOS FFI (`CGEventTap`, `CGEventPost`, AX API, AVFoundation authorization, `NSPanel`, CoreAudio `AudioObject*` properties, hog mode); needs a live desktop session and the Accessibility permission. Compiled on macOS only, so the Windows CI never sees it. |
| `crates/scribe-platform/src/device/` (`microphone.rs`, `clipboard.rs`, `mod.rs`) | Device glue: cpal (default input device, stream per sample format) and arboard (one call per `ClipboardBackend` method). Needs a microphone / the real system clipboard. |
| `src-tauri/src/main.rs` | Tauri bootstrap: builds `Services`, registers commands, starts threads. |
| `src-tauri/src/tray.rs` | Tauri tray icon and menu construction and relabelling; each menu item calls one tested function, labels come from `locale::tray_labels`. |
| `src-tauri/src/menu.rs` | macOS menu bar (app, Edit, Window menus) built from predefined items; labels are `locale::app_menu_labels`. Compiled on macOS only. |
| `src-tauri/src/adapters.rs` | Implementations of the app seams on Tauri/Win32/AppKit/cpal: `TauriUi` (`UiSink`: `emit_to`; a language change is emitted as `language` to every window and relabels the tray through `tray::TrayMenu`), `TauriOverlayWindow` (`OverlayWindow`: `emit_to` + show/hide), `CpalRecorder` (`Recorder`), `TauriAutostart` (`LaunchAtLogin`: tauri-plugin-autostart), `TauriUpdater` (`AppUpdater`: tauri-plugin-updater; startup check and messages are `update.rs`), overlay placement call, `show_main`. Needs a running Tauri app, a desktop and a microphone. |
| `src/main/main.tsx`, `src/overlay/main.tsx` | UI entry files: a single `render(...)` call and the global stylesheet import. |
| `src/**/*.d.ts` | Type declarations (CSS module typing): no runtime code. |
| `src/**/*.test.{ts,tsx}`, `preview/**` | Tests themselves; local design previews (not shipped). |

What is left in excluded files is wiring only; every decision lives in a tested module:
- `windows/hook.rs::hook_proc` decodes `KBDLLHOOKSTRUCT` and calls `scribe_platform::key_filter::KeyFilter`
  (trigger combination held/released, strict matching, swallowing, capture, pause, `lock_vk == 0`, injected
  events, AltGr's fake left Ctrl, lost key-up / trigger changed while held, menu-mask decision; combination
  rules in `scribe_core::chord`).
  Remaining branches: `code == HC_ACTION`, hook not started (`SHARED` empty), inject the mask / forward /
  swallow per the filter's decision.
- `macos/hook.rs::tap_proc` turns the `CGEvent` into (virtual-key code, down) with `mac_keys::KeyMap` (Command
  = Win group, Option = Alt group, letters per layout, key-up given its key-down's code, Fn/Caps Lock ignored)
  and calls `KeyFilter` like Windows (the menu mask is not applied). Remaining branches: the event-type `match`
  (tap re-enabled when macOS disables it), our own events (`INJECTED_MARK`) passed through, swallow / forward,
  and the wait for the Accessibility permission before the tap is created (stopped early when the handle drops).
- `macos/permissions.rs`: `SystemPermissions` on AX/AVFoundation; what the UI offers per state is
  `src/lib/permissions.ts::permissionActions`, the « open the window at launch » rule `permissions::any_missing`.
  Remaining branches: the AVFoundation status `match` and the `open` command result.
- `macos/focus.rs`: classification is `focus_rules::classify_ax`, the app name `focus_rules::process_stem`, the
  text before the caret `focus_rules::prefix_utf16`, the selection length `insert::caret_units`; whether a replace
  may run and whether its selection matched are `insert::plan_replace` / `insert::replace`. Remaining branches: AX
  error / downcast `?`s, the secure-field guard of `AxFieldReader`, the `AXSelectedTextRange` read (`AXValue`
  decode), the empty-selection, caret-range and settable guards of `select_before_caret` (`insert::Selection::of`
  maps its result), and `AXManualAccessibility` set on the focused application by the caret reads only
  (`before_caret`, `select_before_caret`; Chromium/Electron build no AX tree otherwise), its pid remembered only once
  the set succeeded.
- `macos/keys.rs`, `macos/window.rs`, `macos/mod.rs`: CoreGraphics/AppKit/Carbon calls and re-exports only (which key
  pastes on the active layout is `mac_keys::paste_keycode`, which flag tells a modifier is down `mac_keys::modifier_mask`,
  the arrow keys of `press` `mac_keys::arrow_keycode` and their flags `mac_keys::arrow_flags`;
  the layout is read on the main queue with `exec_sync`, from the paste worker thread only). In `window.rs`, the
  overlay's web view gets no mouse event while another app is active: an always-active tracking area feeds
  `overlay_pointer::PointerTracker` (over the visible surface sent by the overlay, which button is under the pointer,
  which cursor to set), which decides the `overlay-pointer` event (toast kept under the pointer, hovered button styled,
  payload `overlay::OverlayPointer`) and the cursor; `hide_overlay` makes it forget the pointer (no `mouseExited:` once
  ordered out). `keys.rs` remaining branches: `ok_or` on event creation. The overlay is an `NSPanel` subclass whose `canBecomeKeyWindow` is NO (a click must leave the
  keyboard to the target app), registered by `define_class!` and set by `object_setClass` in `prepare_overlay`.
  Remaining branches: the `contentView` / main-thread `let else`s, the cursor `match` and the re-assert
  while over a button. Overlay
  `show`/`hide` post to the main queue (`DispatchQueue::main().exec_async`), never wait (same rule as Windows).
- `windows/focus.rs`: the class-name and process-name buffers go through `focus_rules::utf16_prefix` and
  `focus_rules::process_stem`; classification is `focus_rules::classify` (terminals → `Terminal`). `UiaFieldReader`
  only reads the focused field (text pattern, else value; never a password field); whether a paste landed is
  `scribe_core::insert::verify`, the re-reading loop `insert::perform`. The text before the caret, the selection and
  the selection before the caret (`insert::caret_units` characters) use the text pattern only; whether a replace may
  run and whether its selection matched are `insert::plan_replace` / `insert::replace`. Remaining branches: FFI error
  propagation (`?`, `.ok()`), the password / text-pattern / value-pattern `if`s (`insert::Selection::of` maps the
  selection result), the null foreground window →
  `FocusSnapshot::unknown()` guard, the degenerate-selection guard, the text pattern absent, the
  `MoveEndpointByUnit` count and the `Select()` result.
- `windows/audio_output.rs`, `macos/audio_output.rs`: one WASAPI / CoreAudio call per `OutputBackend` method;
  which outputs to mute (not already muted, failures skipped) and which to unmute (still muted, vanished ones
  ignored) is `output_mute::OutputMuter`. Remaining branches: FFI error propagation (`?`, status checks), the
  output-streams filter and missing mute property (macOS), the UID lookup loop (macOS), and `boot_time_ms` (wall
  clock minus `GetTickCount64` / `kern.boottime`; whether to recover in that boot session is `audio_mute::same_boot`).
- `device/microphone.rs`: the recording-thread protocol is `audio_capture::spawn_recorder`, sample conversions
  `audio_capture::{i16_to_f32, u16_to_f32}`, level metering, partial audio and the final clip
  `audio_capture::CaptureBuffer` (`push`, `set_error`, `finish`), exclusive-else-shared input
  `exclusive_mic::open_input`. Remaining branches: the cpal error paths (no device, unusable config, unsupported
  format, open/play failure → `ready` error) and the sample-format `match`.
- `macos/audio_input.rs`: one CoreAudio hog-mode call per `exclusive_mic::HogBackend` method; which inputs to take
  (every physical one, unused ones first, refused when one also plays sound), give back (only if ours) and
  the system default input put back afterwards is `exclusive_mic::HogGuard`;
  taking it once the shared cpal stream runs, keeping that stream when refused and waiting for a device restarting
  after a release is `exclusive_mic::open_input` (`ExclusiveAccess::Hold`). Remaining branches: status checks and
  the « no default input » guard.
- `windows/exclusive_capture.rs`: exclusive event-driven WASAPI capture (`exclusive_mic::InputOpener`); falling back
  to the shared cpal input when refused is `exclusive_mic::open_input`. Format choice (next format when one is
  refused at initialisation, stop on a device refusal) and the buffer-alignment retry are `wasapi_rules::negotiate`
  (one `ExclusiveClient` call per method), refusal messages `wasapi_rules::describe_failure`, `WAVEFORMATEX`
  fields ↔ format `wasapi_rules::PcmFormat::{from_wave, to_wave}`, packet decoding `wasapi_rules::packet_samples`,
  the capture thread protocol `wasapi_rules::spawn_capture`. Remaining branches: FFI error propagation (`?`), the
  COM init/uninit and MMCSS register/revert pairs, the `IsFormatSupported` `S_OK` check, the device-format blob
  guards (`VT_BLOB`, null/short), the extensible-part guard of `read_wave`, the mix-format `if let`, the null packet
  pointer and the capture loop conditions.
- `device/clipboard.rs`: what to read first (text, then image, else `Unsupported`) and how to restore each
  `ClipboardContent` are `clipboard::BackendClipboard`. Remaining branches: arboard error mapping (`?`).
- `windows/keys.rs`, `windows/window.rs`, `windows/mod.rs`: Win32 calls and re-exports only; the arrow burst of
  `press` (keys, Shift around it, extended flag) is `win_keys::arrow_strokes`. Remaining branch: the `SendInput`
  count check. `show_overlay`/`hide_overlay` must stay
  non-blocking across threads (`ShowWindowAsync`, `SWP_ASYNCWINDOWPOS`): see the next point.
- `tray.rs`: pause = `controller::toggle_pause` (tested); open = `adapters::show_main`; labels per language =
  `locale::tray_labels` (relabelled by `TrayMenu::relabel` when `save_settings` reports a new language). Remaining
  branches: the menu-id `match` and the left-click filter.
- `menu.rs`: labels per language = `locale::app_menu_labels`; the whole menu is rebuilt by `TauriUi::language_changed`.
  No branch: one predefined item per entry.
- `adapters.rs`: overlay state/dismiss rules are `overlay::Overlay`, placement is `overlay::overlay_position`, the
  level throttle is `controller::level_emitter`. Remaining branches: cached native window `Some` (Win32/AppKit
  show/hide) / `None` (webview show/hide), the per-OS `raw_window` `cfg`s and the `if let Some(window)` guards. `TauriOverlayWindow` methods run with the `Overlay`
  lock held, from any thread, while the overlay window belongs to the main thread: they must never wait on
  another thread (post, don't send), or a main-thread caller waiting for that lock deadlocks the app. The native
  window is therefore resolved once in `TauriOverlayWindow::new` on the main thread (`WebviewWindow::hwnd()` and
  `ns_window()` are blocking event-loop round-trips in tauri-runtime-wry), never in `show`/`hide`. Commands
  that touch `svc.overlay`, directly or through `dictation::`, are async (`#[tauri::command(async)]` or `async fn`)
  so they never wait for the lock on the main thread (checked by
  `commands_touching_the_overlay_never_run_on_the_main_thread`).
- `main.rs`: `bootstrap::needs_setup` (same case table as `needsSetup` in `src/lib/apiKeys.ts`),
  `bootstrap::hides_on_close`, `audio_mute::recover` (sound left muted by a crash, queued on the mute worker), `AudioMute::restore_before_exit` (sound muted by a recording when Scribe quits). Remaining branches: `?` on setup steps,
  the delayed `update::check_at_startup` spawn, the single-instance callback (a second launch shows the window unless `bootstrap::shows_main_at_launch` says otherwise), the keyboard-hook `Ok` (keep the handle) / `Err` (log + `Notice::HookUnavailable` toast) dispatch, the OS languages
  read with `sys-locale` (resolution: `locale::resolve`), `if shows_main_at_launch(..) { show_main }` (rule: `bootstrap::shows_main_at_launch`), and the
  `CloseRequested` match before `prevent_close` + `hide`, the `RunEvent::Exit` match and the `try_state` guard before
  `restore_before_exit`.

Test-only Rust code is **not** excluded and counts toward the Rust total: `src-tauri/src/testing.rs` (shared
`#[cfg(test)]` fakes and the `Fixture`) and the `#[cfg(test)] mod fake` blocks in `overlay.rs` and `secrets.rs`.
llvm-cov cannot drop `cfg(test)` blocks inside a file, and excluding only `testing.rs` would be inconsistent; the
effect is small (about 150 lines, always fully run). Judge a file's own coverage in the per-file report, not only
from the total.

When you add or remove an exclusion, update this list, the regex/globs above, and the tooling in the same commit.

## PR titles and commit subjects

The changelog is generated from them (semantic-release, squash merge: the PR title becomes the commit subject).
A `feat` or `fix` that the user can notice describes **what changes for the user**, in their words, never how it
was done: `fix(macos): paste the dictation instead of typing "v"`, not
`fix(macos): look up the "v" key with UCKeyTranslate`. The technical how goes in the PR body.

## Language

Everything on GitHub is in English: code, comments, docs, commit messages, PR titles and descriptions, review
comments. Only the app's UI strings are in French.

## Rust formatting

`cargo fmt` with `rustfmt.toml` (120 columns): `bun run fmt`. CI fails on unformatted code
(`bun run fmt:check`); the pre-commit hook in `.githooks/` (enabled by `bun install` through `core.hooksPath`)
reformats and aborts the commit when a staged Rust change is not formatted.

## Checks before finishing

`bun run fmt:check`, `cargo test --workspace`, `bun run test`, `bun run check`, `bun run build`, `bun run coverage:ui`, `bun run coverage:rust`.
