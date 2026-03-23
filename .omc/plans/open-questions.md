# Open Questions

## zellij-new-tab-right - 2026-03-23 (Revised v2)

### Resolved from v1
- [x] PaneUpdate / PaneManifest CWD field — RESOLVED: PaneInfo has NO CWD field. CWD must come externally via pipe message payload or be omitted.
- [x] `MoveTabByTabId` idempotency — RESOLVED: MoveTabByTabId does not exist in plugin API. Using `run_command` with `zellij action move-tab` instead.
- [x] `MessagePlugin` vs `pipe` trigger syntax — RESOLVED: MessagePlugin in KDL keybinds for simple trigger; `zellij pipe` CLI for shell function with CWD payload.

### New open questions (v2)
- [ ] Does `run_command(&["zellij", "action", "move-tab", "--direction", "left"])` work from inside a WASM plugin? — This is the BLOCKER. Spike in Step 1 must answer this before any further implementation. Possible failure modes: permission denied, command not found in WASM sandbox, no effect because the plugin's "focused tab" context differs from the session's.
- [ ] Does `zellij pipe --plugin 'file:...' --name 'new-tab-right' -- "$PWD"` correctly deliver the payload to the plugin's `pipe()` handler? — Needed for CWD inheritance via shell function approach. Must verify exact argument format.
- [ ] What is the latency of sequential `run_command` calls? — If moving a tab 5+ positions, N sequential run_command calls with RunCommandResult waits could be perceptibly slow. May need to document a practical limit or explore batching.
- [ ] Does the plugin receive TabUpdate BEFORE or AFTER the new tab is fully created and focused? — The state machine assumes the first TabUpdate after `new_tab()` will show the new tab as active. If TabUpdate arrives before focus switches, the wrong tab would be identified as "new."
- [ ] Minimum Zellij version for `zellij pipe` CLI command — The `zellij pipe` subcommand may not exist in all 0.38+ versions. Need to verify when it was introduced.
