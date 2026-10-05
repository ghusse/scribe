# Scribe: rules for agents

## Coverage rule (mandatory)

At least **95 % line coverage on every language**: Rust, TypeScript and Svelte components. The gate is
global per tool (Rust workspace total, UI total) and CI fails below it.

```bash
npm run coverage:ui     # vitest + @vitest/coverage-v8, thresholds in vitest.config.ts (lines: 95)
npm run coverage:rust   # cargo llvm-cov --workspace --fail-under-lines 95 --ignore-filename-regex ...
npm run coverage        # both (what CI runs)
```

Reports: UI in `coverage/ui/` (html + lcov); Rust in the terminal (`cargo llvm-cov --workspace --html` for html).
Prerequisites: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`.

Svelte components are covered through component tests (`@testing-library/svelte` + jsdom, see
`src/main/ModelPicker.test.ts`); mock `@tauri-apps/api/core` (`invoke`) and `@tauri-apps/api/event` (`listen`)
with `vi.mock`.

## Test rules

- Prefer small seams (traits, injected functions, pure helpers) over mocking frameworks.
- Tests assert behaviour (outputs, persisted rows, emitted events), not just executed lines.
- Never weaken an existing test. When a test reveals a real bug, fix the code and keep the test.

## Exclusion policy

Excluding a file is allowed **only** for thin OS/framework adapters with no decision logic (raw Windows FFI,
device/clipboard glue, Tauri bootstrap, UI entry files). Any logic in such a file must first be extracted into a
tested module. Every exclusion is listed below and must match the tooling **exactly**:

- Rust (`package.json` > `coverage:rust`): `--ignore-filename-regex "crates.scribe-platform.src.windows.|src-tauri.src.main[.]rs"`
  (`.` instead of a path separator so the regex works with both `\` and `/`).
- UI (`vitest.config.ts` > `coverage.exclude`): `src/**/*.test.ts`, `src/main/main.ts`, `src/overlay/overlay.ts`,
  `preview/**`.

| Excluded | Why |
|---|---|
| `crates/scribe-platform/src/windows/` (`focus.rs`, `hook.rs`, `keys.rs`, `window.rs`, `mod.rs`) | Raw Win32/UIA FFI (`SetWindowsHookExW`, `SendInput`, `SetWindowPos`, UI Automation); needs a live desktop session. |
| `src-tauri/src/main.rs` | Tauri bootstrap: builds `Services`, registers commands, starts threads. |
| `src/main/main.ts`, `src/overlay/overlay.ts` | UI entry files: a single `mount(...)` call. |
| `src/**/*.test.ts`, `preview/**` | Tests themselves; local design previews (not shipped). |

Known debt (logic still inside excluded files, to extract into tested modules, see
`docs/superpowers/reviews/2026-10-05-test-audit.md`):
- `windows/hook.rs::hook_proc`: lock-key swallowing decision (`trigger_down`, pause, `lock_vk == 0`, injected
  events) belongs in a pure `KeyFilter`.
- `main.rs`: the `needs_setup` rule (duplicated in `App.svelte`) belongs in a tested `needs_setup(&Settings, ..)`.

When you add or remove an exclusion, update this list, the regex/globs above, and the tooling in the same commit.

## Checks before finishing

`cargo test --workspace`, `npm test`, `npm run check`, `npm run build`, `npm run coverage`.
