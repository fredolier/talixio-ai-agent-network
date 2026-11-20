# Talixio Agent Network — spécification

Ce document fixe ce que le système doit faire, les décisions prises et ce qui reste ouvert.
Il fait foi : toute décision nouvelle y est reportée dans le même commit que le code.
L'avancement tâche par tâche est dans [PLAN.md](PLAN.md).

## 1. Vision

Des agents IA autonomes écrits en Rust. Chaque agent est un petit process qui tourne sur
n'importe quelle machine et n'importe quel OS. On lui donne un modèle, des instructions et des
droits limités. Un agent peut créer d'autres agents, sur sa machine ou ailleurs, et leur
confier des missions : cela forme une grappe en arbre, pilotée depuis l'agent du sommet.

Toute l'intelligence est dans les prompts, les outils et la communication ; la technique reste
simple et rigoureuse sur la sécurité.

Exemples visés :

- un agent d'audit de sécurité installé sur une VM, qui diagnostique et rend compte ; son parent
  décide ensuite d'envoyer un agent correcteur, avec d'autres droits ;
- un agent d'optimisation d'un serveur (fichiers système, performances) ;
- une équipe d'agents créée pour une mission longue (promouvoir une marque sur le web) ;
- un assistant installé sur le poste d'une personne, qui l'aide dans son travail et discute avec elle.

## 2. Vocabulaire

| Terme | Sens |
|---|---|
| Agent | Un process `talixio-agent` : un modèle, des instructions, des droits, des outils. |
| Maître (niveau 0) | L'agent du sommet. Il détient les clés des modèles et sert l'API et l'interface. |
| Parent / enfant | Un agent qui en crée un autre est son parent. |
| Grappe | L'arbre formé par le maître et toute sa descendance. |
| Droits | Ce qu'un agent peut faire : lire, écrire, exécuter, créer des enfants, quels modèles, quel budget. |
| Mission | Un objectif et un critère de réussite mesurable, confiés par un parent. |
| Lien | La connexion entre un enfant et son parent. |

## 3. Exigences

État : ✅ fait · 🟡 partiel · ⬜ à faire · ❓ à étudier.

### Agent

| N° | Exigence | État |
|---|---|---|
| A1 | Un seul exécutable Rust et des fichiers de configuration, rien d'autre à déployer. L'interface web fait exception, mais elle est embarquée dans l'exécutable du maître. | 🟡 exécutable seul fait (3,3 Mo) ; interface à faire |
| A2 | Tourne sur Linux, macOS et Windows. | 🟡 code portable ; testé sous Linux seulement |
| A3 | Se connecte à n'importe quel modèle : Anthropic, et tout serveur compatible OpenAI (OpenAI, Gemini, Mistral, Ollama, vLLM, OpenRouter), comme dans Talixio AI Assistant. | ✅ |
| A4 | Exécutable de petite taille. | ✅ profil de compilation optimisé en taille |
| A5 | Peut être installé en service ou en démon sur une machine. | ❓ |
| A6 | Le premier agent installé sur une machine peut y tourner en service et servir de relais pour lancer les autres. | ❓ |

### Droits et cloisonnement

| N° | Exigence | État |
|---|---|---|
| D1 | Tout est refusé par défaut. Les droits sont : lecture de dossiers, écriture de dossiers, commandes autorisées, création d'enfants, modèles, budget. | ✅ |
| D2 | Les commandes sont une liste fermée, sans shell : un programme et des arguments contrôlés. | ✅ |
| D3 | Un enfant ne reçoit jamais plus de droits que son parent. | ✅ |
| D4 | Chaque appel d'outil est revérifié par le process, quoi que le modèle demande. | ✅ |
| D5 | Cloisonnement par l'OS en plus du contrôle interne : utilisateur dédié, conteneur, Landlock/seccomp sous Linux, chroot selon les cas. | ⬜ |
| D6 | Les clés des modèles ne vivent qu'au niveau 0 (coffre). Les appels de modèle des enfants remontent de parent en parent jusqu'au maître. | 🟡 relais fait ; les clés viennent de l'environnement du maître, pas encore d'un coffre chiffré |
| D7 | Budget de tokens par agent, descendance comprise, contrôlé par le parent. | ✅ |
| D8 | Chaque action d'un agent est journalisée et remonte jusqu'au maître. | ✅ |
| D9 | Les actions sensibles peuvent exiger une validation humaine, qui remonte jusqu'à l'interface. | ⬜ |
| D10 | Ce que dit un enfant est une donnée pour son parent, jamais un ordre. | 🟡 balisage et consigne dans le prompt ; à durcir selon l'audit |
| D11 | Passe de sécurité dédiée sur le code, les connexions et le cloisonnement, à chaque étape. | 🟡 première passe en cours |

### Grappe et communication

| N° | Exigence | État |
|---|---|---|
| G1 | Un agent qui en a le droit crée des enfants et choisit lui-même leur rôle, leurs instructions et leurs droits. Toute la configuration se fait par l'agent. | ✅ |
| G2 | Un enfant ne communique qu'avec son parent. Le maître reçoit ainsi tout ce qui remonte. | ✅ |
| G3 | Un enfant peut demander de l'aide à son parent (information, droit, décision). | ✅ outil `ask_parent` |
| G4 | L'enfant doit voir son parent : c'est lui qui se connecte. Le parent installe et lance l'enfant par le moyen de son choix. | ✅ en local |
| G5 | Déploiement d'un enfant : sur la machine, dans Docker, par SSH, ou par un autre moyen d'accès distant. | 🟡 local seulement |
| G6 | Lien chiffré et authentifié dès qu'il quitte la machine (port HTTPS ou TCP ouvert par le parent). | ⬜ aujourd'hui : interface locale seulement, écoute distante refusée |
| G7 | Lien coupé : l'enfant se met en veille ; si le parent ne revient pas, il s'éteint avec sa descendance. Jamais d'enfant autonome sans parent. | ✅ |
| G8 | Aucun message perdu pendant une coupure. | ✅ numérotation et acquittement |
| G9 | Un parent peut arrêter un enfant. | ✅ |
| G10 | Un parent peut désinstaller un enfant de la machine où il l'a déployé. | ⬜ va avec le déploiement distant |
| G11 | Chaque agent a un nom et un rôle, et déclare sa machine, son OS et son adresse IP. | ✅ |

### Autonomie

| N° | Exigence | État |
|---|---|---|
| M1 | Une mission a un objectif et un critère de réussite mesurable ; sans les deux, elle est refusée. | ✅ |
| M2 | L'agent itère seul jusqu'à atteindre le critère : il agit, mesure, corrige. S'il s'arrête sans conclure, il est relancé. | ✅ |
| M3 | Mission accomplie ou impossible : l'agent rend compte, preuves à l'appui, se met en pause, et attend de nouvelles consignes de son parent. | ✅ outil `finish_mission` |
| M4 | Missions longues : la conversation d'un agent ne doit pas déborder la fenêtre du modèle. | ⬜ |
| M5 | Accès au web pour les missions qui en ont besoin (navigation, connexion à des sites). | ⬜ |

### Pilotage

| N° | Exigence | État |
|---|---|---|
| P1 | API JSON au niveau du maître, utilisable par Talixio AI Assistant. | ✅ |
| P2 | Interface web soignée (shadcn/ui) : la grappe en direct, chaque agent avec nom, rôle, machine, IP, état ; le fil des messages ; le dialogue avec le maître. | ⬜ |
| P3 | Vue de la grappe et du journal en ligne de commande. | ✅ |

## 4. Décisions

1. **Un seul exécutable**, le même pour le maître et les enfants (`root`, `child`).
2. **L'enfant se connecte au parent** (WebSocket). Le parent n'a pas à joindre l'enfant une
   fois celui-ci lancé. Hypothèse retenue : un enfant peut toujours atteindre l'adresse de son parent.
3. **Jeton par enfant**, créé par le parent au lancement et transmis sur l'entrée standard du
   process : ni dans la ligne de commande, ni dans l'environnement.
4. **Les clés des modèles ne descendent jamais.** Un enfant demande un modèle par son alias ;
   chaque parent vérifie l'alias et le budget, puis relaie vers le haut.
5. **Les droits s'atténuent** : `check_grant` refuse tout droit que le parent n'a pas lui-même.
   Le modèle et le mode de déploiement de l'enfant sont fixés par le parent, pas par l'enfant.
6. **L'environnement est nettoyé** pour tout process lancé par un agent (enfant ou commande) :
   seules quelques variables neutres passent.
7. **Ce qu'un enfant déclare sur lui-même n'est pas cru** : son identité et ses droits affichés
   sont ceux que le parent lui a donnés.
8. **Seuls `ask_parent` et `finish_mission` parviennent au parent.** Le reste de ce qu'écrit un
   enfant va au journal, pas dans la conversation du parent.
9. **Sans chiffrement, pas d'écoute hors de la machine** : toute adresse non locale est refusée
   au démarrage tant que le lien chiffré n'existe pas.
10. **Interface** : React et shadcn/ui, compilée puis embarquée dans l'exécutable du maître. Elle
    n'utilise que l'API JSON publique.

## 5. Architecture

```
src/
  main.rs       ligne de commande : root, child, tree, send, watch, stop
  config.rs     configuration du maître (TOML), consignes de démarrage d'un enfant
  policy.rs     droits : validation, atténuation, contrôle des chemins et des commandes
  protocol.rs   messages du lien, état d'un agent, file des messages garantis
  node.rs       état d'un agent : enfants, budgets, journal, arrêt
  link.rs       lien parent-enfant : identification, battements, veille, extinction, relais des modèles
  agent.rs      boucle de l'agent : messages reçus, modèle, outils, suivi de mission
  tools.rs      outils : fichiers, commandes, enfants, mission
  spawn.rs      lancement d'un enfant (process local)
  api.rs        écoute réseau : lien des enfants, API JSON du maître
  model/        format neutre ; connecteurs Anthropic et compatible OpenAI
tests/e2e.rs    la grappe de bout en bout, avec un faux serveur de modèles
```

### Lien parent-enfant

Trames JSON `{ "seq": n?, "msg": { "type": … } }` sur WebSocket, point d'entrée `/link`.

| Sens | Messages |
|---|---|
| Enfant → parent | `hello` (identité et jeton), `tree` (état de sa branche), `report` (fin de mission, question, erreur), `activity` (journal), `model_request` |
| Parent → enfant | `welcome` / `rejected`, `instruction`, `model_response`, `stop_descendant`, `shutdown` |
| Les deux | `ping` / `pong`, `ack` |

Les messages portant un `seq` sont acquittés et renvoyés après une reconnexion ; le destinataire
écarte les doublons. Délais réglables (`[timing]`) : battement, mise en veille, extinction.

### Outils d'un agent

| Outil | Condition |
|---|---|
| `read_file`, `list_dir` | au moins un dossier lisible |
| `write_file` | au moins un dossier inscriptible |
| `run_command` | au moins une commande autorisée |
| `spawn_agent`, `send_to_agent`, `stop_agent`, `list_agents` | droit de créer des enfants |
| `ask_parent`, `finish_mission` | l'agent a un parent |

### API JSON du maître

Authentification : `Authorization: Bearer <jeton>`. Le jeton vient de `TALIXIO_API_TOKEN`, sinon il
est créé au premier démarrage dans `<state_dir>/api-token`.

| Méthode | Chemin | Rôle |
|---|---|---|
| GET | `/api/health` | Vie du maître (sans jeton). |
| GET | `/api/tree` | La grappe : chaque agent avec nom, rôle, état, machine, OS, IP, droits, tokens, enfants. |
| GET | `/api/activity?after=N&limit=M` | Le journal après l'événement N. |
| GET | `/api/events?after=N` | Le journal en continu (Server-Sent Events). |
| POST | `/api/messages` `{ "text": … }` | Un message au maître. |
| POST | `/api/agents/{id}/stop` | Arrête un agent et sa descendance. |

## 6. Questions ouvertes

| N° | Question | Piste |
|---|---|---|
| Q1 | Où tourne le maître ? Sur un poste, l'éteindre éteint toute la grappe. | Un serveur toujours allumé (par exemple celui de Talixio AI Assistant), le poste ne servant qu'à l'interface. À trancher. |
| Q2 | Droits d'un enfant sur une autre machine : les chemins et commandes du parent n'y ont pas de sens. | Un plafond de droits par cible de déploiement, défini dans le droit de créer des enfants. |
| Q3 | Chiffrement du lien : certificat du parent épinglé chez l'enfant au lancement, ou autorité propre à la grappe. | À concevoir à l'étape 2, avec l'audit. |
| Q4 | Service ou démon (A5, A6) : systemd, launchd, service Windows ; un agent « hôte » par machine qui lance les autres. | À étudier à l'étape 3. |
| Q5 | Assistant sur poste de travail : il observe une personne. | Visible et accepté par elle ; étape à part. |
| Q6 | Coffre des clés au niveau 0. | Fichier chiffré ou coffre du système, à choisir. |
