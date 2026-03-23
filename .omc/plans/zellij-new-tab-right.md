# Plan: Zellij "New Tab Next to Current" WASM Plugin (Revised v3)

**Date:** 2026-03-23
**Status:** APPROVED — Consensus reached (2 iterations, Architect + Critic)
**Complexity:** MEDIUM

---

## RALPLAN-DR Summary

### Principles

1. **Spike before commit** — The highest-leverage unknown (run_command from WASM) must be verified before building the full state machine.
2. **Minimal footprint** — Ship the smallest possible plugin that does one thing well; no config UI, no settings, no feature creep.
3. **State machine correctness** — The async new_tab -> detect -> move flow must be robust against race conditions and edge cases.
4. **CWD via pipe payload** — CWD cannot be read from PaneInfo (no cwd field). The keybinding must capture PWD externally and pass it as a pipe message payload.
5. **Simple installation** — A user should be able to build and install with 2-3 commands and a small KDL snippet.

### Decision Drivers (Top 3)

1. **`run_command` is the critical unknown** — `run_command(&["zellij", "action", "move-tab", "--direction", "left"])` from inside WASM is the ONLY way to reposition tabs. If it fails, repositioning is impossible with current API, and the plugin delivers minimal value over `zellij action new-tab --cwd`.
2. **PaneInfo has NO CWD field** — The previous plan assumed CWD could be tracked via PaneUpdate. This is wrong. CWD must come from the keybinding via pipe message payload.
3. **`new_tab()` returns `()`** — Cannot get tab_id from the call. Must detect the new tab from the next TabUpdate event (the newly focused tab).

### Viable Options

#### Option A: WASM plugin with run_command for tab movement + pipe payload for CWD (RECOMMENDED)

Plugin receives CWD via pipe message payload (keybinding captures `$PWD` and sends it). Plugin calls `new_tab(cwd)`, waits for TabUpdate to detect the new tab's position, then calls `run_command(&["zellij", "action", "move-tab", "--direction", "left"])` N times to reposition.

| Pros | Cons |
|---|---|
| Event-driven coordination via TabUpdate | run_command from WASM is unverified (spike required) |
| run_command result can be checked via RunCommandResult event | State machine complexity for async move operations |
| CWD passed explicitly — no ambiguity about source | Requires N sequential run_command calls for N positions of movement |
| Single plugin handles the full flow atomically | User's keybinding must include shell expansion for PWD |

#### Option B: Shell script wrapper (NO plugin)

Keybinding runs: `bash -c 'CUR=$(zellij action query-tab-names | ...); zellij action new-tab --cwd "$PWD"; for i in $(seq 1 $N); do zellij action move-tab --direction left; done'`

| Pros | Cons |
|---|---|
| Zero Rust/WASM toolchain needed | INVALIDATED: No `zellij action query-tab-names` that gives positions |
| Simple to understand | Race condition: tab list changes between query and move |
| | Cannot detect which tab is "new" after creation |
| | No RunCommandResult feedback — fire and forget |

**Invalidation rationale:** Shell cannot atomically read tab positions AND act on them. Between reading the tab count and issuing move commands, another tab could be created/closed. Also, there is no CLI command to get the current tab's numeric position reliably.

#### Option B2: Shell function with sleep (ACKNOWLEDGED as viable alternative)

Shell function that creates a new tab, sleeps briefly, then issues move-tab commands:
```bash
new-tab-right() {
    local cur_pos=$(zellij action dump-layout | ... ) # parse current position
    zellij action new-tab --cwd "$PWD"
    sleep 0.2
    zellij action move-tab --direction left
}
```

| Pros | Cons |
|---|---|
| Achieves ~80% of goal with zero plugin complexity | Timing-based coordination is fragile (sleep duration is a guess) |
| CWD passing works well (`$PWD` is directly available) | No event feedback — cannot confirm tab was created before moving |
| No Rust/WASM toolchain required | May break under load or slow systems |
| Trivial to understand and modify | Not extensible for future features |

**Why not chosen:** The WASM plugin is chosen over this approach for: deterministic event-driven sequencing (no sleep-based timing), robust error handling via RunCommandResult, and extensibility for future features (e.g., tab naming, position strategies). However, this is a legitimate ~80% solution that users may prefer for its simplicity.

#### Option C: WASM plugin WITHOUT repositioning (phase 1 only)

Plugin receives CWD via pipe, calls `new_tab(cwd)`. No repositioning. Tab appears at the end.

| Pros | Cons |
|---|---|
| No run_command dependency | Delivers near-zero value over `zellij action new-tab --cwd` |
| Simpler state machine | Does not achieve the spec ("next to current") |
| Works today, guaranteed | The unique value proposition is repositioning |

**Why not recommended:** As the Critic noted, the "next to current" positioning IS the feature. Without it, users should just use `zellij action new-tab --cwd $(pwd)` directly. Option C is the fallback ONLY if the spike in Step 1 fails.

---

## ADR: Architectural Decision Record

**Decision:** Build a Rust WASM plugin that receives CWD via pipe message payload, creates a new tab with that CWD, detects the new tab via TabUpdate events, and repositions it using `run_command(&["zellij", "action", "move-tab", ...])` issued N times.

**Drivers:**
- PaneInfo has no CWD field — must pass CWD externally via pipe payload
- No move_tab() function in zellij-tile plugin API — must use run_command to invoke CLI
- new_tab() returns () — must detect new tab from TabUpdate event
- run_command from WASM is unverified — spike required before full implementation

**Alternatives considered:**
- Shell wrapper (Option B) — cannot atomically read positions and act; race conditions
- Shell function with sleep (Option B2) — achieves ~80% with zero plugin complexity, but timing-based coordination is fragile and CWD passing works well. ACKNOWLEDGED as viable alternative. The WASM plugin is chosen for: deterministic event-driven sequencing, no sleep-based timing, extensibility for future features.
- Plugin without repositioning (Option C) — delivers no unique value over CLI one-liner
- Previous plan's PaneUpdate CWD tracking (v1) — PaneInfo has no cwd field; impossible

**Why chosen:** Option A is the only approach that can coordinate tab creation, position detection, and repositioning atomically within Zellij's event system. The run_command workaround (identified by Critic) makes full repositioning potentially achievable with current API.

**Consequences:**
- Spike must pass before committing to full implementation; if it fails, project is blocked until Zellij exposes move_tab in plugin API
- N sequential run_command calls may have visible "tab sliding" effect during repositioning
- Keybinding is slightly more complex (must capture PWD via shell expansion)
- Plugin requires RunCommands permission in addition to ReadApplicationState and ChangeApplicationState

**Follow-ups:**
- If run_command spike fails: file a Zellij feature request for `move_tab(direction)` in zellij-tile plugin API
- Monitor zellij-tile for native move_tab() — would eliminate run_command dependency
- Monitor for PaneInfo.cwd field — would simplify CWD passing

---

## Confirmed API Surface (zellij-tile 0.43.1, from docs.rs)

```rust
// CONFIRMED AVAILABLE:
pub fn new_tab<S: ToString + AsRef<str>>(name: Option<S>, cwd: Option<S>) // returns ()
pub fn run_command(cmd: &[&str], context: BTreeMap<String, String>)       // runs shell cmd
pub fn switch_tab_to(tab_idx: u32)                                        // 1-indexed
pub fn subscribe(event_list: &[EventType])
pub fn request_permission(permissions: &[PermissionType])
pub fn set_timeout(seconds: f64)                                          // triggers Event::Timer(f64)

// CONFIRMED EVENTS:
Event::TabUpdate(Vec<TabInfo>)        // TabInfo has: position (0-indexed), active, name
Event::RunCommandResult(i32, Vec<u8>, Vec<u8>, BTreeMap<String, String>)
Event::PermissionRequestResult(PermissionStatus)
Event::Timer(f64)                     // fired after set_timeout expires

// CONFIRMED PERMISSIONS NEEDED:
PermissionType::ReadApplicationState   // for TabUpdate subscription
PermissionType::ChangeApplicationState // for new_tab, switch_tab_to
PermissionType::RunCommands            // for run_command

// DOES NOT EXIST:
// - PaneInfo.cwd (no CWD field on pane info)
// - move_tab() in zellij-tile::shim
// - new_tab() returning tab_id or index
// - MoveTabByTabId as a PluginCommand
```

---

## Implementation Plan

### Project Structure

```
zellij-new-tab-next-to-current/
  Cargo.toml
  .cargo/config.toml        # default target wasm32-wasip1
  src/
    lib.rs                   # Plugin entry point, state machine, event handlers
  config/
    new-tab-right.kdl        # Example KDL keybinding snippet
```

---

### Step 1: Spike — Verify run_command works from WASM plugin

**Goal:** Confirm that `run_command(&["zellij", "action", "move-tab", "--direction", "left"], ctx)` actually works when called from inside a WASM plugin. This is the highest-leverage unknown. If it fails, the full spec is unachievable with current API.

**Files:** `Cargo.toml`, `.cargo/config.toml`, `src/lib.rs`

**Cargo.toml contents:**
```toml
[package]
name = "zellij-new-tab-next-to-current"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
zellij-tile = "0.43"
```

**.cargo/config.toml contents:**
```toml
[build]
target = "wasm32-wasip1"
```

**Approach:**
- Scaffold minimal Cargo project (wasm32-wasip1, zellij-tile 0.43 dependency, cdylib crate type)
- Implement a trivial plugin that on pipe message:
  1. Calls `run_command(&["zellij", "action", "move-tab", "--direction", "left"], BTreeMap::new())`
  2. Logs the RunCommandResult (exit code, stdout, stderr) via `eprintln!` (visible in Zellij log)
- Build, load in Zellij, create 3 tabs, focus last tab, trigger the plugin
- Check: did the focused tab move left by one position?

**Acceptance criteria:**
- run_command completes with exit code 0
- The focused tab visibly moves one position to the left
- RunCommandResult event is received by the plugin
- **Verify that after `new_tab()`, the TabUpdate event shows the new tab as `active == true`** (confirms new_tab auto-focuses)
- **Compare tab counts before/after `new_tab()` as secondary signal** (tab count should increase by exactly 1)

**If spike FAILS:**
- Document the failure mode (permission denied? command not found? no effect?)
- Fall back to Option C (new tab without repositioning) as an interim solution
- File Zellij feature request for `move_tab(direction)` in plugin API
- Plan is BLOCKED for the repositioning feature until upstream resolves

---

### Step 2: Implement the full state machine plugin

**Files:** `src/lib.rs`

**State machine design:**

```
enum PluginState {
    Idle,
    WaitingForNewTab {
        original_tab_position: usize,  // 0-indexed, from TabUpdate
        target_position: usize,        // original_tab_position + 1
        tab_count_before: usize,       // tab count before new_tab() call
        created_at: std::time::Instant, // or use set_timeout for timeout detection
    },
    MovingTab {
        target_position: usize,        // where the new tab should end up
        moves_remaining: usize,        // decremented on each RunCommandResult
        move_seq: usize,               // sequence counter for context tagging
        tab_count: usize,              // expected tab count (for drop detection)
    },
}
```

**Timeout handling:**

Since `set_timeout(seconds: f64)` is available in zellij-tile and fires `Event::Timer(f64)`, the plugin uses it for timeout detection:
- When transitioning to `WaitingForNewTab`, call `set_timeout(5.0)` (5 second timeout)
- When transitioning to `MovingTab`, call `set_timeout(10.0)` (10 second timeout for all moves)
- On `Event::Timer`: if still in `WaitingForNewTab` or `MovingTab`, log error and abort to `Idle`
- On successful transition to `Idle`, any pending timer that fires is ignored (state is already `Idle`)

**Plugin struct fields:**
- `state: PluginState`
- `tabs: Vec<TabInfo>` — latest snapshot from TabUpdate
- `move_seq_counter: usize` — monotonically increasing counter for context tags

**Event flow:**

1. **`load()`**
   - Request permissions: `ReadApplicationState`, `ChangeApplicationState`, `RunCommands`
   - Subscribe to: `TabUpdate`, `RunCommandResult`, `PermissionRequestResult`, `Timer`

2. **`pipe()` handler (trigger)**
   - **Check that `pipe_message.name == "new-tab-right"` before processing** (ignore other pipe messages)
   - Payload contains CWD string (from keybinding shell expansion)
   - If state is not `Idle`, ignore (guard against double-trigger)
   - Find the focused tab from `self.tabs` (the one with `active == true`)
   - Record `original_tab_position = focused_tab.position`
   - Set `target_position = original_tab_position + 1`
   - Record `tab_count_before = self.tabs.len()`
   - Transition to `WaitingForNewTab { original_tab_position, target_position, tab_count_before, ... }`
   - Call `set_timeout(5.0)` for timeout detection
   - Call `new_tab(None, Some(cwd_from_payload))` (or `new_tab(None, None)` if payload is empty)

3. **`update()` on `PermissionRequestResult(status)`**
   - If `status` is granted: log success, continue (permissions are now active)
   - **If `status` is denied: log error ("Permissions denied — plugin cannot function. Please grant ReadApplicationState, ChangeApplicationState, and RunCommands permissions."), stay in `Idle`**
   - Document in README that permissions are required and the user must click "Grant" when prompted

4. **`update()` on `TabUpdate(tabs)`**
   - Always store `self.tabs = tabs`
   - If state is `WaitingForNewTab { target_position, tab_count_before, .. }`:
     - Find the currently focused (active) tab — this is the newly created tab (new_tab auto-focuses)
     - Verify tab count increased: `self.tabs.len() > tab_count_before` (secondary signal)
     - `new_tab_position = focused_tab.position`
     - **Guarded underflow calculation:**
       ```rust
       if new_tab_position < target_position {
           eprintln!("[new-tab-right] ERROR: new tab position ({}) < target position ({}). This should not happen. Aborting to Idle.", new_tab_position, target_position);
           self.state = PluginState::Idle;
           return;
       }
       let moves_needed = new_tab_position - target_position;
       ```
     - If `moves_needed == 0`: transition to `Idle` (already in correct position, e.g., last tab case)
     - If `moves_needed > 0`: transition to `MovingTab { target_position, moves_remaining: moves_needed, move_seq: 0, tab_count: self.tabs.len() }`, call `set_timeout(10.0)`, issue first `run_command` with explicit context:
       ```rust
       let mut ctx = BTreeMap::new();
       ctx.insert("action".to_string(), "move-tab-left".to_string());
       ctx.insert("seq".to_string(), self.move_seq_counter.to_string());
       self.move_seq_counter += 1;
       run_command(&["zellij", "action", "move-tab", "--direction", "left"], ctx);
       ```
   - **If state is `MovingTab { tab_count, .. }`: check if tab count has dropped below expected. If `self.tabs.len() < tab_count`, log error ("Tab count dropped during move — a tab was closed. Aborting.") and transition to `Idle`.**

5. **`update()` on `RunCommandResult(exit_code, stdout, stderr, context)`**
   - **Verify context: check that `context.get("action") == Some("move-tab-left")`** before processing. Ignore unrelated RunCommandResults.
   - If state is `MovingTab { moves_remaining, .. }`:
     - If `exit_code != 0`: log error (include stderr), transition to `Idle` (abort gracefully)
     - Decrement `moves_remaining`
     - If `moves_remaining == 0`: transition to `Idle` (done)
     - If `moves_remaining > 0`: issue next `run_command` move-tab left with incremented seq context

6. **`update()` on `Timer(_)`**
   - If state is `WaitingForNewTab`: log error ("Timed out waiting for TabUpdate after new_tab(). Aborting to Idle."), transition to `Idle`
   - If state is `MovingTab`: log error ("Timed out waiting for move-tab commands to complete. Aborting to Idle."), transition to `Idle`
   - If state is `Idle`: ignore (stale timer from a previous operation that completed normally)

**Edge case handling:**
- Single tab: position=0, target=1, new tab at position 1 — 0 moves. Correct.
- Last tab focused: position=N-1, target=N, new tab at N — 0 moves. Correct.
- Middle tab (B in [A,B,C]): position=1, target=2, new tab appended at 3 — 1 move left. Result: [A,B,NEW,C]. Correct.
- Double-trigger: state != Idle, pipe message ignored.
- run_command failure: logs error, returns to Idle. Tab exists but may be in wrong position.
- Timeout: if TabUpdate never arrives or RunCommandResult never arrives, timer fires and aborts to Idle.
- usize underflow: explicitly guarded — if new_tab_position < target_position, logs error and aborts.
- Permission denial: logs error, stays in Idle. Plugin is non-functional until permissions are granted.
- Tab closed during move: TabUpdate with reduced count detected, aborts to Idle.
- Wrong pipe message name: ignored (only "new-tab-right" is processed).

**Acceptance criteria:**
- Plugin compiles to WASM with no errors
- State machine transitions: Idle -> WaitingForNewTab -> MovingTab -> Idle
- Double-trigger while not Idle is ignored
- RunCommandResult with non-zero exit code aborts gracefully to Idle
- All three edge cases (single, middle, last) produce correct move counts
- Timeout fires and aborts gracefully if stuck in WaitingForNewTab or MovingTab
- Permission denial is handled gracefully
- Context tags on run_command are verified in RunCommandResult handler
- usize underflow is guarded against

---

### Step 3: KDL configuration with CWD capture

**Files:** `config/new-tab-right.kdl`

**Key design decision:** The keybinding must capture the focused pane's CWD and send it as the pipe message payload. Since PaneInfo has no CWD field, the shell running in the focused pane must provide it. The recommended approach uses Zellij's `MessagePlugin` with a `launch_new` strategy so the plugin is autoloaded, plus a separate keybinding action that pipes PWD.

**KDL keybinding configuration:**

```kdl
// Add to your ~/.config/zellij/config.kdl

keybinds {
    shared_except "locked" {
        bind "Alt n" {
            // This triggers the plugin with the current pane's shell PWD as payload.
            // The shell in the focused pane must support $PWD.
            MessagePlugin "file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm" {
                name "new-tab-right"
                // CWD is passed via the payload field
                // Unfortunately, KDL MessagePlugin does not support shell expansion in payload.
                // So we use a different approach: the plugin is launched, and CWD
                // comes from a pipe message sent by a shell command.
            }
        }
    }
}
```

**CWD challenge and resolution:**

The KDL `MessagePlugin` directive does NOT support shell variable expansion (`$PWD`). The `payload` field is a static string. Therefore, we need one of these approaches:

**Approach 3a (Preferred): Plugin uses a fallback CWD**
- The keybinding sends a static `MessagePlugin` trigger (no CWD in payload)
- The plugin calls `new_tab(None, None)` — Zellij will use the session's default CWD
- This sacrifices CWD inheritance but delivers the core "next to current" feature
- CWD inheritance can be added later if Zellij adds PaneInfo.cwd or shell-expansion in pipe payloads

**Approach 3b (If shell keybinding is acceptable): Use `Run` action with pipe**
- The keybinding runs a shell command that captures PWD and pipes it to the plugin:
```kdl
keybinds {
    shared_except "locked" {
        bind "Alt n" {
            Run "bash" "-c" "zellij pipe --plugin 'file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm' --name 'new-tab-right' -- \"$PWD\"" {
                // Note: This runs bash in the *current focused pane's shell context*
                // Actually: Run spawns a NEW pane. This does NOT capture the focused pane's PWD.
                // INVALIDATED: Run creates a new process, $PWD is the session root, not focused pane's CWD.
            }
        }
    }
}
```

**Approach 3c (Actual working approach): Shell alias/function + zellij pipe**
- User defines a shell function in their .bashrc/.zshrc:
  ```bash
  new-tab-right() {
      zellij pipe --plugin 'file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm' \
          --name 'new-tab-right' -- "$PWD"
  }
  ```
- User runs `new-tab-right` from their shell, OR binds a key in their terminal (not Zellij KDL) to run it
- This DOES capture the correct CWD because it runs in the focused pane's shell

**Approach 3d (Simplest, recommended for v1): MessagePlugin without CWD**
- Use the simple `MessagePlugin` KDL keybinding (works from any context, any pane type)
- Plugin calls `new_tab(None, None)` — new tab gets session default CWD
- Document the CWD limitation clearly
- Users who want CWD inheritance can use the shell function from Approach 3c

**Recommendation:** Ship with BOTH approaches documented:
1. KDL keybinding (Approach 3d) as the primary — works everywhere, no CWD inheritance
2. Shell function (Approach 3c) as the advanced option — CWD inheritance works

**KDL config (final, Approach 3d):**
```kdl
keybinds {
    shared_except "locked" {
        bind "Alt n" {
            MessagePlugin "file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm" {
                name "new-tab-right"
            }
        }
    }
}
```

**Shell function (Approach 3c, documented in README):**
```bash
# Add to ~/.bashrc or ~/.zshrc for CWD inheritance:
new-tab-right() {
    zellij pipe \
        --plugin 'file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm' \
        --name 'new-tab-right' \
        -- "$PWD"
}
# Then: bind to a key in your shell, or just type `new-tab-right`
```

**Plugin pipe handler logic:**
- If payload is non-empty: use it as CWD for new_tab
- If payload is empty/missing: call new_tab(None, None) — session default CWD

#### Known Limitations

**The KDL keybinding (Approach 3d) does NOT satisfy the spec's CWD inheritance requirement.** The `MessagePlugin` directive in KDL does not support shell variable expansion, so `$PWD` cannot be passed as the payload. Users who need CWD inheritance MUST use the shell function approach (Approach 3c) instead. This is a fundamental limitation of Zellij's KDL keybinding system, not a plugin limitation.

**Acceptance criteria:**
- KDL snippet is syntactically valid
- MessagePlugin keybinding does not conflict with Zellij defaults (Alt+n is not a default binding)
- Shell function correctly captures $PWD and pipes it to the plugin
- Plugin handles both cases: with CWD payload and without
- Both approaches are clearly documented with tradeoffs

---

### Step 4: Build, install, and verify

**No new source files.** This step is build and test instructions.

**Build:**
```bash
rustup target add wasm32-wasip1
cargo build --release
# Output: target/wasm32-wasip1/release/zellij_new_tab_next_to_current.wasm
```

**Install:**
```bash
mkdir -p ~/.config/zellij/plugins
cp target/wasm32-wasip1/release/zellij_new_tab_next_to_current.wasm \
   ~/.config/zellij/plugins/
```
Then add the KDL snippet to `~/.config/zellij/config.kdl`.

**Verification matrix:**

| # | Setup | Action | Expected Result |
|---|-------|--------|-----------------|
| 1 | Tabs [A, B, C], B focused | Alt+n (KDL binding) | [A, B, NEW, C], NEW focused, session default CWD |
| 2 | Tabs [A, B, C], B focused, shell in /tmp | `new-tab-right` (shell fn) | [A, B, NEW, C], NEW focused, CWD = /tmp |
| 3 | Single tab [A] | Alt+n | [A, NEW], NEW focused |
| 4 | Tabs [A, B, C], C focused (last) | Alt+n | [A, B, C, NEW], NEW focused |
| 5 | Trigger Alt+n twice rapidly | Second trigger ignored | No crash, no extra tabs |
| 6 | Default Ctrl+t n | Standard new tab | Tab appended at end (unchanged behavior) |
| 7 | Permission denied on prompt | Alt+n | Plugin logs error, no crash, stays idle |
| 8 | Tab closed during move | Close a tab mid-operation | Plugin detects count drop, aborts to Idle |

**Acceptance criteria:**
- WASM binary is under 5 MB
- All 8 verification tests pass
- Plugin logs (visible in Zellij log) show clean state transitions
- No panics or error messages in Zellij log during normal operation

---

## Work Objectives Summary

| # | Step | Files | Acceptance | Blocks |
|---|------|-------|------------|--------|
| 1 | Spike: run_command from WASM | `Cargo.toml`, `.cargo/config.toml`, `src/lib.rs` (minimal) | move-tab works via run_command; TabUpdate shows new tab as active; tab count increases by 1 | Steps 2-4 |
| 2 | Full state machine plugin | `src/lib.rs` | Compiles, handles all edge cases, 3-state machine with timeouts, context tags, underflow guard, permission denial handling, pipe name filtering, tab count monitoring | Step 1 |
| 3 | KDL config + shell function | `config/new-tab-right.kdl` | Valid KDL, both approaches documented, CWD limitation documented | Step 1 |
| 4 | Build + verify | (none) | All 8 verification tests pass | Steps 2, 3 |

## Guardrails

**Must Have:**
- Spike (Step 1) passes before proceeding to Step 2
- State machine guards against double-trigger
- Graceful abort on run_command failure (non-zero exit)
- Works with zellij-tile 0.43
- RunCommands permission requested and granted
- Timeout handling via `set_timeout` for WaitingForNewTab and MovingTab states
- Context tagging on run_command calls with `{"action": "move-tab-left", "seq": "<N>"}`
- Context verification in RunCommandResult handler
- Guarded usize subtraction (saturating_sub or explicit check) to prevent underflow
- Pipe name filtering (`pipe_message.name == "new-tab-right"`)
- Permission denial handling (log error, stay Idle)
- Tab count monitoring in MovingTab state

**Must NOT Have:**
- Modifications to default new-tab behavior
- External dependencies beyond zellij-tile
- Interactive UI / configuration panels
- Any assumption that PaneInfo contains CWD
- Any assumption that new_tab returns a tab ID

---

## Success Criteria

1. Spike passes: `run_command(&["zellij", "action", "move-tab", "--direction", "left"])` works from WASM plugin
2. `[A, B, C]` with B focused -> trigger -> `[A, B, NEW, C]` with NEW focused
3. Single tab `[A]` -> trigger -> `[A, NEW]`
4. Last tab `[A, B, C]` with C focused -> trigger -> `[A, B, C, NEW]`
5. CWD inheritance works via shell function approach
6. Default `new-tab` action unchanged
7. Plugin binary builds cleanly on stable Rust with `wasm32-wasip1` target
8. Permission denial is handled gracefully (no crash, error logged)
9. Timeout fires and recovers to Idle if stuck

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| run_command does not work from WASM | Medium | BLOCKER | Spike in Step 1 before any other work |
| run_command works but move-tab has visible "sliding" flicker | Medium | Low (cosmetic) | Acceptable for v1; note in docs |
| Zellij 0.43 changes TabUpdate event shape | Low | Medium | Pin zellij-tile version in Cargo.toml |
| Multiple rapid move-tab commands race with each other | Medium | Medium | Wait for RunCommandResult before issuing next move |
| User expects CWD inheritance from KDL keybinding | High | Low | Document clearly that CWD requires shell function approach |
| Plugin stuck in WaitingForNewTab or MovingTab | Low | Medium | set_timeout(5.0/10.0) fires Timer event, aborts to Idle |
| Permission denied by user | Low | Medium | Log clear error message, stay in Idle, document in README |
| Tab closed by user during MovingTab | Low | Medium | Monitor tab count in TabUpdate, abort if count drops |
| usize underflow in moves_needed calculation | Low | High (panic) | Explicit guard: if new_pos < target_pos, log and abort |
