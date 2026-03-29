use std::collections::BTreeMap;
use zellij_tile::prelude::*;

const PIPE_NAME: &str = "new-tab-right";
const TIMEOUT_WAITING: f64 = 5.0;

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
