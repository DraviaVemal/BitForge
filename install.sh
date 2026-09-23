#!/usr/bin/env sh
set -eu

REPO="${BITFORGE_REPO:-DraviaVemal/BitForge}"
BIN_NAME="BitForge"
INSTALL_DIR="${BITFORGE_INSTALL_DIR:-$HOME/.local/bin}"
USE_BETA=0

for arg in "$@"; do
  case "$arg" in
    --beta) USE_BETA=1 ;;
    --dir=*) INSTALL_DIR="${arg#--dir=}" ;;
    -h | --help)
      echo "usage: install.sh [--beta] [--dir=<path>]"
      exit 0
      ;;
    *)
      echo "unknown option: $arg" >&2
      exit 1
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || {
  echo "error: curl is required" >&2
  exit 1
}

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *)
    echo "error: unsupported architecture: $arch" >&2
    exit 1
    ;;
esac
case "$os" in
  linux) os=linux ;;
  darwin) os=darwin ;;
  *)
    echo "error: unsupported OS: $os" >&2
    exit 1
    ;;
esac
asset="${BIN_NAME}-${os}-${arch}"

api="https://api.github.com/repos/${REPO}/releases"
releases=$(curl -fsSL -H "Accept: application/vnd.github+json" -H "User-Agent: BitForge" "$api") || {
  echo "error: failed to query releases for $REPO" >&2
  exit 1
}

resolve_url() {
  if command -v jq >/dev/null 2>&1; then
    if [ "$USE_BETA" = "1" ]; then
      printf '%s' "$releases" | jq -r --arg a "$asset" '
        [ .[] | select(.draft|not) ] | sort_by(.published_at) | reverse
        | (map(select(.prerelease|not)) + map(select(.prerelease)))[0]
        | .tag_name, (.assets[] | select(.name==$a) | .browser_download_url)'
    else
      printf '%s' "$releases" | jq -r --arg a "$asset" '
        [ .[] | select(.draft|not) | select(.prerelease|not) ] | sort_by(.published_at) | reverse | .[0]
        | .tag_name, (.assets[] | select(.name==$a) | .browser_download_url)'
    fi
  else
    tag=$(printf '%s' "$releases" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')
    url=$(printf '%s' "$releases" | grep -o "https://[^\"]*${asset}" | head -n1)
    printf '%s\n%s\n' "$tag" "$url"
  fi
}

resolved=$(resolve_url)
tag=$(printf '%s' "$resolved" | sed -n '1p')
url=$(printf '%s' "$resolved" | sed -n '2p')

if [ -z "$url" ]; then
  echo "error: no asset '$asset' found in the selected release${tag:+ ($tag)}" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
target="$INSTALL_DIR/$BIN_NAME"
tmp="$(mktemp)"
echo "Downloading BitForge ${tag} ($asset)..."
# -# shows a progress bar; -S still surfaces errors on failure.
curl -fSL --progress-bar -o "$tmp" "$url"
chmod +x "$tmp"
mv "$tmp" "$target"

echo "Installed BitForge to $target"

add_to_path() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return 0 ;;
  esac

  shell_name=$(basename "${SHELL:-/bin/sh}")
  case "$shell_name" in
    bash) rc="$HOME/.bashrc" ;;
    zsh) rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
    ksh) rc="$HOME/.kshrc" ;;
    fish)
      rc="$HOME/.config/fish/config.fish"
      mkdir -p "$(dirname "$rc")"
      line="fish_add_path $INSTALL_DIR"
      ;;
    *) rc="$HOME/.profile" ;;
  esac
  [ -n "${line:-}" ] || line="export PATH=\"$INSTALL_DIR:\$PATH\""

  touch "$rc" 2>/dev/null || {
    echo "note: could not write to $rc; add $INSTALL_DIR to your PATH manually." >&2
    return 0
  }
  if ! grep -qF "$line" "$rc" 2>/dev/null; then
    printf '\n# Added by BitForge installer\n%s\n' "$line" >>"$rc" 2>/dev/null &&
      echo "Added $INSTALL_DIR to PATH in $rc" ||
      echo "note: could not update $rc; add $INSTALL_DIR to your PATH manually." >&2
  fi

  export PATH="$INSTALL_DIR:$PATH"
  echo "note: run 'source $rc' or restart your shell to update PATH in existing sessions."
}

# PATH setup is best-effort; never fail the install over it.
add_to_path || true
