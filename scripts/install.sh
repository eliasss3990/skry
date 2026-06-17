#!/usr/bin/env bash
# Instalador de skry para Linux/macOS. Descarga el binario del último release y
# lo deja en el PATH para invocarlo desde cualquier carpeta.
#
#   curl -sSL https://raw.githubusercontent.com/eliasss3990/skry/main/scripts/install.sh | bash
#
# Variables opcionales:
#   SKRY_VERSION=v1.2.3   instala una versión puntual (default: latest)
#   SKRY_INSTALL_DIR=...   directorio destino (default: /usr/local/bin o ~/.local/bin)
set -euo pipefail

REPO="eliasss3990/skry"
BIN="skry"

err() { echo "[install] error: $*" >&2; exit 1; }
info() { echo "[install] $*"; }

# --- Detección de plataforma --------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux)  os_target="unknown-linux-gnu" ;;
    Darwin) os_target="apple-darwin" ;;
    *)      err "SO no soportado: $os (sólo Linux y macOS)" ;;
esac
case "$arch" in
    x86_64|amd64)   arch_target="x86_64" ;;
    aarch64|arm64)  arch_target="aarch64" ;;
    *)              err "arquitectura no soportada: $arch" ;;
esac
target="${arch_target}-${os_target}"
asset="${BIN}-${target}"

# --- Resolver versión ---------------------------------------------------------
version="${SKRY_VERSION:-latest}"
if [ "$version" = "latest" ]; then
    base="https://github.com/${REPO}/releases/latest/download"
else
    base="https://github.com/${REPO}/releases/download/${version}"
fi
url="${base}/${asset}"

# --- Directorio de instalación ------------------------------------------------
if [ -n "${SKRY_INSTALL_DIR:-}" ]; then
    install_dir="$SKRY_INSTALL_DIR"
elif [ -w /usr/local/bin ] 2>/dev/null; then
    install_dir="/usr/local/bin"
elif command -v sudo >/dev/null 2>&1 && [ "$os" = "Linux" ]; then
    install_dir="/usr/local/bin"
    use_sudo=1
else
    install_dir="$HOME/.local/bin"
fi
mkdir -p "$install_dir" 2>/dev/null || true

# --- Descargar e instalar -----------------------------------------------------
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
info "descargando $asset ($version)..."
if ! curl -fsSL "$url" -o "$tmp"; then
    err "no se pudo descargar $url (¿existe ya un release para $target?)"
fi
chmod +x "$tmp"

dest="${install_dir}/${BIN}"
if [ "${use_sudo:-0}" = "1" ]; then
    sudo mv "$tmp" "$dest"
else
    mv "$tmp" "$dest" 2>/dev/null || err "sin permisos de escritura en $install_dir (probá SKRY_INSTALL_DIR=~/.local/bin)"
fi
trap - EXIT

info "instalado en $dest"
case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) info "OJO: $install_dir no está en tu PATH. Agregalo a tu shell rc:"
       info "  export PATH=\"$install_dir:\$PATH\"" ;;
esac
info "listo. Probá: $BIN --help"
