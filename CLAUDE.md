# CLAUDE.md — Project Instructions for Claude Code

## Project Overview

Zellij WASM plugin (Rust) that creates new tabs next to the current tab instead of at the end of the tab bar.

## Build & Install

```bash
just init    # first time: installs wasm target + builds + installs
just bi      # subsequent: build + install
```

Target: `wasm32-wasip1`. Output: binary crate (not cdylib — Zellij expects `_start`).

## Architecture

Single-file plugin (`src/main.rs`) with a 3-state machine:

```
Idle → (pipe trigger) → WaitingForNewTab → (TabUpdate) → MovingTab → (RunCommandResult × N) → Idle
```

Key constraint: Zellij's plugin API has no `move_tab()` function. The plugin works around this by calling `run_command(&["zellij", "action", "move-tab", "left"])` — shelling out to the Zellij CLI from inside the WASM plugin.

## API Limitations (zellij-tile 0.43)

These DO NOT exist — do not attempt to use them:
- `PaneInfo.cwd` — no CWD field on pane info
- `move_tab()` / `MoveTabByTabId` — not in plugin API
- `new_tab()` returning a tab ID — returns `()`

What works:
- `new_tab(name, cwd)` — creates tab at end
- `run_command(cmd, context)` — shells out, result via `RunCommandResult` event
- `hide_self()` — hides plugin pane (background mode)
- `set_timeout(seconds)` — fires `Timer` event for timeout handling
- `TabUpdate` event — provides `Vec<TabInfo>` with `position` and `active` fields

## Testing

No automated tests — must test manually in a live Zellij session:
1. `just bi` to build + install
2. Restart Zellij
3. Create tabs, focus a middle one, press Alt+n
4. Check Zellij logs: `grep "new-tab-right" /tmp/zellij-*/zellij-log/zellij.log`

## Common Pitfalls

- **CLI args**: `zellij action move-tab left` (NOT `--direction left`)
- **Crate type**: Must be a binary (`src/main.rs`), not a library (`src/lib.rs` with cdylib). `register_plugin!` generates `main()`.
- **Plugin pane**: Call `hide_self()` in `load()` to prevent visible pane. Use `load_plugins` in KDL config to pre-load at startup.
- **File naming**: Cargo produces `zellij-new-tab-next-to-current.wasm` (hyphens), not underscores.
