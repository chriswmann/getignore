#!/usr/bin/env bash
set -euxo pipefail

# --- Resolving "the latest release" without the REST API ---
# api.github.com allows unauthenticated callers 60 requests/hour per IP, and a
# codespace shares its egress IP with every other codespace on the host, so
# /repos/<repo>/releases/latest intermittently answers 403. Under `set -e` that
# aborted this script partway through, leaving jj, atuin and carapace
# uninstalled. There is no token to authenticate with here (Codespaces does
# not put GITHUB_TOKEN in the container env and gh is not in this image), so use
# github.com's redirect from /releases/latest to /releases/tag/<tag> instead: it
# carries the same answer and is not subject to the API rate limit.
latest_tag() {
  local url
  url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$1/releases/latest")
  printf '%s\n' "${url##*/}"
}

# --- build/runtime deps LazyVim wants (treesitter compiler, telescope tools) ---
# kitty-terminfo carries /usr/share/terminfo/x/xterm-kitty (it is not in
# ncurses-term). `gh codespace ssh` passes the local TERM straight through, so
# without it less warns "terminal is not fully functional" and waits on RETURN
# every time jj, git or man pages something.
sudo apt-get update
sudo apt-get install -y --no-install-recommends build-essential ripgrep fd-find unzip kitty-terminfo

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
x86_64) NVIM_ARCH=linux-x86_64 ;;
aarch64) NVIM_ARCH=linux-arm64 ;;
*)
  echo "unsupported arch: $(uname -m)"
  exit 1
  ;;
esac
curl -fsSL "https://github.com/neovim/neovim/releases/download/stable/nvim-${NVIM_ARCH}.tar.gz" |
  sudo tar -xz -C /opt
sudo ln -sfn "/opt/nvim-${NVIM_ARCH}" /opt/nvim
# /usr/local/bin is on the default PATH of a plain `gh codespace ssh` shell, which
# never sees devcontainer.json's remoteEnv (that is applied by the VS Code server).
sudo ln -sfn /opt/nvim/bin/nvim /usr/local/bin/nvim
export PATH="/opt/nvim/bin:$PATH"

# --- LazyVim starter ---
if [ ! -e "$HOME/.config/nvim/init.lua" ]; then
  git clone --depth 1 https://github.com/LazyVim/starter "$HOME/.config/nvim"
  rm -rf "$HOME/.config/nvim/.git" # so it's yours, not a clone of the starter
fi
# Pre-install plugins now, so the first `nvim` opens instantly instead of
# bootstrapping lazy.nvim + treesitter parsers interactively.
nvim --headless "+Lazy! sync" +qa || true

# --- Jujutsu (jj) ---
# There is no apt package, so take the statically linked musl build from the
# latest GitHub release. The release assets carry the version in their names, so
# /releases/latest/download/<asset> cannot be used; resolve the tag first. Like
# the nightly toolchain and nvim stable above, this is undated: a rebuilt
# container gets whatever is current.
case "$(uname -m)" in
x86_64) JJ_TARGET=x86_64-unknown-linux-musl ;;
aarch64) JJ_TARGET=aarch64-unknown-linux-musl ;;
*)
  echo "unsupported arch: $(uname -m)"
  exit 1
  ;;
esac
JJ_TAG=$(latest_tag jj-vcs/jj)
# The tarball is flat (./jj alongside ./LICENSE, ./README.md), so pull out just
# the binary, into /usr/local/bin for the same PATH reason as nvim above.
curl -fsSL "https://github.com/jj-vcs/jj/releases/download/${JJ_TAG}/jj-${JJ_TAG}-${JJ_TARGET}.tar.gz" |
  sudo tar -xz -C /usr/local/bin ./jj

# jj reads its own config, never ~/.gitconfig, so commits would be authored by
# "(no name configured)". Seed it from git's identity, which Codespaces fills in
# from the GitHub account.
JJ_NAME=$(git config --get user.name || true)
JJ_EMAIL=$(git config --get user.email || true)
if [ -n "$JJ_NAME" ]; then
  jj config set --user user.name "$JJ_NAME"
fi
if [ -n "$JJ_EMAIL" ]; then
  jj config set --user user.email "$JJ_EMAIL"
fi
# A bare `jj` prints help plus a hint to set this — set it here to prevent that message from being displayed
jj config set --user ui.default-command log

# postCreate runs from the workspace folder with the clone already in place, so
# this is the point at which the repo can be colocated. `jj git init --colocate`
# exits 1 on a repo that already has .jj/, and postCreate re-runs on a rebuild,
# hence the guard. It is non-destructive: git's history is imported as-is and
# any uncommitted work becomes the new working-copy commit.
if git rev-parse --is-inside-work-tree >/dev/null 2>&1 && [ ! -d .jj ]; then
  jj git init --colocate
fi

# --- Atuin ---
# ATUIN_NON_INTERACTIVE skips the "import your history?" prompt, which reads
# from /dev/tty and has nothing to read during postCreate.
# The installer appends `atuin init` (plus bash-preexec) to ~/.bashrc and ~/.zshrc.
# History stays local to the codespace; no `atuin login` / cloud sync here.
export ATUIN_NON_INTERACTIVE=yes
curl --proto '=https' --tlsv1.2 -sSf https://setup.atuin.sh | bash

# --- Carapace (multi-shell completions) ---
# Release assets drop the tag's leading "v" and use Go-style arch names, so the
# tag has to be resolved and rewritten rather than used verbatim.
case "$(uname -m)" in
x86_64) CARAPACE_ARCH=amd64 ;;
aarch64) CARAPACE_ARCH=arm64 ;;
*)
  echo "unsupported arch: $(uname -m)"
  exit 1
  ;;
esac
CARAPACE_TAG=$(latest_tag carapace-sh/carapace-bin)
CARAPACE_VERSION=${CARAPACE_TAG#v}
# Flat tarball (carapace beside LICENSE, README.md); /usr/local/bin again so a
# plain `gh codespace ssh` shell finds it.
curl -fsSL "https://github.com/carapace-sh/carapace-bin/releases/download/${CARAPACE_TAG}/carapace-bin_${CARAPACE_VERSION}_linux_${CARAPACE_ARCH}.tar.gz" |
  sudo tar -xz -C /usr/local/bin carapace

# --- zsh-vi-mode (vi keybindings on the command line) ---
# The oh-my-zsh install here only ships the bundled `vi-mode` plugin; this is
# jeffreytse/zsh-vi-mode, which is what the local machine uses — proper mode
# tracking, a cursor that changes shape, surround and text objects.
ZVM_DIR="${ZSH_CUSTOM:-$HOME/.oh-my-zsh/custom}/plugins/zsh-vi-mode"
if [ ! -d "$ZVM_DIR" ]; then
  git clone --depth 1 https://github.com/jeffreytse/zsh-vi-mode "$ZVM_DIR"
fi

# Appended last, after oh-my-zsh's own compinit and atuin's block, because
# carapace must be sourced once the completion system is initialised. Guarded so
# a rebuild (postCreate runs again) cannot duplicate the block.
# Optional: export CARAPACE_BRIDGES='zsh,fish,bash,inshellisense' to fall back to
# other shells' completions for commands carapace has no completer for.
if ! grep -q 'carapace _carapace' "$HOME/.zshrc" 2>/dev/null; then
  # zsh-vi-mode rebinds the whole keymap when it initialises (at the first
  # prompt, after ~/.zshrc has finished), wiping bindings made before it —
  # atuin's ctrl-r among them. So delete the `eval "$(atuin init zsh)"` the
  # atuin installer appended above and re-add it inside zvm_after_init, which
  # runs after that rebind. Same reason the local ~/.zshrc does this.
  sed -i '/atuin init zsh/d' "$HOME/.zshrc"
  cat >>"$HOME/.zshrc" <<'EOF'

# vi keybindings
source "${ZSH_CUSTOM:-$HOME/.oh-my-zsh/custom}/plugins/zsh-vi-mode/zsh-vi-mode.plugin.zsh"

# Anything that binds keys must run here, after zsh-vi-mode's own rebind.
function zvm_after_init() {
  eval "$(atuin init zsh)"
}

# ~/.local/bin, where a user-level installer drops binaries. The image already
# adds it in /etc/zsh/zshrc, but only for interactive shells and only appended,
# so a same-named binary earlier in PATH would win. Prepend it here instead,
# guarded against a second copy.
case ":$PATH:" in
*":$HOME/.local/bin:"*) ;;
*) export PATH="$HOME/.local/bin:$PATH" ;;
esac

# carapace completions
autoload -U compinit && compinit
zstyle ':completion:*' format $'\e[2;37mCompleting %d\e[m'
source <(carapace _carapace)
EOF
fi
if ! grep -q 'carapace _carapace' "$HOME/.bashrc" 2>/dev/null; then
  cat >>"$HOME/.bashrc" <<'EOF'

# ~/.local/bin (see the note in ~/.zshrc)
case ":$PATH:" in
*":$HOME/.local/bin:"*) ;;
*) export PATH="$HOME/.local/bin:$PATH" ;;
esac

# carapace completions
source <(carapace _carapace)
EOF
fi
