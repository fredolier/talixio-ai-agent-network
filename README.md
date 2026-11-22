# Talixio Agent Network

Des agents IA autonomes, en grappe. Chaque agent est un petit process Rust avec un modèle, des
instructions et des droits limités. Un agent peut créer des enfants et leur confier des missions ;
un enfant ne parle qu'à son parent et n'a jamais plus de droits que lui.

- Spécification : [docs/SPEC.md](docs/SPEC.md)
- Planning et avancement : [docs/PLAN.md](docs/PLAN.md)

## Installer

Un exécutable par système est publié à chaque version (Linux et macOS, x86_64 et ARM ; Windows
x86_64), avec ses empreintes. Une ligne suffit ; rien n'est lancé et rien ne demande root.

Linux et macOS :

```bash
curl -fsSL https://github.com/fredolier/talixio-ai-agent-network/releases/latest/download/install.sh | sh
```

Windows (PowerShell) :

```powershell
irm https://github.com/fredolier/talixio-ai-agent-network/releases/latest/download/install.ps1 | iex
```

Le script télécharge l'exécutable de la machine, vérifie son empreinte SHA-256 et le pose dans
`~/.local/bin` (Windows : `%LOCALAPPDATA%\Talixio`). `TALIXIO_VERSION=0.10.0` choisit une version,
`TALIXIO_INSTALL_DIR` un autre dossier. On peut aussi télécharger le fichier à la main depuis la
page des versions du dépôt. L'empreinte vient du même endroit que l'exécutable : elle protège
d'un téléchargement abîmé, pas d'un dépôt compromis.

## Dans Docker

Pour essayer sans rien installer et sans risque pour la machine : une image qui ne contient que
l'exécutable (6,5 Mo), sans système ni shell, où le maître tourne sous un compte sans droit.

Le plus court, depuis le dépôt : `./tools.sh up` construit l'image et lance le maître,
`./tools.sh token` affiche le jeton de l'interface (tiré au hasard, gardé hors du dépôt dans
`~/.config/talixio/`), `./tools.sh up claude` fait de même avec votre abonnement Claude (voir plus
bas), `./tools.sh logs`, `status` et `stop` font le reste. Les commandes qu'il lance sont celles
qui suivent, à la main :

```bash
docker build -t talixio-agent .
```

```bash
export TALIXIO_API_TOKEN="$(openssl rand -hex 32)"; echo "$TALIXIO_API_TOKEN"
```

```bash
docker run -d --name talixio -p 127.0.0.1:7700:7700 -v talixio-data:/data --cap-drop ALL --security-opt no-new-privileges --pids-limit 256 --memory 1g -e TALIXIO_API_TOKEN talixio-agent
```

La deuxième commande tire le jeton de l'interface au hasard et l'affiche : gardez-le. Un jeton
faible ou recopié d'un exemple est refusé. Aucune clé de modèle n'est demandée pour démarrer.

Puis ouvrez `https://127.0.0.1:7700` (le certificat est fabriqué par le maître : le navigateur
demande confirmation) et entrez votre jeton. Dans « Fournisseurs », connectez le vôtre ; dans
« Modèles », activez un modèle de texte : le premier activé devient celui du maître, et
« Donner au maître » en change. Les agents ne lisent et n'écrivent que dans `/data/work`, et
aucune commande ne leur est permise. Pour d'autres droits, montez votre configuration :
`-v /chemin/talixio.toml:/config/talixio.toml:ro` (modèle : `docker/talixio.toml`).

### Avec votre abonnement Claude

L'image ci-dessus est vide : elle ne peut pas héberger la commande `claude`. Une seconde image
l'ajoute, pour servir des modèles avec un abonnement Claude (Pro, Max) plutôt qu'une clé d'API :

```bash
docker build -f docker/Dockerfile.claude -t talixio-agent:claude .
```

```bash
docker run -d --name talixio -p 127.0.0.1:7700:7700 -v talixio-data:/data --cap-drop ALL --security-opt no-new-privileges --pids-limit 512 --memory 3g -e TALIXIO_API_TOKEN talixio-agent:claude
```

Puis connectez `claude` à votre abonnement, une fois (`./tools.sh login` fait la même chose) :

```bash
docker exec -it talixio claude auth login
```

`claude` affiche un lien : ouvrez-le, connectez-vous chez Anthropic, recollez le code. Il garde
sa connexion dans le volume (`/data/.claude`, interdit aux agents) : elle survit aux mises à jour
du conteneur. Autre voie, sans cette étape : tirer un jeton sur votre poste avec
`claude setup-token` et le passer au lancement (`-e CLAUDE_CODE_OAUTH_TOKEN`).

Dans « Fournisseurs », « Claude (abonnement) », « Connecter » ; puis activez Opus, Sonnet ou
Haiku dans « Modèles ». Le maître lance la commande `claude` telle quelle : il ne lit ni ne
garde votre jeton, et n'appelle pas l'API d'Anthropic lui-même. Les outils intégrés de `claude`
sont coupés : il n'a que ceux de l'agent, dont chaque appel repasse par les contrôles de droits.
L'usage compte dans les limites de votre abonnement, aux conditions d'Anthropic pour un usage
personnel ; elles ont changé plusieurs fois en 2026, vérifiez-les. Hors Docker, il suffit que
`claude` soit installé et connecté sous le compte du maître. « Anthropic » (clé d'API) reste
disponible à côté : les deux peuvent servir en même temps.

```bash
docker logs talixio
```

```bash
docker rm -f talixio
```

## Paquets : commandes et rôles

Un paquet donne au maître des commandes (des scripts, avec leurs arguments permis) et des rôles
(des consignes toutes faites pour un agent). C'est vous qui les installez ; le maître ne peut ni
s'en ajouter ni les modifier, et il vérifie leurs empreintes à chaque démarrage.

```bash
./tools.sh pack list
```

```bash
./tools.sh pack show catalogue
```

```bash
./tools.sh pack install catalogue
```

```bash
./tools.sh run claude
```

Hors de Docker : le plus simple est le compte dédié (voir « En service »), après quoi
`sudo talixio-agent pack install catalogue` suffit. À la main : `talixio-agent pack install
catalogue --dir /opt/talixio/packs` sous un autre compte que celui du maître, et
`packs_dir = "/opt/talixio/packs"` dans `talixio.toml`. Voir `examples/catalogue`.

### Dépôt de paquets

Des paquets peuvent venir d'un dépôt : un dossier de fichiers derrière https, dont l'index est
signé. Rien n'est lu sans la clé publique de son éditeur, et l'hébergeur ne peut rien y changer.

```bash
talixio-agent pack list --depot https://exemple.fr/paquets --key <clé publique>
```

```bash
sudo talixio-agent pack install <nom> --depot https://exemple.fr/paquets --key <clé publique>
```

Publier les vôtres (la clé privée reste chez vous, hors du serveur) :

```bash
talixio-agent pack keygen --out ma-cle
```

```bash
talixio-agent pack publish --from mes-paquets --out depot --key-file ma-cle
```

Le dossier `depot` se dépose tel quel sur un serveur de fichiers. Il n'y a pas encore de dépôt
officiel Talixio.

## En service

Pour que le maître reparte seul au démarrage de la machine et après une panne, depuis le dossier
où se trouve `talixio.toml` :

```bash
talixio-agent service install --config talixio.toml
```

Linux : unité systemd de votre compte. macOS : agent launchd. Windows : tâche planifiée à
l'ouverture de session. `--print` montre ce qui serait fait sans rien faire ; `service status` et
`service uninstall` font le reste. Un service n'hérite pas des variables de votre terminal :
donnez les clés par `api_key_file`, depuis l'interface, ou sous Linux par `--env-file`.

### Compte dédié (Linux, recommandé)

```bash
sudo talixio-agent service install --config talixio.toml
```

En root, la commande met tout en place pour que le maître ne puisse modifier ni son exécutable,
ni sa configuration, ni ses paquets :

| Quoi | Où | À qui |
|---|---|---|
| exécutable | `/opt/talixio/talixio-agent` | root |
| paquets (commandes, rôles) | `/opt/talixio/packs` | root |
| configuration | `/etc/talixio/talixio.toml` | root (lisible par le compte) |
| état, clés saisies dans l'interface, travail | `/var/lib/talixio` | `talixio-agent` |

Le compte `talixio-agent` est créé s'il manque, sans mot de passe ni shell. Ensuite :

```bash
sudo talixio-agent pack install catalogue
```

```bash
sudo systemctl restart talixio-agent
```

Modifier la configuration : `sudo` sur `/etc/talixio/talixio.toml`, puis redémarrer le service
(`talixio-agent check --config <fichier>` la vérifie sans rien lancer). Le jeton de l'interface :
`sudo cat /var/lib/talixio/.talixio/api-token`. Les chemins relatifs de la configuration partent
de `/var/lib/talixio`. Pour faire tourner le maître sous un compte à vous, là où sont ses
fichiers : `sudo talixio-agent service install --user <compte>` (jamais root).

Au redémarrage, le maître retrouve son journal, sa mémoire, ses fournisseurs, et ses agents
**permanents** : demandez-lui de créer un agent « permanent », il le recrée à chaque démarrage
avec sa mémoire, jusqu'à ce que vous l'arrêtiez. Les agents ordinaires ne reviennent pas.

## Compiler

```bash
cargo build --release
```

L'exécutable est `target/release/talixio-agent`. C'est tout ce qu'il faut déployer, avec un fichier de configuration.

### Exécutable statique (recommandé pour déployer)

Compilé ainsi, l'exécutable ne dépend d'aucune bibliothèque du système : il tourne tel quel sur
n'importe quelle distribution Linux du même processeur, et dans n'importe quelle image Docker,
même vide. C'est la version à utiliser dès que le maître déploie des agents ailleurs.

```bash
sudo apt-get install -y musl-tools
```

```bash
rustup target add x86_64-unknown-linux-musl
```

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

L'exécutable est alors `target/x86_64-unknown-linux-musl/release/talixio-agent` (5,3 Mo).

## Lancer le maître

```bash
cp talixio.example.toml talixio.toml
```

```bash
ANTHROPIC_API_KEY=... target/release/talixio-agent root --config talixio.toml
```

## Piloter

### Dans un navigateur

Ouvrir l'adresse du maître (`https://127.0.0.1:7700/` par défaut) et saisir le jeton de l'API
(`.talixio/api-token`). Sur un téléphone, la page s'installe comme une application.

![La grappe](docs/images/interface-grappe.png)

L'écran « Commandes » montre ce que le maître a le droit d'exécuter et à qui il l'a confié ;
« Rôles » montre les agents créés.

![Les commandes](docs/images/interface-commandes.png)

« Fournisseurs » liste tous les fournisseurs du catalogue embarqué (`catalog.toml`) : pays,
Europe ou non, niveau de confiance, connecté ou non. « Modèles » liste leurs modèles avec leur
qualité et leur prix indicatif, et dit lesquels sont activés chez le maître.

On se connecte depuis la page : dans « Fournisseurs », choisissez un fournisseur, collez votre clé
(un lien mène à la page où la créer), « Connecter ». Le maître essaie la clé, la range dans un
fichier que seul son compte peut lire (`<state_dir>/keys/`), et ne la rend plus jamais. Dans
« Modèles », « Activer » rend un modèle utilisable aussitôt, sans redémarrage. OpenRouter se
connecte par autorisation dans le navigateur ; OVHcloud se connecte aussi sans clé (« Connecter
sans clé » : anonyme, limites basses, de quoi essayer) ; les autres (OpenAI, Anthropic, Gemini, Mistral,
OVHcloud, AKI.IO, Scaleway, ElevenLabs, DeepSeek) par clé. Le maître lui-même reçoit son
modèle de la même façon : sans `model` dans `talixio.toml`, il démarre sans modèle ni clé, prend
le premier modèle de texte activé, et « Donner au maître » lui en donne un autre. Déclarer les
modèles dans `talixio.toml` reste possible ; un `model` écrit là ne se change pas depuis la page.

![Les fournisseurs](docs/images/interface-fournisseurs.png)

### Voix, image, prévision, décision

Un agent peut aussi transcrire un son, lire un texte à voix haute, créer ou décrire une image,
chercher dans sa mémoire par le sens, prévoir la suite d'une série ou poser des questions fermées
à un modèle de décision. Ces modèles se déclarent chez le maître avec leur `nature`, et se
comptent en appels :

```toml
[models.oreille]
provider = "openai"
nature = "transcription"
base_url = "https://api.mistral.ai/v1"
model = "voxtral-mini-latest"
api_key_file = "/etc/talixio/mistral.key"

[capabilities.max_calls]
transcription = 200
```

Comme pour le texte, la clé reste chez le maître et un enfant n'obtient un modèle de service et
un nombre d'appels que si son parent les lui confie, dans la limite des siens. Sans ligne dans
`max_calls`, aucun appel n'est permis. Toutes les natures et leurs fournisseurs :
`talixio.example.toml`.

L'interface (`web/`) est faite avec React et shadcn/ui. Son résultat construit, `web/dist/`, est
au dépôt : `cargo build` suffit. Pour la modifier :

```bash
cd web && npm ci && npm run build
```

### En ligne de commande

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
  programmes factices, pas encore essayé sur une vraie machine. La cible doit avoir le même processeur que
  le maître ; avec l'exécutable statique, la distribution n'a plus d'importance.
- Le cloisonnement est fait par le process lui-même, pas encore par le système d'exploitation :
  une commande autorisée s'exécute avec tous les droits du compte. N'autoriser que des commandes
  inoffensives, et lancer le maître sous un compte dédié sans privilèges (voir la spécification, §7).
- Les modèles de service (voix, image, prévision, décision…) n'ont été essayés qu'avec de faux fournisseurs : le premier
  essai chez un vrai fournisseur peut demander un ajustement.
- Testé en intégration continue sous Linux, macOS et Windows ; pas encore essayé sur un poste Windows ou macOS réel, ni avec un vrai `ssh` ou un vrai Docker.
