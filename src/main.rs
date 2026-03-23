use std::collections::BTreeMap;
use zellij_tile::prelude::*;

const PIPE_NAME: &str = "new-tab-right";
const TIMEOUT_WAITING: f64 = 5.0;
const TIMEOUT_MOVING: f64 = 10.0;
const CONTEXT_ACTION_KEY: &str = "action";
const CONTEXT_ACTION_VALUE: &str = "move-tab-left";
const CONTEXT_SEQ_KEY: &str = "seq";

enum PluginState {
    Idle,
    WaitingForNewTab {
        target_position: usize,
        tab_count_before: usize,
    },
    MovingTab {
        moves_remaining: usize,
        move_seq: usize,
        tab_count: usize,
    },
}

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    plugin_state: Option<PluginState>,
    seq_counter: usize,
    permissions_granted: bool,
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

    fn issue_move_left(&mut self) {
        let mut ctx = BTreeMap::new();
        ctx.insert(CONTEXT_ACTION_KEY.to_string(), CONTEXT_ACTION_VALUE.to_string());
        ctx.insert(CONTEXT_SEQ_KEY.to_string(), self.seq_counter.to_string());
        self.seq_counter += 1;
        run_command(
            &["zellij", "action", "move-tab", "left"],
            ctx,
        );
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.plugin_state = Some(PluginState::Idle);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::RunCommandResult,
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
                             ReadApplicationState, ChangeApplicationState, and RunCommands."
                        );
                    }
                }
                false
            }

            Event::TabUpdate(tabs) => {
                self.tabs = tabs;

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
                            // Put state back — might be a spurious TabUpdate
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

                        // Guard against underflow
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
                            self.to_idle();
                            return false;
                        }

                        eprintln!("[new-tab-right] Need {} move(s) left", moves_needed);

                        // Transition to MovingTab
                        self.plugin_state = Some(PluginState::MovingTab {
                            moves_remaining: moves_needed,
                            move_seq: 0,
                            tab_count: self.tabs.len(),
                        });

                        set_timeout(TIMEOUT_MOVING);
                        self.issue_move_left();
                        false
                    }

                    PluginState::MovingTab {
                        moves_remaining,
                        move_seq,
                        tab_count,
                    } => {
                        // Check if tab count dropped (a tab was closed during move)
                        if self.tabs.len() < tab_count {
                            eprintln!(
                                "[new-tab-right] Tab count dropped ({} < {}) during move. Aborting.",
                                self.tabs.len(), tab_count
                            );
                            self.to_idle();
                            return false;
                        }
                        // Otherwise keep the state — we're waiting for RunCommandResult
                        self.plugin_state = Some(PluginState::MovingTab {
                            moves_remaining,
                            move_seq,
                            tab_count,
                        });
                        false
                    }

                    idle => {
                        self.plugin_state = Some(idle);
                        false
                    }
                }
            }

            Event::RunCommandResult(exit_code, _stdout, stderr, context) => {
                // Verify this is our move-tab command
                if context.get(CONTEXT_ACTION_KEY).map(|s| s.as_str()) != Some(CONTEXT_ACTION_VALUE)
                {
                    return false;
                }

                match self.plugin_state.take().unwrap_or(PluginState::Idle) {
                    PluginState::MovingTab {
                        moves_remaining,
                        move_seq,
                        tab_count,
                    } => {
                        if exit_code != Some(0) {
                            let err = String::from_utf8_lossy(&stderr);
                            eprintln!(
                                "[new-tab-right] move-tab failed (exit {:?}): {}. Aborting.",
                                exit_code, err
                            );
                            self.to_idle();
                            return false;
                        }

                        let remaining = moves_remaining.saturating_sub(1);
                        eprintln!(
                            "[new-tab-right] Move {} complete, {} remaining",
                            move_seq, remaining
                        );

                        if remaining == 0 {
                            eprintln!("[new-tab-right] All moves complete. Done.");
                            self.to_idle();
                        } else {
                            self.plugin_state = Some(PluginState::MovingTab {
                                moves_remaining: remaining,
                                move_seq: move_seq + 1,
                                tab_count,
                            });
                            self.issue_move_left();
                        }
                        false
                    }
                    other => {
                        // Not in MovingTab — ignore stale result
                        self.plugin_state = Some(other);
                        false
                    }
                }
            }

            Event::Timer(_) => {
                match self.current_state() {
                    PluginState::WaitingForNewTab { .. } => {
                        eprintln!(
                            "[new-tab-right] Timed out waiting for TabUpdate after new_tab(). Aborting."
                        );
                        self.to_idle();
                    }
                    PluginState::MovingTab { .. } => {
                        eprintln!(
                            "[new-tab-right] Timed out waiting for move-tab to complete. Aborting."
                        );
                        self.to_idle();
                    }
                    PluginState::Idle => {
                        // Stale timer from a completed operation — ignore
                    }
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
