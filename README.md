# Talixio Agent Network

Des agents IA autonomes, en grappe. Chaque agent est un petit process Rust avec un modèle, des
instructions et des droits limités. Un agent peut créer des enfants et leur confier des missions ;
un enfant ne parle qu'à son parent et n'a jamais plus de droits que lui.

- Spécification : [docs/SPEC.md](docs/SPEC.md)
- Planning et avancement : [docs/PLAN.md](docs/PLAN.md)

## Compiler

```bash
cargo build --release
```

L'exécutable est `target/release/talixio-agent`. C'est tout ce qu'il faut déployer, avec un fichier de configuration.

## Lancer le maître

```bash
cp talixio.example.toml talixio.toml
```

```bash
ANTHROPIC_API_KEY=... target/release/talixio-agent root --config talixio.toml
```

## Piloter

Depuis le dossier du maître (le jeton de l'API et l'empreinte de son certificat sont lus dans `.talixio/`) :

```bash
talixio-agent send "Audite le dossier /srv/projet et dis-moi ce qui cloche."
```

```bash
talixio-agent watch
```

```bash
talixio-agent tree
```

```bash
talixio-agent stop <identifiant>
```

Ce qui a changé de version en version :

```bash
talixio-agent releases
```

L'API JSON est décrite dans la spécification.

## Tester

```bash
cargo test
```

## Limites actuelles

- Déploiement par SSH et dans Docker (`[targets]`, voir `talixio.example.toml`) : écrit et testé avec des
  programmes factices, pas encore essayé sur une vraie machine. La cible doit avoir le même système et le
  même processeur que le maître.
- Le cloisonnement est fait par le process lui-même, pas encore par le système d'exploitation :
  une commande autorisée s'exécute avec tous les droits du compte. N'autoriser que des commandes
  inoffensives, et lancer le maître sous un compte dédié sans privilèges (voir la spécification, §7).
- Testé sous Linux seulement.
