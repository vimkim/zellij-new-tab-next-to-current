# zellij-new-tab-next-to-current

> **Actively maintained** — I use this plugin every day. If something doesn't work, please [open an issue](https://github.com/vimkim/zellij-new-tab-next-to-current/issues) and I'll respond quickly.

A [Zellij](https://zellij.dev/) plugin that opens new tabs **next to the current tab** instead of at the end.

## The Problem

Zellij's default `NewTab` action always appends the new tab at the end of the tab bar. If you have tabs `[A, B, C, D]` and you're on tab B, the new tab appears after D — not next to B where you'd expect it.

## The Solution

This plugin creates a new tab immediately to the **right** of the currently focused tab.

```
Before: [A, B, C, D]  (B focused)
         Press Alt+n
After:  [A, B, NEW, C, D]  (NEW focused)
```

## Install

Requires [Rust](https://rustup.rs/) and [just](https://github.com/casey/just).

```bash
git clone https://github.com/vimkim/zellij-new-tab-next-to-current.git
cd zellij-new-tab-next-to-current
just init   # installs wasm target + builds + copies to ~/.config/zellij/plugins/
```

Or manually:

```bash
rustup target add wasm32-wasip1
cargo build --release
mkdir -p ~/.config/zellij/plugins
install -m 644 target/wasm32-wasip1/release/zellij-new-tab-next-to-current.wasm \
    ~/.config/zellij/plugins/
```

## Configuration

Add to your `~/.config/zellij/config.kdl`:

```kdl
// Pre-load the plugin at startup (runs in background, no visible pane)
load_plugins {
    "file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm"
}

keybinds {
    shared_except "locked" {
        bind "Alt n" {
            MessagePlugin "file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm" {
                name "new-tab-right"
                launch_new false
            }
        }
    }
}
```

Restart Zellij. On first launch, Zellij will ask you to grant plugin permissions — click **Grant**.

### CWD Inheritance

The KDL keybinding creates the new tab with Zellij's session default working directory. If you want the new tab to **inherit the current pane's working directory**, add this shell function to your `~/.bashrc` or `~/.zshrc`:

```bash
new-tab-right() {
    zellij pipe \
        --plugin 'file:~/.config/zellij/plugins/zellij-new-tab-next-to-current.wasm' \
        --name 'new-tab-right' \
        -- "$PWD"
}
```

Then type `new-tab-right` in your shell (or bind it to a terminal key).

## How It Works

The plugin uses a 3-state state machine:

1. **Idle** — waiting for trigger
2. **WaitingForNewTab** — `new_tab()` called, waiting for `TabUpdate` event to detect the new tab's position
3. **MovingTab** — issuing `zellij action move-tab left` commands (via `run_command`) to reposition the tab

Since the Zellij plugin API doesn't expose a `move_tab()` function, the plugin shells out to the Zellij CLI to move tabs. Each move waits for confirmation before issuing the next.

## Requirements

- Zellij 0.43+
- Rust (stable) with `wasm32-wasip1` target

## License

[MIT](LICENSE)
