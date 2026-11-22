#!/usr/bin/env bash
# Construire et lancer le maître dans Docker, sans retenir les commandes.
#
#   ./tools.sh up            construit l'image vide et lance le maître
#   ./tools.sh up claude     pareil, avec la commande `claude` (abonnement Claude Pro ou Max)
#   ./tools.sh login         connecte `claude` à votre abonnement, une fois (lien à ouvrir, code à recoller)
#   ./tools.sh build [claude]   construit seulement
#   ./tools.sh run [claude]     lance seulement (l'image doit exister)
#   ./tools.sh build navigateur   construit l'image des AGENTS qui lisent le web avec un navigateur
#                                 (talixio-navigateur ; pas une image du maître : voir examples/catalogue)
#   ./tools.sh token         affiche le jeton de l'interface
#   ./tools.sh logs          suit le journal du maître
#   ./tools.sh status        dit si le maître tourne, et où
#   ./tools.sh stop          arrête et retire le conteneur (les données restent)
#   ./tools.sh purge         efface aussi les données (demande confirmation)
#
# Réglages, par l'environnement :
#   TALIXIO_PORT     port sur cette machine (7700)
#   TALIXIO_BIND     adresse d'écoute sur cette machine (127.0.0.1 : cette machine seule)
#   TALIXIO_CONFIG   votre talixio.toml, monté à la place de docker/talixio.toml
#   TALIXIO_TOOLS    un dossier de scripts pour les agents, monté en lecture seule sur /outils
#   TALIXIO_NAME     nom du conteneur (talixio) ; le volume de données est <nom>-data
#   TALIXIO_DOCKER=1 le maître peut lancer ses agents dans d'autres conteneurs (image « claude ») :
#                    il reçoit le socket Docker de cette machine. ATTENTION : qui prend le maître
#                    commande alors Docker, donc cette machine. Voir docs/SPEC.md, décision 36.
#   TALIXIO_BUILD_ARGS   options de plus pour « docker build » (derrière un mandataire, par exemple)
#   CLAUDE_CODE_OAUTH_TOKEN   facultatif, pour « claude » : le jeton tiré par `claude setup-token`,
#                             à la place de « ./tools.sh login »
#
# Le jeton de l'interface est tiré au hasard une fois et gardé hors du dépôt, fermé aux autres
# comptes. La connexion à l'abonnement est faite par `claude` lui-même, qui la garde dans le volume
# de données, hors de portée des agents ; ce script ne voit ni ne garde aucun jeton d'abonnement.

set -euo pipefail
cd "$(dirname "$0")"

NAME="${TALIXIO_NAME:-talixio}"
PORT="${TALIXIO_PORT:-7700}"
BIND="${TALIXIO_BIND:-127.0.0.1}"
VOLUME="$NAME-data"
TOKEN_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/talixio/$NAME-api-token"

die() { echo "tools.sh : $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker n'est pas installé"

# « claude » ou rien : quelle image
variant() {
    case "${1:-}" in
        "") IMAGE="talixio-agent"; DOCKERFILE="Dockerfile"; PIDS=256; MEMORY=1g ;;
        claude) IMAGE="talixio-agent:claude"; DOCKERFILE="docker/Dockerfile.claude"; PIDS=512; MEMORY=3g ;;
        *) die "image inconnue : « $1 » (rien, ou « claude »)" ;;
    esac
}

# Le jeton de l'interface : celui de l'environnement s'il y en a un, sinon le nôtre, tiré une fois.
api_token() {
    if [ -n "${TALIXIO_API_TOKEN:-}" ]; then
        return
    fi
    if [ ! -s "$TOKEN_FILE" ]; then
        mkdir -p "$(dirname "$TOKEN_FILE")"
        (umask 077; od -An -N32 -tx1 /dev/urandom | tr -d ' \n' > "$TOKEN_FILE")
    fi
    chmod 600 "$TOKEN_FILE"
    TALIXIO_API_TOKEN="$(cat "$TOKEN_FILE")"
    [ "${#TALIXIO_API_TOKEN}" -eq 64 ] || die "jeton illisible dans $TOKEN_FILE : effacez ce fichier"
}

build() {
    if [ "${1:-}" = navigateur ]; then
        # l'image des agents à navigateur : l'agent (celui de l'image « claude », même version que
        # le maître), Chromium, et les scripts du dossier d'outils ; aucune clé
        build claude
        local tools="${TALIXIO_TOOLS:-examples/catalogue/outils}"
        [ -d "$tools" ] || die "dossier d'outils introuvable : $tools"
        # shellcheck disable=SC2086
        docker build ${TALIXIO_BUILD_ARGS:-} -f docker/Dockerfile.navigateur --build-context "outils=$tools" -t talixio-navigateur docker
        echo "Image talixio-navigateur construite : à donner à une cible « docker » du maître (examples/catalogue)."
        return
    fi
    variant "${1:-}"
    [ -f web/dist/index.html ] || die "web/dist manque : l'interface construite fait partie du dépôt"
    # shellcheck disable=SC2086  # TALIXIO_BUILD_ARGS : options de plus pour docker build (réseau, mandataire)
    docker build ${TALIXIO_BUILD_ARGS:-} -f "$DOCKERFILE" -t "$IMAGE" .
}

run() {
    variant "${1:-}"
    docker image inspect "$IMAGE" >/dev/null 2>&1 || die "l'image $IMAGE n'existe pas : ./tools.sh build ${1:-}"
    api_token
    export TALIXIO_API_TOKEN
    # les secrets passent par leur nom : leur valeur n'apparaît pas sur la ligne de commande
    local env=(-e TALIXIO_API_TOKEN)
    if [ "${1:-}" = claude ] && [ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ]; then
        env+=(-e CLAUDE_CODE_OAUTH_TOKEN)
    fi
    local config=()
    if [ -n "${TALIXIO_CONFIG:-}" ]; then
        [ -f "$TALIXIO_CONFIG" ] || die "configuration introuvable : $TALIXIO_CONFIG"
        config=(-v "$(cd "$(dirname "$TALIXIO_CONFIG")" && pwd)/$(basename "$TALIXIO_CONFIG"):/config/talixio.toml:ro")
    fi
    if [ -n "${TALIXIO_TOOLS:-}" ]; then
        [ -d "$TALIXIO_TOOLS" ] || die "dossier d'outils introuvable : $TALIXIO_TOOLS"
        # « : » sépare les champs d'un montage : un chemin qui en porte monterait autre chose
        case "$(cd "$TALIXIO_TOOLS" && pwd)" in *:*) die "dossier d'outils refusé : « : » dans son chemin" ;; esac
        # en lecture seule : un agent ne doit pas pouvoir réécrire ce qu'il a le droit de lancer
        config+=(-v "$(cd "$TALIXIO_TOOLS" && pwd):/outils:ro")
    fi
    if [ "$BIND" != 127.0.0.1 ] && [ "$BIND" != localhost ]; then
        echo "Attention : le maître sera joignable depuis $BIND, pas seulement depuis cette machine." >&2
    fi
    local siblings=()
    if [ "${TALIXIO_DOCKER:-}" = 1 ]; then
        [ "${1:-}" = claude ] || die "TALIXIO_DOCKER=1 demande l'image « claude » (l'image ordinaire n'a pas de client Docker)"
        local socket="${TALIXIO_DOCKER_SOCKET:-/var/run/docker.sock}"
        [ -S "$socket" ] || die "socket Docker introuvable : $socket (TALIXIO_DOCKER_SOCKET pour un autre)"
        case "$socket" in *:*) die "socket Docker refusé : « : » dans son chemin" ;; esac
        # un réseau à part : le maître et ses agents s'y joignent, sans passer par la machine
        docker network inspect "$NAME-net" >/dev/null 2>&1 || docker network create "$NAME-net" >/dev/null
        # le compte sans droit du maître n'ouvre le socket que par le groupe qui le détient
        siblings=(--network "$NAME-net" -v "$socket:/var/run/docker.sock" --group-add "$(stat -c %g "$socket")")
        echo "Attention : le maître reçoit le socket Docker ($socket). Qui prend le maître commande Docker, donc cette machine." >&2
    fi
    # un seul maître de ce nom : l'ancien conteneur est remplacé, ses données restent dans le volume ;
    # les conteneurs d'agents de l'ancien partent avec lui (ils n'auraient plus de parent)
    agents_down
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" --restart unless-stopped \
        -p "$BIND:$PORT:7700" -v "$VOLUME:/data" \
        --cap-drop ALL --security-opt no-new-privileges --pids-limit "$PIDS" --memory "$MEMORY" \
        "${env[@]}" ${config[@]+"${config[@]}"} ${siblings[@]+"${siblings[@]}"} "$IMAGE" >/dev/null
    echo "Le maître tourne : https://$BIND:$PORT"
    echo "(certificat fabriqué par le maître : le navigateur demande confirmation)"
    echo "Jeton de l'interface : ./tools.sh token"
    if [ "${1:-}" = claude ]; then
        [ -n "${CLAUDE_CODE_OAUTH_TOKEN:-}" ] || echo "Connectez votre abonnement, une fois : ./tools.sh login"
        echo "Puis « Fournisseurs », « Claude (abonnement) », « Connecter »."
    fi
}

# Les conteneurs d'agents que ce maître a lancés (TALIXIO_DOCKER=1) : ceux qui sont dans son
# réseau ou qui tiennent son volume. Jamais un conteneur d'un autre nom que « talixio-a-… ».
agents_down() {
    {
        docker ps -aq --filter "network=$NAME-net" --filter "name=^talixio-a-" 2>/dev/null || true
        docker ps -aq --filter "volume=$VOLUME" --filter "name=^talixio-a-" 2>/dev/null || true
    } | sort -u | while read -r id; do docker rm -f "$id" >/dev/null 2>&1 || true; done
}

case "${1:-}" in
    build) build "${2:-}" ;;
    run) run "${2:-}" ;;
    up) build "${2:-}"; run "${2:-}" ;;
    token) api_token; echo "$TALIXIO_API_TOKEN" ;;
    login) docker exec -it "$NAME" claude auth login ;;
    logs) docker logs -f "$NAME" ;;
    status) docker ps -a --filter "name=^$NAME\$" --format '{{.Names}} : {{.Status}} ({{.Image}}, {{.Ports}})' | grep . || echo "$NAME : pas de conteneur" ;;
    stop) agents_down; docker rm -f "$NAME" >/dev/null 2>&1 && echo "$NAME arrêté ; ses données restent dans le volume $VOLUME" || echo "$NAME ne tournait pas" ;;
    purge)
        printf "Effacer le conteneur %s ET ses données (volume %s : journal, mémoire, clés saisies) ? [oui/non] " "$NAME" "$VOLUME"
        read -r answer
        [ "$answer" = oui ] || die "rien n'a été effacé"
        agents_down
        docker rm -f "$NAME" >/dev/null 2>&1 || true
        docker network rm "$NAME-net" >/dev/null 2>&1 || true
        docker volume rm "$VOLUME" >/dev/null 2>&1 || true
        rm -f "$TOKEN_FILE"
        # ne pas annoncer ce qui n'est pas fait : un conteneur qui tient encore le volume l'empêche
        if docker volume inspect "$VOLUME" >/dev/null 2>&1; then
            die "le volume $VOLUME n'a pas pu être effacé (un conteneur le tient encore : docker ps -a --filter volume=$VOLUME)"
        fi
        if docker network inspect "$NAME-net" >/dev/null 2>&1; then
            echo "Effacé, sauf le réseau $NAME-net, encore employé (docker network inspect $NAME-net)."
        else
            echo "Effacé."
        fi
        ;;
    *) sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
