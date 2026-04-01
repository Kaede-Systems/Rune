#!/usr/bin/env sh
set -eu

usage() {
    cat <<'EOF'
Usage:
  install-unix.sh [./rune] [--prefix DIR] [--shellrc FILE]
  install-unix.sh [--repo OWNER/REPO] [--version latest|TAG] [--prefix DIR] [--shellrc FILE]

Modes:
  1. Local install: pass a built Rune binary path.
  2. Release install: omit the binary path and the installer will download the
     matching Rune release bundle for the current host.

Examples:
  ./install-unix.sh ./rune
  ./install-unix.sh --repo Kaede-Systems/Rune
  ./install-unix.sh --repo Kaede-Systems/Rune --version v0.1.0
EOF
}

PREFIX="${HOME}/.local"
SHELLRC=""
REPO="Kaede-Systems/Rune"
VERSION="latest"
SOURCE_BINARY=""
LLVM_VERSION="21.1.7"
WASMTIME_VERSION="43.0.0"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX=$2
            shift 2
            ;;
        --shellrc)
            SHELLRC=$2
            shift 2
            ;;
        --repo)
            REPO=$2
            shift 2
            ;;
        --version)
            VERSION=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            if [ -z "$SOURCE_BINARY" ] && [ -f "$1" ]; then
                SOURCE_BINARY=$1
                shift
            else
                printf 'unknown argument: %s\n' "$1" >&2
                exit 1
            fi
            ;;
    esac
done

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

detect_asset_name() {
    uname_s=$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')
    uname_m=$(uname -m 2>/dev/null)

    case "${uname_s}:${uname_m}" in
        linux:x86_64|linux:amd64)
            printf 'rune-bundle-linux-x64.tar.gz\n'
            ;;
        linux:aarch64|linux:arm64)
            printf 'rune-bundle-linux-arm64.tar.gz\n'
            ;;
        darwin:x86_64)
            printf 'rune-bundle-macos-x64.tar.gz\n'
            ;;
        darwin:arm64|darwin:aarch64)
            printf 'rune-bundle-macos-arm64.tar.gz\n'
            ;;
        *)
            printf 'unsupported host: %s %s\n' "$uname_s" "$uname_m" >&2
            exit 1
            ;;
    esac
}

detect_tool_urls() {
    uname_s=$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')
    uname_m=$(uname -m 2>/dev/null)

    case "${uname_s}:${uname_m}" in
        linux:x86_64|linux:amd64)
            printf 'linux-x64\n'
            printf 'https://github.com/llvm/llvm-project/releases/download/llvmorg-%s/LLVM-%s-Linux-X64.tar.xz\n' "$LLVM_VERSION" "$LLVM_VERSION"
            printf 'https://github.com/bytecodealliance/wasmtime/releases/download/v%s/wasmtime-v%s-x86_64-linux.tar.xz\n' "$WASMTIME_VERSION" "$WASMTIME_VERSION"
            ;;
        linux:aarch64|linux:arm64)
            printf 'linux-arm64\n'
            printf 'https://github.com/llvm/llvm-project/releases/download/llvmorg-%s/LLVM-%s-Linux-ARM64.tar.xz\n' "$LLVM_VERSION" "$LLVM_VERSION"
            printf 'https://github.com/bytecodealliance/wasmtime/releases/download/v%s/wasmtime-v%s-aarch64-linux.tar.xz\n' "$WASMTIME_VERSION" "$WASMTIME_VERSION"
            ;;
        darwin:x86_64)
            printf 'macos-x64\n'
            printf 'https://github.com/llvm/llvm-project/releases/download/llvmorg-%s/LLVM-%s-macOS-X64.tar.xz\n' "$LLVM_VERSION" "$LLVM_VERSION"
            printf 'https://github.com/bytecodealliance/wasmtime/releases/download/v%s/wasmtime-v%s-x86_64-macos.tar.xz\n' "$WASMTIME_VERSION" "$WASMTIME_VERSION"
            ;;
        darwin:arm64|darwin:aarch64)
            printf 'macos-arm64\n'
            printf 'https://github.com/llvm/llvm-project/releases/download/llvmorg-%s/LLVM-%s-macOS-ARM64.tar.xz\n' "$LLVM_VERSION" "$LLVM_VERSION"
            printf 'https://github.com/bytecodealliance/wasmtime/releases/download/v%s/wasmtime-v%s-aarch64-macos.tar.xz\n' "$WASMTIME_VERSION" "$WASMTIME_VERSION"
            ;;
        *)
            printf 'unsupported\n\n\n'
            ;;
    esac
}

download_release_bundle() {
    require_cmd curl
    require_cmd tar

    asset_name=$(detect_asset_name)
    temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/rune-install.XXXXXX")
    archive_path="${temp_dir}/${asset_name}"

    if [ "$VERSION" = "latest" ]; then
        url="https://github.com/${REPO}/releases/latest/download/${asset_name}"
    else
        url="https://github.com/${REPO}/releases/download/${VERSION}/${asset_name}"
    fi

    printf 'Downloading %s\n' "$url"
    curl -fL "$url" -o "$archive_path"

    extract_dir="${temp_dir}/extract"
    mkdir -p "$extract_dir"
    tar -xzf "$archive_path" -C "$extract_dir"

    root="$extract_dir"
    entry_count=$(find "$extract_dir" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')
    if [ "$entry_count" = "1" ]; then
        only_entry=$(find "$extract_dir" -mindepth 1 -maxdepth 1)
        if [ -d "$only_entry" ]; then
            root="$only_entry"
        fi
    fi

    printf '%s\n' "$root"
}

install_tree() {
    source_root=$1
    dest_root=$2
    bin_dir="${dest_root}/bin"
    share_dir="${dest_root}/share/rune"

    mkdir -p "$bin_dir" "$share_dir"

    if [ ! -f "${source_root}/bin/rune" ]; then
        printf 'release bundle is missing bin/rune\n' >&2
        exit 1
    fi

    cp "${source_root}/bin/rune" "${bin_dir}/rune"
    chmod 755 "${bin_dir}/rune"

    if [ -d "${source_root}/share/rune" ]; then
        rm -rf "${share_dir}"
        mkdir -p "${share_dir}"
        cp -R "${source_root}/share/rune/." "${share_dir}"
    fi
}

ensure_host_tools() {
    dest_root=$1
    require_cmd curl
    require_cmd tar

    info=$(detect_tool_urls)
    host_bundle=$(printf '%s' "$info" | sed -n '1p')
    llvm_url=$(printf '%s' "$info" | sed -n '2p')
    wasmtime_url=$(printf '%s' "$info" | sed -n '3p')

    if [ "$host_bundle" = "unsupported" ]; then
        printf 'unsupported host for packaged tool downloads\n' >&2
        exit 1
    fi

    tools_root="${dest_root}/share/rune/tools"
    llvm_dest="${tools_root}/llvm21/${host_bundle}"
    wasmtime_dest="${tools_root}/wasmtime/extract/${host_bundle}"
    mkdir -p "${tools_root}/llvm21" "${tools_root}/wasmtime/extract"

    temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/rune-tools.XXXXXX")

    if [ ! -d "$llvm_dest" ] || [ -z "$(find "$llvm_dest" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
        llvm_archive="${temp_dir}/llvm.tar.xz"
        printf 'Downloading LLVM toolchain for %s\n' "$host_bundle"
        curl -fL "$llvm_url" -o "$llvm_archive"
        mkdir -p "$llvm_dest"
        tar -xf "$llvm_archive" -C "$llvm_dest"
    fi

    if [ ! -d "$wasmtime_dest" ] || [ -z "$(find "$wasmtime_dest" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
        wasmtime_archive="${temp_dir}/wasmtime.tar.xz"
        printf 'Downloading Wasmtime for %s\n' "$host_bundle"
        curl -fL "$wasmtime_url" -o "$wasmtime_archive"
        mkdir -p "$wasmtime_dest"
        tar -xf "$wasmtime_archive" -C "$wasmtime_dest"
    fi
}

if [ -n "$SOURCE_BINARY" ]; then
    if [ ! -f "$SOURCE_BINARY" ]; then
        printf 'rune binary not found: %s\n' "$SOURCE_BINARY" >&2
        exit 1
    fi
    temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/rune-install.XXXXXX")
    mkdir -p "${temp_dir}/bundle/bin" "${temp_dir}/bundle/share/rune"
    cp "$SOURCE_BINARY" "${temp_dir}/bundle/bin/rune"
    chmod 755 "${temp_dir}/bundle/bin/rune"
    if [ -d "./tools" ]; then
        mkdir -p "${temp_dir}/bundle/share/rune/tools"
        cp -R "./tools/." "${temp_dir}/bundle/share/rune/tools"
    fi
    BUNDLE_ROOT="${temp_dir}/bundle"
else
    BUNDLE_ROOT=$(download_release_bundle)
fi

install_tree "$BUNDLE_ROOT" "$PREFIX"
ensure_host_tools "$PREFIX"

if [ -z "$SHELLRC" ]; then
    if [ -n "${ZDOTDIR:-}" ] && [ -f "${ZDOTDIR}/.zshrc" ]; then
        SHELLRC="${ZDOTDIR}/.zshrc"
    elif [ -f "${HOME}/.zshrc" ]; then
        SHELLRC="${HOME}/.zshrc"
    else
        SHELLRC="${HOME}/.bashrc"
    fi
fi

PATH_LINE="export PATH=\"${PREFIX}/bin:\$PATH\""
if [ -f "$SHELLRC" ]; then
    if ! grep -F "$PATH_LINE" "$SHELLRC" >/dev/null 2>&1; then
        printf '\n# Rune CLI\n%s\n' "$PATH_LINE" >> "$SHELLRC"
    fi
else
    printf '# Rune CLI\n%s\n' "$PATH_LINE" > "$SHELLRC"
fi

printf 'Installed Rune to %s/bin/rune\n' "$PREFIX"
if [ -d "${PREFIX}/share/rune/tools" ]; then
    printf 'Installed Rune shared assets to %s/share/rune\n' "$PREFIX"
fi
printf 'Added %s/bin to PATH in %s\n' "$PREFIX" "$SHELLRC"
