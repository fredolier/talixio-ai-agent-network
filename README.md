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

```bash
docker logs talixio
```

```bash
docker rm -f talixio
```

## En service

Pour que le maître reparte seul au démarrage de la machine et après une panne, depuis le dossier
où se trouve `talixio.toml` :

```bash
talixio-agent service install --config talixio.toml
```

Linux : unité systemd de votre compte (en root : `--user <compte>`, le maître ne tourne jamais en
root). macOS : agent launchd. Windows : tâche planifiée à l'ouverture de session. `--print` montre
ce qui serait fait sans rien faire ; `service status` et `service uninstall` font le reste. Un
service n'hérite pas des variables de votre terminal : donnez les clés par `api_key_file`, depuis
l'interface, ou sous Linux par `--env-file`.

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
