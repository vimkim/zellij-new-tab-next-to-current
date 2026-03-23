# Architecture & Code Analysis

## Overview

A Zellij WASM plugin that intercepts a custom keybinding to create new tabs immediately to the right of the focused tab. The entire plugin is a single file (`src/main.rs`, ~340 lines).

## State Machine

The plugin operates as a 3-state finite state machine:

```
                    pipe("new-tab-right")
                ┌──────────────────────────┐
                │                          ▼
            ┌───────┐           ┌─────────────────────┐
     ──────►│ Idle  │◄──────── │   WaitingForNewTab   │
            └───────┘  timeout │  {target_position,   │
                │      or err  │   tab_count_before}  │
                │              └──────────┬────────────┘
                │                         │ TabUpdate (tab count increased)
                │              ┌──────────▼────────────┐
                └────────────  │      MovingTab         │
                   timeout,    │  {moves_remaining,     │
                   err, or     │   move_seq, tab_count} │
                   done        └────────────────────────┘
                                     ▲           │
                                     └───────────┘
                                   RunCommandResult
                                   (moves_remaining > 0)
```

### States

| State | Fields | Entered When | Exits When |
|-------|--------|-------------|------------|
| `Idle` | (none) | Startup, completion, error, timeout | Pipe message received |
| `WaitingForNewTab` | `target_position`, `tab_count_before` | After `new_tab()` called | TabUpdate shows increased tab count |
| `MovingTab` | `moves_remaining`, `move_seq`, `tab_count` | Tab detected, moves needed | All moves done, error, or timeout |

### Transitions

1. **Idle → WaitingForNewTab**: `pipe()` receives message with `name == "new-tab-right"`. Calls `new_tab(name, cwd)` and `set_timeout(5.0)`.

2. **WaitingForNewTab → MovingTab**: `TabUpdate` arrives with `tabs.len() > tab_count_before`. Calculates `moves_needed = new_position - target_position`. Issues first `run_command("zellij action move-tab left")`.

3. **WaitingForNewTab → Idle** (zero moves): New tab already at correct position (e.g., appended when last tab was focused).

4. **MovingTab → MovingTab** (loop): `RunCommandResult` arrives with exit code 0. Decrements `moves_remaining`, issues next move.

5. **MovingTab → Idle** (done): `moves_remaining` reaches 0.

6. **Any → Idle** (error/timeout): Non-zero exit code, timer fires, or tab count drops.

## Key Design Decisions

### Why `run_command` instead of a native API?

Zellij's plugin API (`zellij-tile 0.43`) does not expose `move_tab()`. The `Action::MoveTab` exists in Zellij's core but is not wired into `PluginCommand`. The plugin works around this by shelling out: `run_command(&["zellij", "action", "move-tab", "left"])`.

This is reliable because:
- The `zellij` CLI binary is available on the host
- Environment variables (`ZELLIJ`, `ZELLIJ_SESSION_NAME`) are inherited by `run_command` subprocesses
- `RunCommandResult` provides exit code feedback for error handling

### Why `hide_self()` in `load()`?

When `MessagePlugin` triggers a plugin that isn't loaded, Zellij creates a visible pane for it. Calling `hide_self()` immediately suppresses this pane. Combined with `load_plugins` in the KDL config (pre-loads at startup) and `launch_new false` (reuses existing instance), the plugin runs entirely in the background.

### Why `Option<PluginState>` with `.take()`?

Rust's ownership rules prevent mutating `self` while pattern-matching on `self.plugin_state`. Using `Option::take()` moves the state out temporarily, allowing the match arms to transition freely without borrow conflicts.

### Why sequential moves with `RunCommandResult` waits?

Firing N `run_command` calls simultaneously could race — each `move-tab left` assumes the tab is at a specific position. Waiting for each `RunCommandResult` ensures the previous move completed before issuing the next. This adds latency (~50ms per move) but guarantees correctness.

### CWD passing via pipe payload

`PaneInfo` has no CWD field (as of zellij-tile 0.43). The KDL `MessagePlugin` directive doesn't support shell variable expansion. So CWD inheritance requires the user to invoke the plugin via a shell function that passes `$PWD` as the pipe payload:

```bash
zellij pipe --plugin 'file:...' --name 'new-tab-right' -- "$PWD"
```

The plugin checks `pipe_message.payload` — if present, passes it to `new_tab(None, Some(cwd))`.

## Safety Guards

| Guard | Location | Purpose |
|-------|----------|---------|
| Pipe name filter | `pipe()` | Ignores messages not named `"new-tab-right"` |
| Idle check | `pipe()` | Prevents double-trigger while operation in progress |
| Permission check | `pipe()` | Blocks operation if permissions not yet granted |
| Tab count check | `TabUpdate` in `WaitingForNewTab` | Waits for tab count to actually increase |
| Underflow guard | `TabUpdate` in `WaitingForNewTab` | Prevents `usize` panic if `new_pos < target_pos` |
| Tab count drop | `TabUpdate` in `MovingTab` | Aborts if a tab was closed during repositioning |
| Exit code check | `RunCommandResult` | Aborts on non-zero exit from `zellij action` |
| Context tag check | `RunCommandResult` | Ignores results from unrelated `run_command` calls |
| Timeout (5s) | `Timer` in `WaitingForNewTab` | Recovers if `TabUpdate` never arrives |
| Timeout (10s) | `Timer` in `MovingTab` | Recovers if moves stall |

## Event Subscriptions

| Event | Permission | Usage |
|-------|-----------|-------|
| `TabUpdate` | `ReadApplicationState` | Detect new tab position, track tab list |
| `RunCommandResult` | `RunCommands` | Confirm each move-tab completed |
| `PermissionRequestResult` | (none) | Track whether permissions were granted |
| `Timer` | (none) | Timeout recovery for stuck states |

## File Structure

```
├── Cargo.toml              # zellij-tile 0.43 dependency, wasm32-wasip1 target
├── .cargo/config.toml      # Default build target
├── src/main.rs             # Entire plugin (~340 lines)
├── config/new-tab-right.kdl # Example KDL configuration
├── justfile                # Build/install recipes
├── README.md               # User-facing documentation
├── LICENSE                  # MIT
├── CLAUDE.md               # Claude Code project instructions
└── docs/
    └── ARCHITECTURE.md     # This file
```

## Future Improvements

- **Native `move_tab()` in plugin API**: If Zellij exposes `MoveTab` as a `PluginCommand`, replace `run_command` with the native call. The state machine simplifies (no `RunCommandResult` waits).
- **`PaneInfo.cwd`**: If Zellij adds CWD to `PaneInfo`, the plugin can read CWD directly instead of requiring the shell function workaround.
- **Batch moves**: If visual "tab sliding" bothers users, investigate whether Zellij could support a `move_tab_to_position(index)` action.

## Debugging

All plugin logs use the `[new-tab-right]` prefix via `eprintln!`. View them in:

```bash
grep "new-tab-right" /tmp/zellij-*/zellij-log/zellij.log
```

Each state transition is logged, making it straightforward to trace issues.
