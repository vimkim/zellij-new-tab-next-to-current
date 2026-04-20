use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use zellij_tile::prelude::*;

const PIPE_NAME: &str = "new-tab-right";
const TIMEOUT_WAITING: f64 = 5.0;

// Cross-instance coordination: zellij can end up with multiple "ghost" plugin
// instances after reattach cycles. MessagePlugin broadcasts to all of them,
// and each runs its own state machine, producing duplicate tabs.
//
// We coordinate via two files on the host FS (zellij maps the session CWD to
// /host). Both files are best-effort — if the FS is not writable, we degrade
// to the old (duplicating) behavior.
//
//   <HEARTBEAT_PATH>  — latest TabUpdate timestamp seen by any live instance.
//                       A ghost that stopped receiving events has a stale local
//                       timestamp and can detect itself by comparing.
//   <LOCK_PATH>       — most recent "I handled this trigger" stamp. Instances
//                       that see a fresh lock (< LOCK_TTL_MS old) bow out.
const HEARTBEAT_FILE: &str = ".zellij-ntr-heartbeat";
const LOCK_FILE: &str = ".zellij-ntr-lock";
const GHOST_TOLERANCE_MS: u128 = 500;
const LOCK_TTL_MS: u128 = 1000;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn session_suffix() -> String {
    std::env::var("ZELLIJ_SESSION_NAME").unwrap_or_else(|_| "default".to_string())
}

fn heartbeat_path() -> String {
    format!("/host/{}-{}", HEARTBEAT_FILE, session_suffix())
}

fn lock_path() -> String {
    format!("/host/{}-{}", LOCK_FILE, session_suffix())
}

fn read_stamp(path: &str) -> u128 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u128>().ok())
        .unwrap_or(0)
}

fn write_stamp(path: &str, ms: u128) -> bool {
    fs::write(path, ms.to_string()).is_ok()
}

/// Try to claim the current trigger. Returns true if we won, false if another
/// instance already claimed within LOCK_TTL_MS.
fn try_claim_trigger(now: u128) -> bool {
    let prev = read_stamp(&lock_path());
    if now.saturating_sub(prev) < LOCK_TTL_MS {
        return false;
    }
    write_stamp(&lock_path(), now)
}

enum PluginState {
    Idle,
    WaitingForNewTab {
        target_position: usize,
        tab_count_before: usize,
    },
}

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    plugin_state: Option<PluginState>,
    permissions_granted: bool,
    // Timestamp (ms since epoch) of the most recent TabUpdate this instance
    // saw. Ghosts that stopped receiving events leave this frozen while the
    // shared heartbeat on disk keeps advancing.
    last_tabupdate_ms: u128,
}

register_plugin!(State);

impl State {
    fn current_state(&self) -> &PluginState {
        self.plugin_state.as_ref().unwrap_or(&PluginState::Idle)
    }

    fn is_idle(&self) -> bool {
        matches!(self.current_state(), PluginState::Idle)
    }

    fn to_idle(&mut self) {
        self.plugin_state = Some(PluginState::Idle);
    }

    fn move_tab_left_n(&self, n: usize) {
        let action = actions::Action::MoveTab {
            direction: Direction::Left,
        };
        for _ in 0..n {
            run_action(action.clone(), BTreeMap::new());
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.plugin_state = Some(PluginState::Idle);
        // Seed with load time so a freshly-loaded instance isn't wrongly
        // classified as a ghost before its first TabUpdate arrives.
        self.last_tabupdate_ms = now_ms();
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunActionsAsUser,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PermissionRequestResult,
            EventType::Timer,
        ]);
        // Don't hide_self() here — wait until permissions are granted,
        // otherwise the permission prompt is never visible to the user.
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        // Only handle our specific pipe message
        if pipe_message.name != PIPE_NAME {
            return false;
        }

        // Guard against double-trigger
        if !self.is_idle() {
            eprintln!("[new-tab-right] Ignoring trigger: not idle");
            return false;
        }

        if !self.permissions_granted {
            eprintln!("[new-tab-right] Permissions not yet granted. Please grant permissions when prompted.");
            return false;
        }

        // Ghost detection: if the shared heartbeat is much newer than my last
        // TabUpdate, another instance is receiving events that I'm not — I'm
        // a ghost from a previous client attach. Bow out.
        let shared_heartbeat = read_stamp(&heartbeat_path());
        if shared_heartbeat > self.last_tabupdate_ms.saturating_add(GHOST_TOLERANCE_MS) {
            eprintln!(
                "[new-tab-right] Ghost instance detected (mine={}ms, shared={}ms, lag={}ms). Aborting.",
                self.last_tabupdate_ms,
                shared_heartbeat,
                shared_heartbeat.saturating_sub(self.last_tabupdate_ms)
            );
            return false;
        }

        // Cross-instance trigger lock: even among fresh instances, only one
        // should process a given pipe.
        let now = now_ms();
        if !try_claim_trigger(now) {
            eprintln!("[new-tab-right] Another instance already claimed this trigger. Aborting.");
            return false;
        }

        // Find the focused tab
        let focused_tab = match self.tabs.iter().find(|t| t.active) {
            Some(tab) => tab,
            None => {
                eprintln!("[new-tab-right] No focused tab found");
                return false;
            }
        };

        let original_position = focused_tab.position;
        let target_position = original_position + 1;
        let tab_count_before = self.tabs.len();

        eprintln!(
            "[new-tab-right] Triggered: current tab at position {}, target position {}, tab count {}",
            original_position, target_position, tab_count_before
        );

        // Transition to WaitingForNewTab
        self.plugin_state = Some(PluginState::WaitingForNewTab {
            target_position,
            tab_count_before,
        });

        // Set timeout
        set_timeout(TIMEOUT_WAITING);

        // Create new tab with CWD from payload if provided
        let cwd = pipe_message
            .payload
            .as_deref()
            .filter(|s| !s.is_empty());

        match cwd {
            Some(cwd_path) => {
                eprintln!("[new-tab-right] Creating new tab with CWD: {}", cwd_path);
                new_tab(None::<&str>, Some(cwd_path));
            }
            None => {
                eprintln!("[new-tab-right] Creating new tab with session default CWD");
                new_tab(None::<&str>, None::<&str>);
            }
        }

        false
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                match status {
                    PermissionStatus::Granted => {
                        self.permissions_granted = true;
                        hide_self();
                        eprintln!("[new-tab-right] Permissions granted");
                    }
                    PermissionStatus::Denied => {
                        self.permissions_granted = false;
                        eprintln!(
                            "[new-tab-right] Permissions denied. Plugin requires \
                             ReadApplicationState, ChangeApplicationState, and RunActionsAsUser."
                        );
                    }
                }
                false
            }

            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                // Advance both local and shared heartbeat so ghosts can detect
                // they've fallen behind.
                self.last_tabupdate_ms = now_ms();
                write_stamp(&heartbeat_path(), self.last_tabupdate_ms);

                match self.plugin_state.take().unwrap_or(PluginState::Idle) {
                    PluginState::WaitingForNewTab {
                        target_position,
                        tab_count_before,
                    } => {
                        // Verify tab count increased
                        if self.tabs.len() <= tab_count_before {
                            eprintln!(
                                "[new-tab-right] TabUpdate received but tab count didn't increase ({} <= {}). Waiting...",
                                self.tabs.len(), tab_count_before
                            );
                            self.plugin_state = Some(PluginState::WaitingForNewTab {
                                target_position,
                                tab_count_before,
                            });
                            return false;
                        }

                        // Find the new focused tab (new_tab auto-focuses the new tab)
                        let focused = match self.tabs.iter().find(|t| t.active) {
                            Some(tab) => tab,
                            None => {
                                eprintln!("[new-tab-right] No focused tab after new_tab(). Aborting.");
                                self.to_idle();
                                return false;
                            }
                        };

                        let new_tab_position = focused.position;
                        eprintln!(
                            "[new-tab-right] New tab detected at position {}, target is {}",
                            new_tab_position, target_position
                        );

                        if new_tab_position < target_position {
                            eprintln!(
                                "[new-tab-right] ERROR: new tab position ({}) < target position ({}). Aborting.",
                                new_tab_position, target_position
                            );
                            self.to_idle();
                            return false;
                        }

                        let moves_needed = new_tab_position - target_position;

                        if moves_needed == 0 {
                            eprintln!("[new-tab-right] No moves needed. Done.");
                        } else {
                            eprintln!("[new-tab-right] Moving {} position(s) left via run_action", moves_needed);
                            self.move_tab_left_n(moves_needed);
                            eprintln!("[new-tab-right] All moves dispatched. Done.");
                        }

                        self.to_idle();
                        false
                    }

                    idle => {
                        self.plugin_state = Some(idle);
                        false
                    }
                }
            }

            Event::Timer(_) => {
                if let PluginState::WaitingForNewTab { .. } = self.current_state() {
                    eprintln!(
                        "[new-tab-right] Timed out waiting for TabUpdate after new_tab(). Aborting."
                    );
                    self.to_idle();
                }
                false
            }

            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        // This plugin has no UI — it runs as a background worker
    }
}
