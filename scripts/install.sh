#!/bin/sh
# Installe talixio-agent (Linux, macOS) : télécharge l'exécutable de ce système, vérifie son
# empreinte, le pose dans un dossier de l'utilisateur. Rien n'est lancé, rien ne demande root.
#   TALIXIO_VERSION=0.10.0   une version précise (par défaut : la dernière)
#   TALIXIO_INSTALL_DIR=…    où le poser (par défaut : ~/.local/bin)
set -eu

main() {
    repo="https://github.com/fredolier/talixio-ai-agent-network"
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) echo "Système non pris en charge : $(uname -s). Sous Windows : install.ps1." >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        x86_64 | amd64) cpu=x86_64 ;;
        aarch64 | arm64) cpu=aarch64 ;;
        *) echo "Processeur non pris en charge : $(uname -m)." >&2; exit 1 ;;
    esac
    name="talixio-agent-$os-$cpu"
    version="${TALIXIO_VERSION:-}"
    case "$version" in
        "") base="$repo/releases/latest/download" ;;
        *[!0-9.]*) echo "Version invalide : $version (attendu : 0.10.0)." >&2; exit 1 ;;
        *) base="$repo/releases/download/v$version" ;;
    esac
    dir="${TALIXIO_INSTALL_DIR:-$HOME/.local/bin}"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    echo "Téléchargement de $name…"
    curl --proto '=https' --tlsv1.2 -fsSL -o "$tmp/$name" "$base/$name"
    curl --proto '=https' --tlsv1.2 -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
    wanted="$(awk -v n="$name" '$2 == n { print $1 }' "$tmp/SHA256SUMS")"
    if command -v sha256sum >/dev/null 2>&1; then
        got="$(sha256sum "$tmp/$name" | cut -d' ' -f1)"
    else
        got="$(shasum -a 256 "$tmp/$name" | cut -d' ' -f1)"
    fi
    if [ -z "$wanted" ] || [ "$wanted" != "$got" ]; then
        echo "Empreinte inattendue pour $name : rien n'est installé." >&2
        exit 1
    fi

    mkdir -p "$dir"
    chmod 755 "$tmp/$name"
    mv "$tmp/$name" "$dir/talixio-agent"
    echo "Installé : $dir/talixio-agent"
    case ":$PATH:" in
        *":$dir:"*) ;;
        *) echo "Ajoutez $dir à votre PATH pour l'appeler par son nom." ;;
    esac
    echo "Ensuite : écrivez talixio.toml (modèle : $base/talixio.example.toml), puis"
    echo "  talixio-agent root --config talixio.toml"
}

main "$@"
