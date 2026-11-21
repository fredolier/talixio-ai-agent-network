# L'agent maître (agent 0) dans l'image la plus légère possible : l'exécutable statique, seul.
# Ni système, ni shell, ni bibliothèque : il n'y a rien d'autre à mettre à jour ni à attaquer.
#
#   docker build -t talixio-agent .
#   docker run -d --name talixio -p 127.0.0.1:7700:7700 -v talixio-data:/data \
#     -e ANTHROPIC_API_KEY=… -e TALIXIO_API_TOKEN=un-jeton-long-de-votre-choix talixio-agent
#   puis https://127.0.0.1:7700 (certificat fabriqué par le maître : le navigateur demande confirmation)
#
# /config/talixio.toml : la configuration (docker/talixio.toml par défaut ; montez la vôtre par-dessus).
# /data : ce qui dure. /data/work, le seul dossier ouvert aux agents ; /data/.talixio, le dossier
# d'état (journal, mémoire, certificat, clés saisies dans l'interface).

FROM rust:1-alpine AS construction
RUN apk add --no-cache build-base
WORKDIR /src
COPY . .
RUN cargo build --release --locked \
 && strip target/release/talixio-agent \
 && mkdir -p /data/work && chown -R 65532:65532 /data && chmod -R 700 /data

FROM scratch
COPY --from=construction /src/target/release/talixio-agent /talixio-agent
COPY --from=construction --chown=65532:65532 /data /data
COPY docker/talixio.toml /config/talixio.toml
# un compte sans aucun droit : le maître ne tourne jamais en root
USER 65532:65532
WORKDIR /data
VOLUME /data
EXPOSE 7700
ENTRYPOINT ["/talixio-agent"]
CMD ["root", "--config", "/config/talixio.toml"]
