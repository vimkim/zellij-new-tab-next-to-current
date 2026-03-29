# Patch: Instant tab moves via `run_action` (zellij-tile 0.44)

## Problem

After upgrading to zellij-tile 0.44, the plugin worked but tab repositioning was **very slow**. The new tab would visibly slide left one position at a time because each move:

1. Spawned a subprocess (`zellij action move-tab left`)
2. Waited for the `RunCommandResult` event
3. Only then issued the next move

With 10+ tabs, this created a noticeable multi-second delay.

## Root Cause

The plugin used `run_command(&["zellij", "action", "move-tab", "left"])` to move tabs — the only option in zellij-tile 0.43, which lacked a native move-tab API. Each move was a full fork+exec+IPC round-trip, executed sequentially.

## Fix

Replaced `run_command()` (subprocess) with `run_action()` (in-process plugin API), available since zellij-tile 0.44:

```rust
// Before (0.43 — slow, sequential subprocesses)
run_command(&["zellij", "action", "move-tab", "left"], ctx);
// ...then wait for RunCommandResult, then issue next move

// After (0.44 — instant, in-process batch)
let action = actions::Action::MoveTab { direction: Direction::Left };
for _ in 0..moves_needed {
    run_action(action.clone(), BTreeMap::new());
}
```

All moves are dispatched in a single tight loop — no subprocess spawning, no waiting between moves.

## Changes

| Area | Before | After |
|------|--------|-------|
| Move mechanism | `run_command()` (subprocess) | `run_action()` (in-process) |
| Move strategy | Sequential (wait for each result) | Batch (all at once) |
| State machine | 3 states: `Idle`, `WaitingForNewTab`, `MovingTab` | 2 states: `Idle`, `WaitingForNewTab` |
| Permissions | `ReadApplicationState`, `ChangeApplicationState`, `RunCommands` | `ReadApplicationState`, `ChangeApplicationState`, `RunActionsAsUser` |
| Events subscribed | `TabUpdate`, `RunCommandResult`, `PermissionRequestResult`, `Timer` | `TabUpdate`, `PermissionRequestResult`, `Timer` |
| Lines of code | ~340 | ~233 |

## Upgrade Notes

- **Re-grant permissions**: The plugin now requests `RunActionsAsUser` instead of `RunCommands`. Zellij will prompt you to grant permissions again after upgrading.
- **Requires zellij-tile 0.44+**: The `run_action()` API does not exist in 0.43.
