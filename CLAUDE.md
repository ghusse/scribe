# Scribe: rules for agents

## Coverage rule (mandatory)

At least **95 % line coverage on every language**: Rust, TypeScript and Svelte components. Each language has
its own gate: the Rust workspace total (cargo-llvm-cov), and in vitest one threshold per glob (`src/**/*.ts`,
`src/**/*.svelte`) on top of the UI total, so a well-covered language cannot hide a poorly covered one. CI
fails below any of them.

```bash
bun run coverage:ui     # vitest on the bun runtime + @vitest/coverage-istanbul, thresholds in vitest.config.ts (lines: 95, per glob)
bun run coverage:rust   # cargo llvm-cov --workspace --fail-under-lines 95 --ignore-filename-regex ...
bun run coverage        # both, locally (CI runs them as two independent steps)
```

Reports: UI in `coverage/ui/` (html + lcov); Rust in the terminal (`cargo llvm-cov --workspace --html` for html).
Prerequisites: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`.

Svelte components are covered through component tests (`@testing-library/svelte` + happy-dom, see
`src/main/ModelPicker.test.ts`, which also shows how to observe a `$bindable` prop through a props object with
a getter/setter); mock `@tauri-apps/api/core` (`invoke`) and `@tauri-apps/api/event` (`listen`)
with `vi.mock` and the shared doubles in `tests/tauri.ts` (`commands({...})` answers each command, `emit` fires
an event, `calls` lists a command's arguments), with settings and providers from `tests/fixtures.ts`. Decision logic
stays out of components (`src/overlay/model.ts`, `src/lib/history.ts`, `src/lib/glossary.ts`, ...) and is tested
there as plain functions.

## Test rules

- Prefer small seams (traits, injected functions, pure helpers) over mocking frameworks.
- Tests assert behaviour (outputs, persisted rows, emitted events), not just executed lines.
- Never weaken an existing test. When a test reveals a real bug, fix the code and keep the test.

## Exclusion policy

Excluding a file is allowed **only** for thin OS/framework adapters with no decision logic (raw Windows FFI,
device/clipboard glue, Tauri bootstrap, UI entry files). Any logic in such a file must first be extracted into a
tested module. Every exclusion is listed below and must match the tooling **exactly**.
`tests/coverage-policy.test.ts` (run by `bun run test`) fails when the lines below and the tooling disagree: the
threshold, the Rust regex and the set of Rust files it actually excludes, the vitest excludes and per-glob
thresholds, and the two CI coverage steps.

- Threshold (lines, every language): `95`
- Rust ignore regex (`package.json` > `coverage:rust`):
  `crates.scribe-platform.src.windows.|crates.scribe-platform.src.device.|src-tauri.src.main[.]rs|src-tauri.src.tray[.]rs|src-tauri.src.adapters[.]rs`
  (`.` instead of a path separator so the regex works with both `\` and `/`).
- Rust files excluded by that regex: `crates/scribe-platform/src/device/clipboard.rs`,
  `crates/scribe-platform/src/device/microphone.rs`, `crates/scribe-platform/src/device/mod.rs`,
  `crates/scribe-platform/src/windows/focus.rs`,
  `crates/scribe-platform/src/windows/hook.rs`, `crates/scribe-platform/src/windows/keys.rs`,
  `crates/scribe-platform/src/windows/mod.rs`, `crates/scribe-platform/src/windows/window.rs`, `src-tauri/src/main.rs`,
  `src-tauri/src/tray.rs`, `src-tauri/src/adapters.rs`
- UI excludes (`vitest.config.ts` > `coverage.exclude`): `src/**/*.test.ts`, `src/main/main.ts`,
  `src/overlay/overlay.ts`, `preview/**`
- UI per-language thresholds (`vitest.config.ts` > `coverage.thresholds`): `src/**/*.ts`, `src/**/*.svelte`

| Excluded | Why |
|---|---|
| `crates/scribe-platform/src/windows/` (`focus.rs`, `hook.rs`, `keys.rs`, `window.rs`, `mod.rs`) | Raw Win32/UIA FFI (`SetWindowsHookExW`, `SendInput`, `SetWindowPos`, UI Automation); needs a live desktop session. `mod.rs` re-exports the Windows entry points (`lib.rs` re-exports them, or `fallback.rs` off Windows). |
| `crates/scribe-platform/src/device/` (`microphone.rs`, `clipboard.rs`, `mod.rs`) | Device glue: cpal (default input device, stream per sample format) and arboard (one call per `ClipboardBackend` method). Needs a microphone / the real system clipboard. |
| `src-tauri/src/main.rs` | Tauri bootstrap: builds `Services`, registers commands, starts threads. |
| `src-tauri/src/tray.rs` | Tauri tray icon and menu construction; each menu item calls one tested function. |
| `src-tauri/src/adapters.rs` | Implementations of the app seams on Tauri/Win32/cpal: `TauriUi` (`UiSink`: `emit_to`), `TauriOverlayWindow` (`OverlayWindow`: `emit_to` + show/hide), `CpalRecorder` (`Recorder`), overlay placement call, `show_main`. Needs a running Tauri app, a desktop and a microphone. |
| `src/main/main.ts`, `src/overlay/overlay.ts` | UI entry files: a single `mount(...)` call. |
| `src/**/*.test.ts`, `preview/**` | Tests themselves; local design previews (not shipped). |

What is left in excluded files is wiring only; every decision lives in a tested module:
- `windows/hook.rs::hook_proc` decodes `KBDLLHOOKSTRUCT` and calls `scribe_platform::key_filter::KeyFilter`
  (trigger combination held/released, strict matching, swallowing, capture, pause, `lock_vk == 0`, injected
  events, AltGr's fake left Ctrl, lost key-up / trigger changed while held, menu-mask decision; combination
  rules in `scribe_core::chord`).
  Remaining branches: `code == HC_ACTION`, hook not started (`SHARED` empty), inject the mask / forward /
  swallow per the filter's decision.
- `windows/focus.rs`: the class-name and process-name buffers go through `focus_rules::utf16_prefix` and
  `focus_rules::process_stem`; classification is `focus_rules::classify` (terminals included). `UiaFieldReader`
  only reads the focused field (text pattern, else value; never a password field); whether a paste landed is
  `scribe_core::insert::verify`, the re-reading loop `insert::perform`. Remaining branches: FFI error
  propagation (`?`, `.ok()`), the password / text-pattern / value-pattern `if`s and the null foreground window →
  `FocusSnapshot::unknown()` guard.
- `device/microphone.rs`: the recording-thread protocol is `audio_capture::spawn_recorder`, sample conversions
  `audio_capture::{i16_to_f32, u16_to_f32}`, level metering, partial audio and the final clip
  `audio_capture::CaptureBuffer` (`push`, `set_error`, `finish`). Remaining branches: the cpal error paths
  (no device, unusable config, unsupported format, open/play failure → `ready` error) and the sample-format `match`.
- `device/clipboard.rs`: what to read first (text, then image, else `Unsupported`) and how to restore each
  `ClipboardContent` are `clipboard::BackendClipboard`. Remaining branches: arboard error mapping (`?`).
- `windows/keys.rs`, `windows/window.rs`, `windows/mod.rs`: Win32 calls and re-exports only. `show_overlay`/`hide_overlay` must stay
  non-blocking across threads (`ShowWindowAsync`, `SWP_ASYNCWINDOWPOS`): see the next point.
- `tray.rs`: pause = `controller::toggle_pause` (tested); open = `adapters::show_main`. Remaining branches: the menu-id
  `match` and the left-click filter.
- `adapters.rs`: overlay state/dismiss rules are `overlay::Overlay`, placement is `overlay::overlay_position`, the
  level throttle is `controller::level_emitter`. Remaining branches: cached HWND `Some` (Win32 show/hide) / `None`
  (webview show/hide) and the `if let Some(window)` guards. `TauriOverlayWindow` methods run with the `Overlay`
  lock held, from any thread, while the overlay window belongs to the main thread: they must never wait on
  another thread (post, don't send), or a main-thread caller waiting for that lock deadlocks the app. The HWND
  is therefore resolved once in `TauriOverlayWindow::new` on the main thread (`WebviewWindow::hwnd()` is a
  blocking event-loop round-trip in tauri-runtime-wry), never in `show`/`hide`. Commands
  that touch `svc.overlay` are `#[tauri::command(async)]` so they never wait for the lock on the main thread
  (checked by `commands_touching_the_overlay_never_run_on_the_main_thread`).
- `main.rs`: `bootstrap::needs_setup` (same case table as `needsSetup` in `src/lib/apiKeys.ts`),
  `bootstrap::hides_on_close`, `bootstrap::hook_unavailable_message`. Remaining branches: `?` on setup steps,
  the keyboard-hook `Ok` (keep the handle) / `Err` (log + toast) dispatch, `if shows_main_at_launch(..) { show_main }` (rule: `bootstrap::shows_main_at_launch`), and the
  `CloseRequested` match before `prevent_close` + `hide`.

Test-only Rust code is **not** excluded and counts toward the Rust total: `src-tauri/src/testing.rs` (shared
`#[cfg(test)]` fakes and the `Fixture`) and the `#[cfg(test)] mod fake` blocks in `overlay.rs` and `secrets.rs`.
llvm-cov cannot drop `cfg(test)` blocks inside a file, and excluding only `testing.rs` would be inconsistent; the
effect is small (about 150 lines, always fully run). Judge a file's own coverage in the per-file report, not only
from the total.

When you add or remove an exclusion, update this list, the regex/globs above, and the tooling in the same commit.

## Checks before finishing

`cargo test --workspace`, `bun run test`, `bun run check`, `bun run build`, `bun run coverage:ui`, `bun run coverage:rust`.
