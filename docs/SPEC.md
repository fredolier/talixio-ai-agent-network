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
| D2 | Les commandes sont une liste fermée, sans shell : un programme (chemin absolu) et des arguments contrôlés position par position. Un argument libre n'est jamais une option ; un chemin passé à une commande est vérifié contre les droits de lecture ou d'écriture. | ✅ dans le process ; voir D5 |
| D3 | Un enfant ne reçoit jamais plus de droits que son parent. | ✅ |
| D4 | Chaque appel d'outil est revérifié par le process, quoi que le modèle demande. | ✅ |
| D5 | Cloisonnement par l'OS en plus du contrôle interne : utilisateur dédié, conteneur, Landlock/seccomp sous Linux, chroot selon les cas. | ⬜ **tant qu'il manque, une commande autorisée a tous les droits du compte du process** (§7) |
| D6 | Les clés des modèles ne vivent qu'au niveau 0 (coffre). Les appels de modèle des enfants remontent de parent en parent jusqu'au maître. | 🟡 relais fait ; clés lues dans un fichier en 0600 ou dans l'environnement du maître, mémoire et environnement du process fermés aux autres process ; pas encore de coffre chiffré |
| D7 | Budget de tokens par agent, descendance comprise, contrôlé par le parent. Chaque appel réserve sa part avant de partir ; le budget du maître vaut pour toute la grappe. | ✅ |
| D8 | Chaque action d'un agent est journalisée et remonte jusqu'au maître. | ✅ |
| D9 | Les actions sensibles peuvent exiger une validation humaine, qui remonte jusqu'à l'interface. | ⬜ |
| D10 | Ce que dit un enfant est une donnée pour son parent, jamais un ordre. | 🟡 chaque message entre dans une balise à marqueur secret, infalsifiable par son contenu ; comptes rendus bornés et refusés hors mission. Une injection reste possible par le sens du texte : D9 est le vrai garde-fou |
| D11 | Passe de sécurité dédiée sur le code, les connexions et le cloisonnement, à chaque étape. | 🟡 première passe faite et traitée (§7) ; à refaire à chaque étape |
| D12 | Le dossier d'état d'un agent (jeton de l'API, journaux) et les fichiers de clés sont interdits à ses outils, quels que soient ses droits. | ✅ |
| D13 | Rien de ce qu'un agent lance ne lui survit : ses commandes et ce qu'elles ont lancé s'arrêtent avec elles, et avec lui. | ✅ sous Linux (groupes de process) |

### Grappe et communication

| N° | Exigence | État |
|---|---|---|
| G1 | Un agent qui en a le droit crée des enfants et choisit lui-même leur rôle, leurs instructions et leurs droits. Toute la configuration se fait par l'agent. | ✅ |
| G2 | Un enfant ne communique qu'avec son parent. Le maître reçoit ainsi tout ce qui remonte. | ✅ |
| G3 | Un enfant peut demander de l'aide à son parent (information, droit, décision). | ✅ outil `ask_parent` |
| G4 | L'enfant doit voir son parent : c'est lui qui se connecte. Le parent installe et lance l'enfant par le moyen de son choix. | ✅ en local |
| G5 | Déploiement d'un enfant : sur la machine, dans Docker, par SSH, ou par un autre moyen d'accès distant. | 🟡 local seulement |
| G6 | Lien chiffré et authentifié dès qu'il quitte la machine (port HTTPS ou TCP ouvert par le parent). | 🟡 authentification mutuelle faite (le secret ne circule jamais) ; chiffrement à faire : interface locale seulement, écoute distante refusée |
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

### Budgets et quotas

| N° | Exigence | État |
|---|---|---|
| B1 | Chaque agent a un budget, et le répartit lui-même entre ses enfants. La règle vaut en cascade, à chaque niveau. | 🟡 un parent fixe le budget de chaque enfant, au plus égal au sien, et chaque appel est compté à tous les niveaux qu'il traverse. Manque : la somme des budgets distribués n'est pas plafonnée par ce qui reste au parent |
| B2 | Un agent connaît son budget et ce qu'il en reste, et en tient compte quand il délègue : pas de dépense libre. | ⬜ aujourd'hui il ne voit ni l'un ni l'autre ; à mettre dans ses consignes et dans ses outils |
| B3 | Arrivé à un seuil de son budget, l'agent prévient son parent ; budget épuisé, il s'arrête et attend. L'alerte remonte de parent en parent. | 🟡 budget épuisé : l'agent rend compte à son parent et se met en pause. Manque : l'alerte avant la fin, et sa remontée |
| B4 | Budget en argent, pas seulement en tokens : une clé d'API prépayée a un solde. | ⬜ il faut le prix de chaque modèle dans la configuration |
| B5 | Quotas d'un abonnement (fenêtres d'usage) : les voir, et les répartir comme un budget. | ❓ voir Q8 |
| B6 | Budgets et consommation visibles dans l'interface, agent par agent. | 🟡 consommation dans `/api/tree` ; budget restant à ajouter |

### Stockage et mémoire

| N° | Exigence | État |
|---|---|---|
| S1 | Chaque agent a un endroit à lui pour travailler et déposer ses fichiers. | ❓ voir Q7 |
| S2 | Chaque agent a une petite mémoire qui dure : ses consignes, ce qu'il a appris, où il en est. Elle survit à un redémarrage. | ❓ voir Q7 |
| S3 | Le journal et l'état de la grappe survivent à un redémarrage du maître. | ❓ aujourd'hui tout est en mémoire vive |

### Pilotage

| N° | Exigence | État |
|---|---|---|
| P1 | API JSON au niveau du maître, utilisable par Talixio AI Assistant. | ✅ |
| P2 | Interface web soignée (shadcn/ui) : la grappe en direct, chaque agent avec nom, rôle, machine, IP, état ; le fil des messages ; le dialogue avec le maître. | ⬜ |
| P3 | Vue de la grappe et du journal en ligne de commande. | ✅ |
| P4 | Pilotage depuis un téléphone (Android) : discuter avec le maître, voir la grappe, être prévenu. | ❓ voir Q9 |

## 4. Décisions

1. **Un seul exécutable**, le même pour le maître et les enfants (`root`, `child`).
2. **L'enfant se connecte au parent** (WebSocket). Le parent n'a pas à joindre l'enfant une
   fois celui-ci lancé. Hypothèse retenue : un enfant peut toujours atteindre l'adresse de son parent.
3. **Secret par enfant**, créé par le parent au lancement et transmis sur l'entrée standard du
   process : ni dans la ligne de commande, ni dans l'environnement. Il ne circule jamais sur le
   lien : à chaque connexion, parent et enfant se prouvent l'un à l'autre qu'ils le détiennent
   (défi-réponse HMAC-SHA256). Un faux parent n'apprend rien et ne peut ni commander ni arrêter l'enfant.
4. **Les clés des modèles ne descendent jamais.** Un enfant demande un modèle par son alias ;
   chaque parent vérifie l'alias et le budget, puis relaie vers le haut.
5. **Les droits s'atténuent** : `check_grant` refuse tout droit que le parent n'a pas lui-même.
   Le modèle et le mode de déploiement de l'enfant sont fixés par le parent, pas par l'enfant.
6. **L'environnement est nettoyé** pour tout process lancé par un agent (enfant ou commande) :
   seules quelques variables neutres passent.
7. **Ce qu'un enfant déclare n'est pas cru** : son identité, ses droits et sa consommation affichés
   sont ceux que le parent connaît. L'arbre qu'il déclare est borné et nettoyé, et il ne peut pas y
   revendiquer un agent d'une autre branche. Ses événements de journal sont signés du nom que le
   parent lui connaît.
8. **Seuls `ask_parent` et `finish_mission` parviennent au parent.** Le reste de ce qu'écrit un
   enfant va au journal, pas dans la conversation du parent.
9. **Sans chiffrement, pas d'écoute hors de la machine** : toute adresse non locale est refusée
   au démarrage tant que le lien chiffré n'existe pas.
10. **Interface** : React et shadcn/ui, compilée puis embarquée dans l'exécutable du maître. Elle
    n'utilise que l'API JSON publique.
11. **Les dossiers accordés sont des chemins réels**, résolus une fois quand le droit est donné. Un
    lien symbolique ne peut ni élargir un droit, ni faire écrire ailleurs ; un fichier est ouvert
    sans suivre de lien, puis revérifié une fois ouvert.
12. **Chaque message entre dans la conversation d'un agent sous une balise à marqueur secret**,
    propre à l'agent et tiré au hasard. Le contenu d'un message ne peut donc pas se faire passer
    pour un autre interlocuteur. Un tour qui échoue est retiré de la conversation.
13. **Une erreur de fournisseur de modèle reste chez le maître** (journal) ; les enfants n'en
    reçoivent qu'un message neutre.

## 5. Architecture

```
src/
  main.rs       ligne de commande : root, child, tree, send, watch, stop
  config.rs     configuration du maître (TOML), consignes de démarrage d'un enfant
  policy.rs     droits : validation, atténuation, contrôle des chemins et des commandes
  protocol.rs   messages du lien, état d'un agent, file des messages garantis
  node.rs       état d'un agent : enfants, budgets, journal, arrêt
  link.rs       lien parent-enfant : authentification mutuelle, battements, veille, extinction, relais des modèles
  agent.rs      boucle de l'agent : messages reçus, modèle, outils, suivi de mission
  tools.rs      outils : fichiers, commandes, enfants, mission
  spawn.rs      lancement d'un enfant (process local)
  api.rs        écoute réseau : lien des enfants, API JSON du maître
  model/        format neutre ; connecteurs Anthropic et compatible OpenAI
tests/e2e.rs       la grappe de bout en bout, avec un faux serveur de modèles
tests/security.rs  les attaques de l'audit, rejouées : chacune doit échouer
```

### Lien parent-enfant

Trames JSON `{ "seq": n?, "msg": { "type": … } }` sur WebSocket, point d'entrée `/link`.

| Sens | Messages |
|---|---|
| Enfant → parent | `hello` (identité, aléa), `proof` (preuve, état), `tree` (état de sa branche), `report` (fin de mission, question, erreur), `activity` (journal), `model_request` |
| Parent → enfant | `challenge` (aléa, preuve), `welcome` / `rejected`, `instruction`, `model_response`, `stop_descendant`, `shutdown` |
| Les deux | `ping` / `pong`, `ack` |

Les messages portant un `seq` sont acquittés et renvoyés après une reconnexion ; le destinataire
écarte les doublons. Délais réglables (`[timing]`) : battement, mise en veille, extinction.

Présentation : `hello` → `challenge` → `proof` → `welcome`, en 10 s au plus. La preuve de chacun
est `HMAC-SHA256(secret, rôle ‖ aléa de l'enfant ‖ aléa du parent ‖ identifiant)`. Huit
présentations simultanées au plus ; une requête portant un en-tête `Origin` (un navigateur) est
refusée. Trames : 16 Mio au plus pour un appel de modèle, 1 Mio pour le reste. Un compte rendu
n'est accepté que d'un enfant en mission, 20 au plus entre deux consignes, 20 000 caractères au plus.

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
| Q6 | Coffre des clés au niveau 0. | Fichier chiffré ou coffre du système, à choisir. En attendant : fichier de clé en 0600. À terme, les clés devraient vivre sous un autre compte que celui des agents. |
| Q7 | Stockage et mémoire des agents (S1–S3) : dossier, SQLite, ou les deux ? | Proposition, à valider : **des fichiers d'abord**. Chaque agent reçoit un dossier de travail à lui (`<state_dir>/agents/<id>/`), ajouté d'office à ses droits, et une mémoire en fichier texte (`memoire.md`) relue à chaque démarrage, qu'il tient avec un outil dédié. Le maître écrit le journal et l'état de la grappe sur disque. **SQLite chez le maître seulement**, quand l'interface aura besoin de chercher dans l'historique : embarqué, il ajoute environ 1 Mo à l'exécutable, et un agent enfant n'en a pas besoin. À étudier : mémoire partagée entre agents, taille maximale, effacement à la désinstallation. |
| Q8 | Abonnement Claude Max au lieu d'une clé d'API (B5). | À vérifier avant tout : qu'un abonnement Max puisse servir à des agents tiers n'est pas acquis, ni techniquement ni dans les conditions d'Anthropic. La voie sûre est la clé d'API avec un solde. Si c'est permis : lire les limites renvoyées par le fournisseur à chaque réponse, et les traiter comme un budget qui se recharge. |
| Q9 | Android (P4) : piloter depuis le téléphone, ou y faire tourner un agent ? | Piloter : l'interface web, installable comme application (PWA), suffit ; elle demande HTTPS et un maître joignable depuis le téléphone, donc l'étape 2. Agent sur le téléphone : Rust se compile pour Android, mais le système arrête les process d'arrière-plan et enferme chaque application ; à n'étudier que si un usage le demande. |

## 7. Sécurité

### Ce contre quoi on se défend

- **Un agent qui déraille** (modèle trompé par un texte qu'il lit, ou qui se trompe) : il ne doit
  pas sortir de ses droits, ni manipuler son parent, ni épuiser les ressources de la grappe.
- **Un autre process de la machine** : il ne doit ni lire les secrets, ni se faire passer pour un
  parent ou un enfant, ni piloter le maître.
- **Le réseau**, dès que le lien quittera la machine (étape 2).

### Limites connues, à ne pas oublier

1. **Les droits sont contrôlés par le process, pas encore par le système.** Une commande autorisée
   s'exécute avec tous les droits du compte : elle peut lire et écrire hors des dossiers accordés.
   Tant que D5 n'est pas fait, on n'autorise que des commandes inoffensives, écrites au plus serré,
   et le maître tourne sous un compte dédié sans privilèges.
2. **Le lien n'est pas chiffré.** L'authentification mutuelle empêche de se faire passer pour un
   parent ou un enfant, pas d'écouter ni de relayer. D'où l'écoute locale seulement.
3. **L'injection par le sens** : aucun balisage n'empêche un modèle d'être convaincu par un texte.
   Les droits minimaux, les budgets et, à venir, la validation humaine (D9) limitent les dégâts.
4. La vérification d'un chemin puis son ouverture ne sont pas une seule opération : la fenêtre est
   réduite (ouverture sans suivre de lien, puis comparaison du fichier ouvert), pas supprimée.
   Seul le cloisonnement par le système la ferme.
5. Un enfant décrit lui-même sa propre descendance : le parent borne et nettoie, mais ne peut pas
   vérifier ce qu'il ne voit pas.

### Première passe (2026-11-20) : constats et traitement

Audit complet du code, des connexions et du cloisonnement. Conclusion de l'auditeur : l'invariant
« un enfant n'a jamais plus de droits que son parent » ne tenait pas. Chaque constat vérifié a
été corrigé et rejoué en test (`tests/security.rs`, tests de `policy.rs`).

| Constat | Gravité | Traitement |
|---|---|---|
| Les règles de commande ne confinaient rien (`git log --output=…`, configuration de git) | critique | Un argument libre n'est jamais une option ; chemins vérifiés (`{read}`, `{write}`) ; programme en chemin absolu, hors de tout dossier inscriptible ; dossier d'exécution fixé ; configuration git de la machine ignorée. Limite 1 documentée. |
| Un lien symbolique élargissait les droits accordés à un enfant | élevée | Dossiers figés sur leur chemin réel ; un dossier devenu lien n'ouvre plus rien. |
| Écriture hors des dossiers par un lien cassé | élevée | Lien refusé en bout de chemin, ouverture sans suivre de lien. |
| Fenêtre entre vérification et ouverture d'un fichier ; liens physiques | moyenne | Fichier revérifié une fois ouvert ; écriture refusée sur un fichier à plusieurs noms. |
| Secrets lisibles : environnement du maître, jeton de l'API dans un dossier accordé | élevée | Mémoire et environnement du process fermés ; clé en fichier 0600 ; dossier d'état en 0700 et interdit aux outils ; jeton existant vérifié avant réemploi. |
| L'enfant envoyait son jeton à qui répondait | élevée | Authentification mutuelle par défi-réponse ; le secret ne circule plus. Arrêt propre sur SIGTERM et SIGHUP. |
| Un compte rendu sortait de sa balise et imitait le système | élevée | Balise à marqueur secret, un bloc par message, comptes rendus bornés et refusés hors mission. |
| Un compte rendu énorme bloquait le parent pour de bon | élevée | Tailles bornées ; un tour en échec est retiré de la conversation. |
| Budgets contournables par des appels simultanés ; pas de budget pour le maître | moyenne | Réservation avant chaque appel, 16 appels simultanés au plus par enfant, budget du maître appliqué à toute la grappe. |
| Un enfant revendiquait des agents d'une autre branche (arrêts détournés, journal falsifié) | moyenne | Arbre borné, identifiants uniques entre branches, journal signé par le parent, consommation comptée par le parent, caractères de contrôle retirés. |
| Un arbre énorme faisait tomber le parent | moyenne | 2000 agents et 32 niveaux au plus, trames de 1 Mio hors appels de modèle. |
| Connexions non identifiées occupant le parent | moyenne | 8 présentations simultanées, 10 s au plus, navigateurs refusés. |
| Des process survivaient à l'agent | moyenne | Groupe de process par commande, tué à la fin, au délai, et à l'extinction de l'agent. |
| Sortie de commande lue en entier en mémoire | moyenne | Lecture bornée à 64 Kio par sortie. |
| Un ordre visait un ancien enfant arrêté du même nom | faible | L'enfant en vie passe d'abord. |
| Texte d'un inconnu recopié dans le journal | faible | Rien n'est journalisé avant l'authentification. |
| Erreurs du fournisseur relayées telles quelles aux enfants | faible | Message neutre ; le détail reste au journal du maître. |

Reste de cette passe, reporté à l'étape 2 avec le lien distant : détecter un correspondant qui
envoie une trame au compte-gouttes pour paraître vivant ; séparer l'écoute du lien de celle de l'API.
