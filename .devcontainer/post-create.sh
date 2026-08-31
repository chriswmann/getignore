#!/usr/bin/env bash
set -euxo pipefail

# --- build/runtime deps LazyVim wants (treesitter compiler, telescope tools) ---
sudo apt-get update
sudo apt-get install -y --no-install-recommends build-essential ripgrep fd-find unzip

# --- Rust nightly ---
# The rust feature rejects a bare `nightly` (it only takes `latest`, `none`, a
# released version, or a dated `nightly-YYYY-MM-DD`), so it installs stable and
# we roll forward to nightly here — undated, so it stays current.
# The feature chowns CARGO_HOME/RUSTUP_HOME to this user, so no sudo needed, and
# puts /usr/local/cargo/bin on PATH via containerEnv, so rustup is already here.
rustup toolchain install nightly --profile default
rustup default nightly

# --- Neovim: the apt version is far older than LazyVim's >= 0.11.2 floor ---
case "$(uname -m)" in
  x86_64)  NVIM_ARCH=linux-x86_64 ;;
  aarch64) NVIM_ARCH=linux-arm64  ;;
  *) echo "unsupported arch: $(uname -m)"; exit 1 ;;
esac
curl -fsSL "https://github.com/neovim/neovim/releases/download/stable/nvim-${NVIM_ARCH}.tar.gz" \
  | sudo tar -xz -C /opt
sudo ln -sfn "/opt/nvim-${NVIM_ARCH}" /opt/nvim
# /usr/local/bin is on the default PATH of a plain `gh codespace ssh` shell, which
# never sees devcontainer.json's remoteEnv (that is applied by the VS Code server).
sudo ln -sfn /opt/nvim/bin/nvim /usr/local/bin/nvim
export PATH="/opt/nvim/bin:$PATH"

# --- LazyVim starter ---
if [ ! -e "$HOME/.config/nvim/init.lua" ]; then
  git clone --depth 1 https://github.com/LazyVim/starter "$HOME/.config/nvim"
  rm -rf "$HOME/.config/nvim/.git"   # so it's yours, not a clone of the starter
fi
# Pre-install plugins now, so the first `nvim` opens instantly instead of
# bootstrapping lazy.nvim + treesitter parsers interactively.
nvim --headless "+Lazy! sync" +qa || true

# --- Atuin ---
# ATUIN_NON_INTERACTIVE skips the "import your history?" prompt, which reads
# from /dev/tty and has nothing to read during postCreate.
# The installer appends `atuin init` (plus bash-preexec) to ~/.bashrc and ~/.zshrc.
# History stays local to the codespace; no `atuin login` / cloud sync here.
export ATUIN_NON_INTERACTIVE=yes
curl --proto '=https' --tlsv1.2 -sSf https://setup.atuin.sh | bash
