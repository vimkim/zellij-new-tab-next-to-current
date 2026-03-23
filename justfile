default_target := "wasm32-wasip1"
plugin_name := "zellij-new-tab-next-to-current"
install_dir := env("HOME") / ".config/zellij/plugins"

# Build the plugin in release mode
build:
    cargo build --release

# Build in debug mode
build-debug:
    cargo build

# Install the plugin to ~/.config/zellij/plugins/
install: build
    mkdir -p {{ install_dir }}
    install -m 644 target/{{ default_target }}/release/{{ plugin_name }}.wasm {{ install_dir }}/{{ plugin_name }}.wasm
    @echo "Installed to {{ install_dir }}/{{ plugin_name }}.wasm"
    @echo "Restart Zellij to load the updated plugin."

# Build and install in one step
bi: install

# Clean build artifacts
clean:
    cargo clean

# Check that the project compiles without building
check:
    cargo check

# Show the installed plugin info
info:
    @echo "Plugin: {{ plugin_name }}"
    @ls -lh {{ install_dir }}/{{ plugin_name }}.wasm 2>/dev/null || echo "Not installed yet. Run: just install"

# Set up the wasm target if not already installed
setup:
    rustup target add {{ default_target }}

# Full setup: install target + build + install plugin
init: setup install
