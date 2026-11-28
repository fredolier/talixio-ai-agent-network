# Talixio Agent Network — spécification

Ce document fixe ce que le système doit faire, les décisions prises et ce qui reste ouvert.
Il fait foi : toute décision nouvelle y est reportée dans le même commit que le code.
L'avancement tâche par tâche est dans [PLAN.md](PLAN.md).
La liste exhaustive de ce qui a été demandé, phrase d'origine à l'appui, avec un critère
d'acceptation par demande, est dans [EXIGENCES.md](EXIGENCES.md) ; l'état constaté par une
vérification indépendante est dans [VERIFICATION.md](VERIFICATION.md).

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
| A1 | Un seul exécutable Rust et des fichiers de configuration, rien d'autre à déployer. L'interface web fait exception, mais elle est embarquée dans l'exécutable du maître. | 🟡 exécutable seul fait (5,2 Mo, SQLite et TLS compris) ; interface embarquée (5 fichiers statiques, sans étape de construction) |
| A2 | Tourne sur Linux, macOS et Windows. | 🟢 Les trois compilent et passent les tests en intégration continue (`ubuntu-latest`, `macos-latest`, `windows-latest`), test de bout en bout compris. Les protections sont écrites deux fois, dans `src/os.rs`, parce qu'elles reposent sur des mécanismes propres à chaque système. **Windows** : fichiers secrets fermés par une liste de droits protégée (le compte, SYSTEM, Administrateurs) et vérifiés à l'ouverture (propriétaire, chaque entrée) ; ouverture sans suivre les liens ni les points d'analyse ; chaque commande lancée dans un Job Object qui tue toute sa descendance avec elle, et l'agent lui-même dans un Job qui emporte tout s'il meurt ; process fermé aux autres comptes. **macOS** : code Unix commun, `PT_DENY_ATTACH` ; pas de PDEATHSIG, la descendance est tuée par groupe de process. **Restes** : (1) sous Windows, le propriétaire d'un process peut réécrire ses droits : la fermeture du process y est un frein, pas une garantie ; (2) le déploiement SSH suppose une cible POSIX, un agent Windows ne peut pas encore être une cible ; (3) `tests/deploy.rs` (faux `ssh` en script shell) et un test d'outils ne tournent que sous Unix ; (4) essais faits sur les machines de l'intégration continue, jamais sur un poste réel. |
| A3 | Se connecte à n'importe quel modèle : Anthropic, et tout serveur compatible OpenAI (OpenAI, Gemini, Mistral, Ollama, vLLM, OpenRouter), comme dans Talixio AI Assistant. | ✅ |
| A4 | Exécutable de petite taille. | ✅ profil de compilation optimisé en taille |
| A5 | Peut être installé en service ou en démon sur une machine. | 🟡 `talixio-agent service install` (décision 28, v0.11.0) : le maître repart au démarrage de la machine et après une panne. Linux : unité systemd, vérifiée par `systemd-analyze`, pas encore essayée sur une machine qui redémarre. macOS (launchd) et Windows (tâche planifiée à l'ouverture de session, sans relance) : écrits, non essayés. Les agents déclarés permanents sont recréés au redémarrage (décision 30). |
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
| G5 | Déploiement d'un enfant : sur la machine, dans Docker, par SSH, ou par un autre moyen d'accès distant. | 🟡 local, SSH et Docker (le maître seul déploie hors de sa machine ; même processeur ; exécutable statique pour ne pas dépendre de la distribution). Essayé avec un faux `ssh` ; pas encore sur une vraie machine ni un vrai démon Docker |
| G6 | Lien chiffré et authentifié dès qu'il quitte la machine (port HTTPS ou TCP ouvert par le parent). | ✅ TLS 1.3 partout (lien et API) ; l'enfant n'accepte que le certificat de son parent, épinglé au lancement ; authentification mutuelle liée à ce certificat |
| G7 | Lien coupé : l'enfant se met en veille ; si le parent ne revient pas, il s'éteint avec sa descendance. Jamais d'enfant autonome sans parent. | ✅ |
| G8 | Aucun message perdu pendant une coupure. | ✅ numérotation et acquittement |
| G9 | Un parent peut arrêter un enfant. | ✅ |
| G10 | Un parent peut désinstaller un enfant de la machine où il l'a déployé. | ✅ à l'arrêt de l'enfant ou du maître : exécutable, base et journal retirés de la cible ; conteneur supprimé |
| G11 | Chaque agent a un nom et un rôle, et déclare sa machine, son OS et son adresse IP. | ✅ |
| G12 | Un parent ne conclut pas sa mission tant qu'un de ses enfants travaille encore : il attend son compte rendu, ou l'arrête d'abord. Sinon il rendrait un résultat qu'il n'a pas, et son propre parent arrêterait la branche en plein travail. | ✅ `finish_mission` refusé avec le nom des enfants au travail ; test dans `tests/tree.rs` |

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
| B6 | Budgets et consommation visibles dans l'interface, agent par agent. | 🟡 tokens et appels de service (utilisés / permis, par nature) dans `/api/tree` et la fiche de chaque agent ; reste : l'alerte et le reste en argent |

### Fournisseurs, modèles et capacités

Demandé le 2026-11-20, sur le modèle de Talixio AI Assistant. Rien n'est codé.

| N° | Exigence | État |
|---|---|---|
| F1 | Le maître tient un catalogue de fournisseurs ; chaque fournisseur offre des modèles de plusieurs natures : texte (LLM), image, voix, et d'autres. | 🟡 Catalogue embarqué (`catalog.toml`, repris de l'assistant : 14 fournisseurs, 61 modèles, prévision et décision comprises, avec pays, Europe ou non, niveau de confiance, nature, qualité de 1 à 10, prix indicatifs), servi par `GET /api/models`. Écran « Fournisseurs » : une table, connecté ou non. Écran « Modèles » : par fournisseur, activé (alias du maître, qui l'utilise, à qui il est accordé) ou non. Un modèle configuré est rattaché à son fournisseur par la machine qui le sert ; jamais la clé. Chaque modèle du catalogue dit s'il est appelable (le maître a un connecteur pour sa nature chez ce fournisseur) et comment le déclarer (`provider`, `nature`). Depuis la v0.10.0, on se connecte à un fournisseur et on active ses modèles depuis la page, sans toucher à la configuration ni redémarrer (décision 26) : clé collée une fois, ou autorisation dans le navigateur quand le fournisseur en offre une sans préparation (OpenRouter). `catalog_file` remplace le catalogue embarqué. Reste : Infomaniak, TypeSafe AI et Nixtla, qui se déclarent encore dans la configuration (adresse dépendante du compte, ou pas de connecteur) ; mise à jour du catalogue depuis les fournisseurs ; essai chez un vrai fournisseur. |
| F2 | Un agent reçoit le droit d'utiliser tel modèle de tel fournisseur, nature par nature. Ses enfants n'en reçoivent jamais plus. | ✅ `capabilities.models` (les alias, toutes natures) et `capabilities.max_calls` (nombre d'appels par nature de service : une nature absente vaut zéro). Un enfant reçoit son modèle de conversation, plus les seuls modèles de service que son parent lui confie nommément, et jamais plus d'appels que lui (tests). |
| F3 | À la création d'un enfant, le parent choisit son modèle, son niveau d'effort, la taille de son contexte, ses instructions, et ses capacités (texte, image, voix), en plus de ses droits sur les dossiers et les commandes. | 🟡 modèle, instructions, droits, et capacités de service (`models`, `max_calls` dans `spawn_agent`) ; l'effort et le contexte sont fixés par l'alias, pas par enfant |
| F4 | Les appels d'image et de voix passent par le maître, comme ceux de texte : les clés ne descendent jamais, et chaque appel est compté dans le budget. | 🟡 Fait pour sept natures : transcription, synthèse vocale, image, plongements (recherche par le sens dans la mémoire), prévision, décision, et lecture d'image par un modèle de texte (`vision`). Les appels remontent par le lien jusqu'au maître (décision 25), comptés en appels par nature à chaque niveau. Revue de sécurité indépendante faite (§7, quatrième passe). Testé avec de faux fournisseurs, de bout en bout par un vrai enfant ; **aucun essai encore chez un vrai fournisseur** (pas de clé dans l'environnement de développement). Reste : compter en argent (B4), une image rendue par adresse (refusée aujourd'hui), le son en flux. |

### Répertoire des rôles et apprentissage

Demandé le 2026-11-20. Rien n'est codé.

| N° | Exigence | État |
|---|---|---|
| R1 | Chaque rôle créé dans la grappe (nom, rôle, instructions, droits demandés, modèle, contexte de la mission) remonte au maître, qui le range dans un répertoire qui dure. | 🟡 le maître inscrit dans sa base chaque agent vu dans l'arbre (nom, rôle, modèle, parent, niveau, cible, droits accordés, dates), à tout niveau ; `GET /api/roles`, écran « Rôles ». Mission, critère et consignes ne sont connus que pour les enfants directs du maître : pour les niveaux plus bas, ils ne remontent pas encore (Q11) |
| R2 | Tout agent, à n'importe quel niveau, peut interroger ce répertoire avant de créer un enfant, pour reprendre un rôle qui existe déjà et convient au contexte. | ⬜ |
| R3 | Le répertoire apprend : pour chaque rôle, ce que ses missions ont donné (réussie, échouée, impossible, coût), pour que les rôles qui marchent soient préférés et les autres corrigés. | ⬜ |

### Stockage et mémoire

| N° | Exigence | État |
|---|---|---|
| S1 | Chaque agent a un endroit à lui pour travailler et déposer ses fichiers. | ❓ voir Q7 |
| S2 | Chaque agent a une petite mémoire qui dure : ses consignes, ce qu'il a appris, où il en est. Elle survit à un redémarrage. | 🟡 chaque agent a sa base SQLite et y tient ses notes (`memory_write`, `memory_read`), relues dans son prompt au démarrage. Celle du maître survit aux redémarrages ; celle d'un enfant ordinaire dure le temps de sa vie ; celle d'un agent permanent le suit d'une vie à l'autre (décision 30, test de bout en bout) |
| S3 | Le journal et l'état de la grappe survivent à un redémarrage du maître. | 🟡 journal sur disque (SQLite), repris au démarrage ; les agents permanents du maître sont recréés (décision 30) ; les agents ordinaires et les conversations ne le sont pas |

### Pilotage

| N° | Exigence | État |
|---|---|---|
| P1 | API JSON au niveau du maître, utilisable par Talixio AI Assistant. | ✅ |
| P2 | Interface web soignée, faite avec la bibliothèque shadcn/ui personnalisée, à l'identité propre : la grappe en direct, chaque agent avec nom, rôle, machine, IP, état ; le fil des messages ; le dialogue avec le maître ; les commandes et à qui elles sont confiées ; les fournisseurs et modèles ; les rôles. | ✅ servie par le maître à `/` (React, shadcn/ui, Tailwind ; décision 22) : Grappe, Dialogue, Journal, Commandes, Modèles, Rôles, Nouveautés ; clair et sombre ; téléphone. Reste : revue de sécurité indépendante de l'interface |
| P3 | Vue de la grappe et du journal en ligne de commande. | ✅ |
| P5 | Notes de version, tenues à chaque version, lisibles en ligne de commande et dans l'interface web. | 🟡 `releases.toml` embarqué, `talixio-agent releases`, `GET /api/releases` ; écran « Nouveautés » de l'interface |
| P6 | Tâches planifiées : le maître lance seul un travail régulier ; une page les montre toutes et permet de les créer, modifier, lancer, suspendre et supprimer. | ✅ décision 33 ; page « Tâches » |
| P4 | Pilotage depuis un téléphone (Android) : discuter avec le maître, voir la grappe, être prévenu. | 🟡 l'interface s'adapte au téléphone et porte un manifeste d'application ; l'installation demande un certificat reconnu ; pas encore de notification |

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
9. **Tout est chiffré, sans autorité de certification à gérer.** Chaque agent qui écoute a son
   certificat, signé par lui-même ; le lien et l'API sont en TLS 1.3, jamais en clair. L'enfant
   reçoit au lancement l'empreinte du certificat de son parent et refuse tout autre
   correspondant. La preuve du secret partagé inclut cette empreinte : elle ne vaut que sur
   cette connexion, ce qui exclut l'écoute comme le relais. Le certificat du maître est gardé
   dans le dossier d'état (son empreinte sert à la ligne de commande) ; l'exploitant peut
   fournir le sien (`[tls]`), d'une autorité reconnue, pour les navigateurs.
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
14. **SQLite est embarqué dans l'exécutable**, le même pour tous les agents (environ 1 Mo de plus).
    Chaque agent a sa base dans le dossier d'état : `talixio.db` pour le maître,
    `agents/<identifiant>.db` pour un enfant. Elle n'est accessible que par les outils de mémoire :
    le dossier d'état reste interdit aux outils de fichiers et de commandes.
15. **Le maître tourne sur un serveur toujours allumé** (une VM), pas sur un poste. Le poste et le
    téléphone ne servent qu'à l'interface.
16. **Le début de la conversation d'un agent ne change jamais** : consignes, droits et mémoire
    sont figés au démarrage, et toute information nouvelle (note de mémoire, message, résultat)
    s'ajoute à la fin. C'est la condition pour que le cache du fournisseur de modèles serve :
    modifier le début ferait repayer toute la conversation à chaque appel.
17. **Hors de la machine, c'est la cible qui borne les droits** : chaque cible de déploiement
    (`[targets.<nom>]`, SSH ou Docker) porte son plafond de dossiers et de commandes, écrit par
    l'humain dans la configuration du maître. Un enfant déployé ne reçoit jamais plus que ce
    plafond, quoi que demande son parent, et il le revérifie à l'arrivée sur les chemins réels de
    sa machine. Modèles, budget et descendance restent bornés par le parent. Seul le maître
    déploie sur une cible ; les descendants d'un agent déployé naissent sur sa machine.
18. **SSH : rien d'interactif, rien de deviné.** Machine déjà connue exigée (jamais de confiance
    à la première vue), pas de transfert d'agent ni de port ; l'exécutable et les consignes
    passent par l'entrée standard de `ssh`, jamais par la ligne de commande. Tout ce qui vient de
    la configuration d'une cible est validé caractère par caractère : rien ne peut y passer pour
    une option ou une commande.
19. **Docker : conteneur jetable et fermé.** Aucune capacité, pas de nouveaux privilèges, système
    en lecture seule, mémoire et process bornés, état en mémoire, jamais root, image jamais
    téléchargée d'office, seuls les montages de la cible.
21. **L'exécutable de déploiement est statique** (cible `x86_64-unknown-linux-musl`) : aucune
    bibliothèque du système, donc aucune dépendance à la glibc ni à la distribution de la machine
    d'arrivée. Tout ce qui est en C (SQLite, la cryptographie de TLS) est compilé dedans. Essayé :
    5,3 Mo contre 5,2 Mo en dynamique, tous les tests passent. Pas de paquets Debian : un
    exécutable seul reste plus simple à déposer et à retirer. Prix à connaître : l'allocateur de
    musl est plus lent sous forte charge (sans effet pour un agent, qui attend surtout son
    modèle), et la résolution de noms ignore NSS (pas de LDAP ni de mDNS ; `/etc/hosts` et DNS
    seulement).
22. **L'interface web est une application React faite avec shadcn/ui** (`web/` : Vite, Tailwind,
    composants shadcn sur Base UI), à la demande de l'utilisateur, avec un thème propre à Talixio
    (`web/src/index.css` : papier chaud, encre verte, un seul signal vert acide réservé à ce qui
    vit ou à la page ouverte ; Instrument Sans, et JetBrains Mono pour les seules données).
    Le résultat construit (`web/dist/`, noms de fichiers fixes, polices comprises) est **gardé au
    dépôt** et embarqué dans l'exécutable : `cargo build` n'a toujours pas besoin de Node, et
    l'intégration continue vérifie que `dist/` correspond bien aux sources. Elle est servie sans
    jeton (elle ne contient rien de secret) ; toute donnée passe par l'API, avec le jeton saisi
    par l'utilisateur, gardé dans le navigateur (session, ou appareil s'il le demande).
    Politique de contenu stricte, inchangée : aucun script ni style en ligne, aucune ressource
    extérieure (polices embarquées), jamais dans un cadre ; tout texte venu de la grappe est posé
    comme du texte (React l'échappe ; `dangerouslySetInnerHTML` est proscrit). Coût : environ
    500 Ko de plus dans l'exécutable. Les dépendances npm ne servent qu'à la construction ; ce
    qui en reste dans `dist/` (React, Base UI, icônes) s'exécute dans le navigateur de
    l'exploitant, sous la politique de contenu.
23. **La configuration d'un enfant est scellée.** Au lancement, l'enfant ne reçoit en clair que
    de quoi joindre son parent (adresse, empreinte du certificat, identifiant, secret du lien).
    Tout le reste (nom, rôle, instructions, modèle, droits, plafond, interdits, limites) est
    chiffré et signé d'un bloc (ChaCha20-Poly1305), avec une clé neuve par enfant que seul le
    parent détient. L'enfant va la chercher sur le lien chiffré : le parent prouve d'abord qui il
    est (certificat épinglé, puis secret), l'enfant ensuite, et la clé n'est remise qu'une fois.
    L'enveloppe est liée à l'identifiant de l'enfant, au certificat et à l'adresse du parent.
    Ce que cela garantit : la configuration ne peut être ni lue ni modifiée en chemin ou au
    repos (tube, canal SSH, entrée du conteneur, et demain fichier d'un agent permanent), et un
    agent ne démarre jamais avec autre chose que ce que son parent a écrit. Ce que cela ne
    garantit pas : un process d'agent dont le code serait remplacé ou détourné n'est tenu par
    aucun chiffrement ; ce qui le borne alors, c'est ce que le parent vérifie lui-même (modèles,
    budget, taille de l'arbre, messages) et le cloisonnement par le système (D5).
24. **Les journaux sont bornés par le parent.** `log_max_bytes` (maître) descend dans la
    configuration scellée. Le parent fait tourner les journaux de ses enfants locaux et de ses
    conteneurs ; un agent déployé par SSH fait tourner le sien. Le journal SQLite du maître
    garde ses 200 000 derniers événements.
25. **Les modèles de service passent par le maître, comptés en appels.** Un modèle a une nature :
    `llm` (défaut ; `vision = true` s'il lit les images), `transcription`, `speech`, `image`,
    `embedding`, `forecast`, `decision`. Le maître parle quatre familles d'API : `openai`
    (transcription, voix, image, plongements, chez tout serveur compatible), `elevenlabs` (voix),
    `timesfm` (prévision : `POST /forecast`), `systemone` (décision : `POST /v1/systemone`).
    Un agent demande un service à son parent (`service_request`) ; chaque niveau revérifie
    l'alias, les bornes de la demande et le quota, et seul le maître appelle le fournisseur.
    - **Le quota se compte en appels, par nature** (`capabilities.max_calls`), descendance
      comprise. Une nature absente : aucun appel, même si le modèle est accordé. On avait d'abord
      pensé compter des minutes et des images : la durée d'un son ne se mesure pas de façon sûre
      avant l'appel, alors que chaque appel est borné en taille. Un appel parti chez le
      fournisseur est compté, abouti ou non (le fournisseur l'a peut-être facturé) ; seul un
      refus avant tout envoi est gratuit. Un appel de service n'est jamais retenté, ni par le
      maître, ni après une coupure du lien ; l'attente d'une réponse est bornée à 11 minutes.
    - **Bornes** : son ou image de 8 Mio au plus ; image à lire de 4 Mio ; texte à dire ou à
      dessiner de 4 000 caractères ; 64 textes de 8 000 caractères par appel de plongements,
      vecteurs de 4 096 dimensions au plus ; série de 3 à 5 000 valeurs, horizon de 256 pas ;
      état de 60 000 caractères et 20 questions pour une décision ; quatre appels de service
      simultanés par enfant.
    - **Rien ne sort de l'adresse configurée.** Le maître ne suit aucune redirection d'un
      fournisseur, pour le texte comme pour les services. Voix, langue, taille et nom de fichier sont
      réduits à des mots simples avant de former une adresse ou un formulaire. Une image que le
      fournisseur rend par adresse n'est pas suivie. La réponse est lue sous une borne, et
      vérifiée : une image doit en être une (PNG, JPEG, WebP), un son aussi (premiers octets),
      une décision ne garde que les
      questions posées, les options proposées et des nombres bornés.
    - **Ce que rend un service est une donnée** : texte transcrit nettoyé, et dit comme tel à
      l'agent. Le détail d'une panne de fournisseur reste au journal du maître.
    - **Seul un son part en transcription** (premiers octets reconnus), pas n'importe quel
      fichier lisible. Les images jointes à une demande de texte sont revérifiées à chaque
      niveau : huit au plus, 4 Mio chacune, format connu, modèle `vision` exigé.
    - **Un enfant n'a qu'un modèle de conversation** ; `spawn_agent` ne lui confie en plus que
      des modèles de service (ou `vision`).
    - **Les fichiers passent par les droits de l'agent** : le son ou l'image à lire dans ses
      dossiers lisibles, le son ou l'image produits dans ses dossiers inscriptibles, vérifiés
      avant de dépenser un appel.
    - **Recherche par le sens** (`memory_search`) : les notes de l'agent partent chez le
      fournisseur de plongements. Les vecteurs sont gardés dans sa base (table `vectors`) : seules
      les notes nouvelles ou modifiées, et la question, coûtent un appel.
26. **On se connecte à un fournisseur et on active un modèle depuis l'interface, à chaud.**
    (`src/connect.rs`, v0.10.0.)
    - **Où va la clé** : seulement à l'adresse que le catalogue donne pour ce fournisseur. La
      page ne choisit jamais une adresse ; le maître ne suit aucune redirection. La clé est
      d'abord essayée (liste des modèles du fournisseur) : refusée (401, 403), elle n'est pas
      gardée ; ni oui ni non (pas de liste à cette adresse), elle est gardée et dite « non
      vérifiée ».
    - **Où elle reste** : `<state_dir>/keys/<fournisseur>.key`, fichier créé vide et fermé au
      compte du maître avant d'être rempli. Le dossier d'état est interdit aux outils des
      agents. Aucune route ne la rend ; le journal dit « fournisseur connecté », jamais la clé.
      Ce qui est connecté et activé (sans secret) est dans `<state_dir>/connections.json`,
      repris au démarrage.
    - **Activer** un modèle d'un fournisseur connecté lui donne un alias et l'ajoute aux
      droits du maître, qui peut alors l'accorder ; pour un modèle de service, on donne le
      nombre d'appels de sa nature (un million au plus). Un alias déjà pris est refusé :
      l'interface ne masque jamais un modèle de la configuration. **Désactiver** ou
      **déconnecter** ne touche que ce qui a été fait depuis l'interface, et se refuse tant
      qu'un agent en vie détient le modèle. La configuration reste la base ; l'interface n'y
      écrit pas.
    - **OAuth** quand le fournisseur l'offre sans application à enregistrer (aujourd'hui
      OpenRouter, PKCE S256) : le secret de vérification ne quitte pas le maître, le retour
      ne peut viser que la page d'où vient la demande, un code ne sert qu'une fois et dans
      les dix minutes. Ailleurs (OpenAI, Anthropic, Gemini, Mistral, OVHcloud, AKI.IO,
      Scaleway, ElevenLabs, DeepSeek), il n'existe pas d'autorisation utilisable par un outil
      tiers sans enregistrement préalable : la page demande une clé et donne le lien vers la
      page du fournisseur où la créer. L'abonnement Claude (Pro, Max) n'est pas utilisable
      par des agents tiers : Anthropic se connecte par clé d'API.
    - **Qui peut le faire** : quiconque détient le jeton de l'API, qui pouvait déjà parler au
      maître et arrêter des agents. Il peut désormais aussi engager une dépense chez un
      fournisseur avec la clé donnée ; il ne peut ni lire une clé, ni l'envoyer ailleurs.
27. **Une version téléchargeable par système, installée en une ligne.** À chaque étiquette
    `vX.Y.Z`, l'intégration continue (`.github/workflows/release.yml`) compile un exécutable
    pour Linux (x86_64 et ARM, statiques), macOS (ARM et x86_64) et Windows (x86_64), refuse
    une étiquette qui n'est pas la version du code, et publie le tout avec `SHA256SUMS` et
    deux scripts (`scripts/install.sh`, `scripts/install.ps1`). Le script ne fait que
    télécharger en HTTPS, vérifier l'empreinte et poser l'exécutable dans un dossier de
    l'utilisateur : ni root, ni service, ni lancement. Limites : l'empreinte vient de la même
    publication que l'exécutable (pas de signature) ; les exécutables macOS et Windows ne sont
    pas signés par un certificat d'éditeur, le système peut demander une confirmation au
    premier lancement ; un dépôt privé ne se télécharge pas sans compte.
28. **Le maître s'installe en service du système.** `talixio-agent service install` écrit la
    définition du service et le démarre ; `service uninstall` la retire sans toucher à la
    configuration, aux clés ni au dossier d'état ; `service status` montre son état ; `--print`
    montre ce qui serait fait, sans rien faire.
    - **Linux** : unité systemd du compte (`~/.config/systemd/user/`), relancée après une
      panne (`Restart=on-failure`), sans gain de privilèges possible (`NoNewPrivileges`).
      Lancée en root, la commande exige `--user <compte>` et écrit une unité du système qui
      tourne sous ce compte : le maître ne tourne jamais en root.
    - **macOS** : agent launchd du compte, relancé après une panne ; il démarre à l'ouverture
      de la session.
    - **Windows** : tâche planifiée à l'ouverture de session, sans droits élevés ; pas de
      relance après une panne (un vrai service Windows reste à faire).
    - **Aucun secret dans la définition.** Les clés restent dans leurs fichiers (`api_key_file`,
      ou saisies dans l'interface) ; sous Linux, `--env-file` désigne un fichier
      d'environnement fermé aux autres comptes. La configuration est relue avant
      l'installation : un service ne démarre pas en boucle sur une configuration fausse.
    - Les chemins entrent dans la définition tels quels : un chemin relatif, ou portant un
      guillemet, un retour à la ligne, `%`, `$`, `<`, `>`, `&`, `;` ou `|` est refusé.
    - En unité du système, systemd relit ces chemins en root à chaque démarrage : l'exécutable,
      la configuration, le dossier de travail et tous leurs dossiers parents doivent
      appartenir à root ou au compte du service, sans écriture pour le groupe ni les autres ;
      le fichier d'environnement, à root seul. Le compte ne doit pas avoir l'uid 0. L'unité du
      système retire toutes les capacités et ferme le noyau, les groupes de contrôle, l'horloge
      et le nom de machine.
    - **Ce qui revient au redémarrage** : le maître, son journal, sa mémoire, le répertoire
      des rôles, les fournisseurs connectés, les modèles activés, et ses agents permanents
      (décision 30). **Ce qui ne revient pas** : les agents ordinaires et les conversations.
    - Le journal d'activité des agents suit la sortie du maître : journald sous Linux,
      `~/Library/Logs` sous macOS. Il ne porte ni clé ni jeton, mais ce que les agents
      traitent s'y lit : le protéger comme le reste.
29. **Une image Docker minimale pour le maître** (`Dockerfile`). L'image finale part de rien
    (`FROM scratch`) et ne contient que l'exécutable statique (6,5 Mo) et une configuration par
    défaut (`docker/talixio.toml`) : ni système, ni shell, ni bibliothèque. Le maître y tourne
    sous un compte sans droit (65532), jamais root. `/data` porte ce qui dure : `/data/work`,
    seul dossier ouvert aux agents, et le dossier d'état. Aucune commande n'est accordée, et
    l'image n'en contient aucune. Le jeton de l'API se donne par `TALIXIO_API_TOKEN` (pas de
    shell pour aller le lire). C'est la façon la plus sûre d'essayer : ce que fait un agent
    reste dans le conteneur et son volume. Limites : les enfants créés là tournent dans le même
    conteneur, sous le même compte ; les cibles SSH et Docker n'y sont pas utilisables (pas de
    client dans l'image). Essayé : l'exécutable seul dans une racine vide, sous le compte 65532
    (démarrage, TLS, interface, API, arrêt propre). L'image elle-même n'a pas pu être
    construite dans l'environnement de développement (pas de démon Docker).
30. **Les agents permanents reviennent après un redémarrage du maître.** `spawn_agent` accepte
    `permanent: true`, chez le maître seulement. Le maître garde alors la demande de création
    dans sa base (table `permanent`). À son démarrage, il rejoue chaque demande par le chemin
    d'une création ordinaire : tous les contrôles sont refaits avec les droits qu'il a à ce
    moment-là, rien n'est repris tel quel. Une demande qui ne passe plus est signalée au
    journal et gardée pour le démarrage suivant.
    - L'agent garde son **identifiant**, donc sa base et sa **mémoire** (cible locale). Sa
      conversation, elle, est perdue : sa mission lui est redonnée avec la consigne de relire
      sa mémoire et de reprendre où il en était ; à sa création, on lui demande d'y noter son
      avancement. La reprise repose donc sur ce que l'agent a noté.
    - Un agent arrêté à la demande (interface, `stop_agent`) n'est plus permanent. Un agent
      perdu (process mort, lien coupé) le reste : il revient au prochain démarrage du maître.
    - Limites : seuls les enfants directs du maître ; un agent déployé ailleurs (SSH, Docker)
      est redéployé, sans sa mémoire restée là-bas ; le compteur de tokens repart de zéro à
      chaque vie, donc un maître qui redémarre en boucle refait dépenser ses agents permanents
      (sous systemd, dix démarrages en cinq minutes au plus) ; l'interface ne montre pas
      encore qu'un agent est permanent ; 64 agents permanents au plus.
31. **Le maître reçoit son modèle comme un autre agent.** Le maître est un agent comme les
    autres, avec plus de moyens, jamais moins : ce qui se fait pour un enfant depuis
    l'interface se fait aussi pour lui. `model` et `[models]` sont facultatifs dans la
    configuration. Sans `model`, le maître démarre sans modèle ni clé ; son interface répond,
    et un message qu'on lui envoie reçoit pour toute réponse la marche à suivre, sans aucun
    appel. Le premier modèle de conversation activé depuis l'interface devient le sien ;
    `POST /api/models/{alias}/default` lui en donne un autre, parmi ceux qu'il peut déjà
    appeler et qui tiennent une conversation (activés d'ici ou déclarés dans la
    configuration). Le choix est gardé dans `connections.json` et repris au démarrage s'il
    est encore dans ses droits ; sinon le maître repart sans modèle, il ne retombe jamais sur
    un autre de lui-même. Désactiver son modèle, ou déconnecter son fournisseur, le laisse
    sans modèle, et le journal le dit.
    - La configuration garde le dernier mot : un `model` écrit là ne se change pas d'ici.
    - Un fournisseur que le catalogue dit `keyless` (OVHcloud) se connecte sans clé : accès
      anonyme, limites basses. Sans clé, aucun en-tête d'autorisation ne part (une fausse
      clé est refusée). Avec l'image Docker, on essaie donc le maître sans compte nulle part.
    - D'abord écarté, puis fait autrement (décision 32) : la connexion par abonnement (Claude Pro/Max,
      ChatGPT). L'assistant la fait pour Claude en pilotant la commande officielle `claude`,
      qui doit alors être installée à côté du maître et faire elle-même chaque appel ; le
      jeton d'abonnement ne s'emploie pas en appelant l'API directement. Incompatible avec
      l'exécutable seul et l'image vide.
    - Changer de modèle en cours de conversation envoie la conversation en cours (résultats
      d'outils compris) au nouveau fournisseur : c'est le geste de la personne qui a le jeton.
32. **L'abonnement Claude passe par la commande officielle `claude`, jamais par l'API.**
    Fournisseur `claude-abonnement` du catalogue, connecteur `claude-code`. Le maître lance
    `claude -p`, non modifié, sous son propre compte ; c'est `claude` qui se connecte (sa
    connexion sur la machine, ou `CLAUDE_CODE_OAUTH_TOKEN` tiré de `claude setup-token`). Le
    maître ne lit, ne garde ni ne transmet aucun jeton d'abonnement, et ne propose pas lui-même
    la connexion à claude.ai : c'est ce que les règles d'Anthropic demandent. Talixio reste un
    exécutable seul : `claude` est un programme externe facultatif, comme `ssh` et `docker`.
    - `claude` tient la boucle d'outils, mais l'interface reste celle des autres connecteurs
      (une demande, une réponse), donc les contrôles aussi. Ses outils intégrés, réglages,
      extensions et serveurs d'outils du compte sont coupés (`--tools ""`,
      `--strict-mcp-config`, `--setting-sources ""`, `--permission-mode dontAsk`). Il n'a que
      les outils de l'agent, servis par un pont (`talixio-agent mcp-bridge`) qu'il lance et
      qui rappelle le maître (`POST /bridge`, jeton tiré pour ce tour). Un appel d'outil est
      retenu et rendu à l'agent comme tout autre ; l'agent l'exécute avec ses droits ; sa
      demande suivante libère l'appel.
    - Une session `claude` par agent et par modèle, reprise d'un tour à l'autre. Sa clé est
      liée, à chaque niveau de l'arbre, à l'enfant reconnu par le lien : un agent ne peut pas
      prendre le tour d'un autre. Si la session est perdue (maître relancé, tour abandonné),
      la conversation est redonnée en clair à une session neuve.
    - `claude` reçoit un environnement réduit : jamais les clés du maître ni le jeton de son
      API ; `ANTHROPIC_API_KEY` est toujours retirée (elle passerait avant l'abonnement).
      Consignes et jeton du pont passent par des fichiers fermés du dossier d'état, pas par
      la ligne de commande.
    - Limites : pas d'images ; un process `claude` par tour actif (quelques centaines de Mo) ;
      un tour qui attend un outil plus de cinq minutes est abandonné et repris dans une
      session neuve ; les tokens d'un tour abandonné ne sont pas comptés ; l'image Docker
      vide ne peut pas l'héberger (`docker/Dockerfile.claude`) ; `claude` garde ses propres
      transcriptions dans le dossier personnel du compte du maître.
    - Garde-fous à l'exécution (huitième passe) : le maître lit ce que `claude` annonce avoir
      en main et tue le tour s'il y a autre chose que les outils de l'agent, ou si `claude`
      appelle autre chose ; seize process au plus, quatre par branche (un enfant du maître et
      sa descendance), deux gardés au maître ; une session n'est reprise que si l'empreinte du
      début de la conversation (consignes comprises) est la même ; `~/.claude`,
      `~/.claude.json` et `CLAUDE_CONFIG_DIR` sont interdits d'office à tous les agents.
    - Ce que `claude_env` laisse passer est de la responsabilité de l'exploitant : une variable
      comme `ANTHROPIC_BASE_URL` change le service que `claude` appelle. Le détenteur du jeton
      d'un tour (le pont, donc `claude`) peut poser des appels d'outils pour ce tour et en
      reçoit les résultats, rien d'autre.
33. **Tâches planifiées : une consigne de la personne, écrite d'avance.** Le maître la reçoit à
    heure fixe (chaque jour, chaque semaine, toutes les N heures) comme un message ordinaire :
    elle ne lui donne aucun droit de plus. Créées, modifiées, lancées et supprimées par l'API de
    pilotage seulement (page « Tâches ») ; gardées dans la base du maître ; aucun outil ne
    permet à un agent d'en créer ni d'en changer. Trente-deux au plus. L'heure est celle de la
    personne (décalage du navigateur, fixé à la création : il ne suit pas le changement d'heure).
    Une échéance manquée pendant un arrêt part une fois au retour. Pas de dépendance ajoutée :
    le calcul des dates est fait à la main (`src/schedule.rs`).
34. **Un exemple complet vit dans le dépôt** (`examples/catalogue`, et depuis la décision 37 le
    paquet `packs/catalogue` pour ses scripts et ses rôles) : une équipe de trois agents
    aux droits opposés (collecteur avec le web, rédacteur avec l'écriture, vérificateur en
    lecture seule) tient à jour le catalogue des modèles. Tout y est de la configuration : règles
    de commandes, script `lire-web` qui borne le web à une liste de sites, consigne à planifier.
    Deux commandes de l'exécutable le servent : `catalog show` (le catalogue embarqué) et
    `catalog check <fichier>` (validité, et ce qui change), qui donne au vérificateur un critère
    mesurable. Le résultat reste une proposition : c'est une personne qui le copie dans le dépôt.
35. **Le navigateur vit dans le conteneur de l'agent qui s'en sert, jamais à côté du maître**
    (demandé après la première passe réelle, où aucune grille de prix officielle n'avait pu être
    lue ; d'abord mis dans l'image du maître, puis retiré à la demande : « au lieu de polluer
    celui du maître »). Le script `lire-page` de l'exemple pilote Chromium par Playwright, avec
    les bornes de `lire-web` : la page et ses cadres sur les sites de la liste, ce qu'elle charge
    sur des noms publics en https, ni image ni téléchargement ni connexion permanente, profil
    jeté, 60 s. L'exécutable ne change pas et ne dépend de rien : l'image `talixio-navigateur`
    (`docker/Dockerfile.navigateur`, `./tools.sh build navigateur`) ne contient que Chromium et
    son pilote ; le maître y lance l'agent par une cible `docker` (décision 19), qui reçoit deux
    réglages : `memory`, et `seccomp`, le chemin d'un filtre d'appels système
    (`docker/chromium-seccomp.json` : celui de Docker plus ce que le bac à sable de Chromium
    demande). Le navigateur exécute ainsi les scripts des pages dans un conteneur jetable, en
    lecture seule, sans capacité, qui ne voit que ses scripts et son dossier de relevés.
    Limites : ce conteneur garde le réseau (limite 8) ; un nom public qui change d'adresse entre
    le contrôle et la connexion n'est pas arrêté ; un site peut refuser un navigateur piloté ;
    un maître lui-même dans Docker ne peut pas encore lancer de conteneur (Q12).
36. **Un maître en conteneur lance ses agents dans des conteneurs voisins** (demandé :
    « créer des Docker avec des agents dedans, avec un contexte isolé »). Sur option explicite
    (`TALIXIO_DOCKER=1 ./tools.sh up claude`), le maître reçoit le socket Docker de la machine,
    qu'il ouvre par le groupe qui le détient, et tourne avec ses agents dans un réseau Docker à
    part. La cible `docker` reçoit ce qu'il faut pour ne dépendre d'aucun chemin de la machine,
    que le maître en conteneur ne connaît pas : `network` (l'agent joint le maître dans ce
    réseau), `agent_in_image` (l'exécutable de l'agent est dans l'image, construite à partir de
    celle du maître), et des montages par `volume` et `subpath` (un sous-dossier du volume du
    maître, jamais le volume entier, où sont ses clés : `subpath` est obligatoire, et jamais
    sous un dossier qu'un autre conteneur écrit). Le conteneur d'un agent reste celui de
    la décision 19 : jetable, lecture seule, aucune capacité, jamais le socket. **Prix, accepté
    pour un premier temps** : qui prend le process du maître commande Docker, donc la machine
    en root (limite 9) ; cela vaut pour tout ce que le maître lance sous son compte (agents à
    côté de lui, commandes permises, `claude`), d'où : aucune règle de commande à arguments
    libres chez un tel maître. Le socket est interdit d'office aux outils des agents, aucune règle de
    commande ne donne `docker` par défaut, et le client Docker sans socket ne peut rien. À
    terme : le maître dans une machine virtuelle. Non essayé : Docker sans droits
    d'administrateur (« rootless »), où le compte sans droit du conteneur n'ouvre sans doute
    pas le socket.
37. **Les paquets : commandes et rôles installés par l'exploitant, jamais par le maître**
    (demandé : « un CLI qui permettrait d'ajouter les commandes, les rôles […] à partir d'un
    catalogue », « indépendant du maître », « on vérifie […] qu'on n'a rien modifié »). Un paquet
    est un dossier : un manifeste (`pack.toml`), des scripts, des textes. Il donne des commandes
    (un script du paquet, avec ses arguments permis, jamais libres) et des rôles (consignes,
    commandes employées, modèle et cible conseillés). `talixio-agent pack list | show | install |
    remove | verify` : les paquets viennent de l'exécutable (le paquet « catalogue », ancien
    exemple) ou d'un dossier (`--from`) ; à terme, d'un service de téléchargement.
    - **Séparation** : le maître n'a aucun outil ni aucune route pour installer. Il lit le
      dossier (`packs_dir`) à son démarrage, et **refuse de démarrer** si ce dossier, un de ses
      fichiers ou un dossier au-dessus lui appartient ou lui est inscriptible, hors montage en
      lecture seule (`packs_unprotected = true` pour un essai) ou si un fichier ne répond plus à l'empreinte
      SHA-256 notée à l'installation (`pack.lock`). Le dossier est interdit à ses outils de
      fichiers. Dans Docker : `./tools.sh pack …` écrit le dossier sur la machine, à votre
      compte ; le maître le reçoit monté en lecture seule.
    - **Revérifié à chaque appel** : la règle d'une commande de paquet porte l'empreinte de son
      programme (`sha256`), qui suit la règle de parent en enfant sans que le modèle la recopie
      et ne peut pas être retirée en route ; l'agent qui lance la commande, où qu'il soit,
      relit le programme et refuse s'il a changé. Une empreinte ne s'écrit pas dans la
      configuration.
    - **Identifiant** (demandé : « les identifier avec un identifiant unique », pour qu'on ne
      contourne pas par le nom) : un paquet a pour identifiant l'empreinte SHA-256 de tout son
      contenu (SHA-1 et MD5 ne résistent plus à une modification voulue). `pack show` et
      `pack verify` le donnent. La configuration du maître peut fixer ses paquets par cet
      identifiant (`[packs] catalogue = "…"`) : le maître ne démarre alors qu'avec exactement
      ces paquets, à ce contenu ; un paquet du même nom et d'un autre contenu est refusé. C'est
      l'ancre hors du dossier des paquets, qui manquait à `pack.lock`. Un rôle se nomme
      « paquet/rôle », une commande porte l'empreinte de son programme : tous deux tiennent
      donc à l'identifiant du paquet.
    - **Rôles** : le maître voit les rôles installés dans ses consignes ; `spawn_agent` avec le
      nom complet d'un rôle installé (« paquet/rôle ») donne à l'enfant les consignes et les commandes du rôle. Dossiers,
      budget et mission restent choisis par le maître, dans ses propres droits.
    - Non fait : interface de navigation (TUI), service de téléchargement et signature des
      paquets (l'empreinte protège ce qui est installé, pas ce qu'on installe : `pack show`
      d'abord), cibles de déploiement dans un paquet, rôles installés dans la page « Rôles ».
20. **Les notes de version ont une source unique**, `releases.toml`, embarquée dans l'exécutable.
    Chaque version y ajoute une entrée ; un test refuse une version sans note.

38. **Le compte dédié : ce que le maître lit est à root** (2026-11-22, demandé : « mettre un
    utilisateur talixio-agent […] et tous les fichiers à root ; le maître ne pourrait pas modifier
    les fichiers »). `sudo talixio-agent service install [--config f.toml]`, en root et sans
    `--user`, met tout en place sous Linux (systemd) :
    - le compte `talixio-agent`, créé s'il manque (`useradd --system`), sans mot de passe, sans
      shell ; refusé s'il a le numéro de root ;
    - `/opt/talixio/talixio-agent` (copie de l'exécutable) et `/opt/talixio/packs` (paquets), à
      root ; `/etc/talixio/talixio.toml`, à root, lisible par le groupe du compte (0640) ;
      `packs_dir = "/opt/talixio/packs"` y est ajouté s'il manque, un autre dossier est refusé ;
    - `/var/lib/talixio`, seul dossier qui appartienne au compte : état, clés saisies dans
      l'interface (Q6), travail ; les chemins relatifs de la configuration en partent. Root le
      crée et le donne, puis n'y écrit plus ;
    - la configuration est d'abord posée à côté et **essayée sous le compte** (`talixio-agent
      check`, nouvelle commande : charge la configuration comme le maître, paquets compris, sans
      rien lancer) ; refusée, rien n'est changé ; acceptée, l'ancienne est gardée en
      `talixio.toml.ancien` ;
    - l'unité ajoute `ProtectSystem=full` et `ReadOnlyPaths=/opt/talixio /etc/talixio` : même un
      droit de fichier mal remis ne rend pas ces dossiers inscriptibles au maître.
    Ensuite `sudo talixio-agent pack install <nom>` vise `/opt/talixio/packs` sans option (sans
    root : refus qui dit pourquoi). Modifier la configuration demande `sudo` puis un redémarrage
    du service : c'est le but. `--user <compte>` garde l'ancien mode (un compte à vous, fichiers
    là où ils sont). `--print` montre tout sans rien faire. macOS et Windows : pas encore. Dans
    Docker, l'équivalent existe déjà (dossier des paquets monté en lecture seule, décision 37).

39. **Les rôles installés se voient dans l'interface** (2026-11-22). `GET /api/roles` rend, en
    plus du répertoire des agents créés, `installed` : pour chaque rôle installé, son nom
    (« paquet/rôle »), son paquet, la version et l'identifiant de celui-ci, sa description, ses
    consignes, le modèle et la cible conseillés, ses commandes avec leur empreinte. La page
    « Rôles » les montre avant les agents créés, avec le nombre d'agents qui en sont issus. Lecture
    seule : rien ne s'installe ni ne se modifie depuis l'interface (décision 37). À la création
    d'un agent par un rôle installé, le journal note le paquet, sa version et son identifiant.

40. **Le dépôt de paquets signé** (2026-11-22, « à terme un service qui permettra d'accéder et
    de télécharger des rôles, des commandes »). Un dépôt est un dossier de fichiers statiques, à
    déposer derrière n'importe quel serveur https, sans programme côté serveur :
    `index.toml` (chaque paquet : nom, version, description, identifiant, et pour chaque fichier
    son nom, sa taille, son SHA-256), `index.toml.sig` (signature Ed25519 de l'index, précédé d'un
    préfixe de domaine), `<nom>/<version>/<fichier>`. `src/depot.rs`.
    - **Rien sans signature.** `pack list|show|install --depot https://… --key <clé publique>` :
      l'index est vérifié avant d'être analysé, contre les clés de l'exécutable (`TRUSTED_KEYS`,
      aucune pour l'instant) et celles que l'exploitant donne (`--key`, `TALIXIO_PACK_KEY`). Sans
      clé de confiance, ou sans signature valable : refus. `http://` : refus.
    - **L'hébergeur n'est pas de confiance.** Chaque fichier est vérifié contre l'empreinte de
      l'index signé, et le paquet reconstitué contre son identifiant ; un fichier changé est
      refusé et rien n'est installé. Aucune redirection n'est suivie. L'index signé est lui-même lu
      avec prudence (noms sans chemin, tailles bornées).
    - **Publier** : `pack keygen --out <fichier>` (clé privée en 0600, clé publique affichée) et
      `pack publish --from <paquets> --out <dossier> --key-file <fichier>`.
    - **Deux options, deux natures.** `--depot` (https, ou un dossier écrit par `pack publish`)
      exige une signature ; `--from` lit un dossier local tel quel, non signé, et le dit. `--key`
      ne va qu'avec `--depot` : ce que contient le dossier ou le serveur ne décide jamais de la
      nature de la source.
    - **Pas de retour en arrière silencieux.** La date de l'index est affichée ; un index daté
      dans le futur est refusé ; `pack.lock` garde la clé et la date de l'index d'où vient le
      paquet, et `install --replace` refuse un index plus ancien de la même clé (`--allow-older`
      pour le vouloir). Un index ne porte qu'une version par paquet.
    - Limites : rien n'empêche un hébergeur de cacher une mise à jour (gel) ni, à la première
      installation, de servir un ancien index ; fixer les identifiants dans `[packs]` y pare
      côté maître. Pas de révocation de clé autrement que par une nouvelle version de
      l'exécutable. Un mandataire d'entreprise qui déchiffre le https fera échouer la lecture
      (racines publiques seulement) ; l'intégrité, elle, ne repose pas sur le https. Avec
      `tools.sh pack` (conteneur sans réseau), un dépôt https ne se lit pas encore. Dépôt
      officiel : voir Q13.

41. **L'écran des paquets** (2026-11-22, « peut-être un CLI ou un TUI pour qu'on puisse un peu
    naviguer »). `talixio-agent pack tui` : la liste des paquets à gauche (✓ installé au même
    contenu, ≠ installé avec un autre contenu), ce que le paquet choisi donnerait à droite
    (le texte de `pack show`), et `i` installer, `r` retirer, `v` vérifier, `q` quitter. Rien ne
    s'installe ni ne se retire sans un « o » à la question posée. Mêmes sources (`--from`,
    `--depot` et `--key`) et même dossier (`--dir`, `TALIXIO_PACKS`, ou `/opt/talixio/packs` en
    root) que les autres commandes `pack`, dont il n'est qu'une façade (`src/tui.rs`) : mêmes
    contrôles, et rien que le maître puisse lancer lui-même. Sans dossier d'installation connu,
    il ne fait que lire. Tout texte affiché tient en une colonne par caractère et se voit (le reste
    devient « ? ») ; l'écran dit où en est la lecture et d'où viennent les paquets (signés ou non). Il tourne
    hors de Docker sur le dossier des paquets de la machine, ou par `./tools.sh pack tui` ; en
    sortant, il rappelle de relancer le maître si quelque chose a changé. Dépendance ajoutée :
    `crossterm` (compilée dans l'exécutable, toujours unique).

42. **Dépôt public en anglais, interface en deux langues** (2026-11-22, « il faut passer en
    anglais sur le repo […] un i18n […] par défaut, l'anglais et le français dans
    l'application » ; puis : sur `main`, pas les spécifications ni ce qui a été dit, « un condensé
    ou quelque chose un peu plus pro »).
    - *Deux branches.* La branche de travail garde tout. `main`, publique, ne reçoit jamais
      `docs/SPEC.md`, `PLAN.md`, `EXIGENCES.md`, `VERIFICATION.md` ni `CLAUDE.md` : leur condensé
      est `docs/DESIGN.md` (anglais : architecture, modèle de sécurité, décisions, bilan des
      revues, API, limites), à tenir à jour avec le code. `scripts/publier-main.sh` pose sur
      `main` le même arbre moins ces fichiers. L'historique de `main` a été
      réécrit avec l'accord du propriétaire (2026-11-22) : il repart d'un commit sans passé, et
      ces documents n'y figurent plus. Les anciens commits restent atteignables par la branche
      de travail, qui les contient : elle doit rester privée ou être supprimée avant d'ouvrir
      le dépôt.
    - *Anglais.* `README.md` (le français reste dans `README.fr.md`), `CONTRIBUTING.md`,
      `SECURITY.md`, `docs/DESIGN.md`. Le code Rust garde pour l'instant ses commentaires et ses
      messages en français (journal, erreurs de l'API, ligne de commande, écran des paquets,
      notes de version, consignes données aux agents) : c'est la tranche suivante, voir Q14.
    - *Interface.* L'anglais est le texte source, écrit dans le code (`t('Cluster')`,
      `tn(n, '{n} agent', '{n} agents')`) ; le français est une table de l'anglais vers le
      français (`web/src/locales/fr/*.json`), embarquée. Langue retenue : celle choisie
      (gardée dans le navigateur), sinon la première langue du navigateur que l'on parle, sinon
      l'anglais ; un sélecteur est dans le menu et sur la page d'entrée. `npm run build` échoue
      si un texte du code n'a pas de traduction, si une traduction ne sert plus ou si ses
      variables diffèrent. Aucune dépendance ajoutée, aucune route, rien de lu depuis le réseau :
      pas de nouvelle surface. Les textes venus du serveur s'affichent tels qu'ils arrivent.

43. **Le paquet « audit »** (2026-11-28, « une flotte d'agents qui utilisent la base de code de
    Talixio AI Claude Auditor pour faire des audits de repo […] on lui donne une liste de repos
    et il lance […] tous les agents dont il a besoin »). `packs/audit`, livré dans l'exécutable,
    en anglais. Il reprend de l'auditeur ses deux scripts, sa méthode et son rapport ; l'auditeur
    est un projet Claude Code (skills et réglages), pas un programme : ses skills deviennent des
    rôles, ses règles `allow` des commandes.
    - *Commandes* : `fetch-repo <dossier> <adresse https> [branche]` (clone en lecture seule,
      sans crochet, sous-module ni LFS, adresse de poussée morte ; d'après `fetch.sh`) ;
      `repo-map <clone> <fichier.md>` (les chiffres : tailles, matériel pour assistants, graphe
      d'appels, doublons, croissance ; d'après `map.py`) ; `repo-git <clone> <question> […]`
      (neuf questions fixes en lecture : files, search, history, recent, commit, show, diff,
      blame, authors ; remplace les règles `git -C targets/* log *` de l'auditeur, que nos
      règles d'arguments ne savent pas dire sans donner les options de git) ; `new-report`
      (le gabarit du rapport, neuf sections).
    - *Rôles* : `lead` (un par dépôt : clone, ouvre le rapport, confie chaque étape, conclut),
      `goal`, `code-reviewer`, `setup-reviewer`, `planner`, `verifier` (lecture seule, cherche
      à refuser le rapport). Le rapport sur disque est la mémoire commune : aucun agent ne
      dépend du souvenir d'un autre, ce qui tient lieu de compaction (absente).
    - *Le but confirmé.* L'auditeur s'arrête après avoir trouvé le but et attend la personne.
      Ici le `lead` pose la question à son parent, que le maître relaie dans le dialogue ; sans
      réponse, le rapport dit « but non confirmé » (`brief.md`, la consigne à donner au maître).
    - *Rapport seulement.* Rien n'est changé dans le dépôt audité ni lancé depuis lui. La
      correction (branche, demande de fusion) viendra avec un rôle distinct et d'autres droits.
    - *Où vit git.* Les données git d'un clone sont gardées hors du dossier de l'agent
      (`~/.local/state/talixio-audit/`, sous le compte de l'agent) : il faut que ce dossier ne
      soit donné à aucun agent (seizième passe, F1).
    - *Premiers essais réels* (2026-11-28, v0.24.1 ; OVHcloud AI Endpoints, `gpt-oss-120b`, clé
      de l'utilisateur ; `dtolnay/itoa` trois fois, `pallets/itsdangerous` deux fois ; environ
      8 millions de tokens en tout). La chaîne tient : le maître crée le `lead`, qui clone,
      ouvre le rapport, crée ses cinq agents, et le maître rend son tableau. Un audit coûte 1 à
      2,5 millions de tokens et 6 à 9 minutes pour un dépôt de quelques centaines de lignes,
      dont plus de la moitié chez le `lead` (sa conversation grossit à chaque tour). Défauts
      vus et corrigés : refus sans explication (budget, dossier absent, règle recopiée de
      travers) qui faisaient tourner le maître en rond ; `{read}` recopié tel quel (les règles
      sont montrées avec `<read-path>`, `<arg>`) ; budget de 2 000 tokens donné à un enfant
      (chaque agent lit son budget) ; rapport écrit dans le clone (dossier `report/` séparé) ;
      mission du vérificateur réduite par le `lead` à « pas de section vide » ; numéros de ligne
      faux que personne ne relevait (commande `check-report`, dix-huitième passe).
      Le rapport unique, réécrit en entier par chaque agent, était écrasé : ce modèle y
      remplaçait les titres du gabarit par les siens.
    - *Un fichier par partie* (2026-11-28, v0.25.0, « chaque agent écrit son propre rapport
      et […] un autre agent consolide » ; « un rapport bien formaté, joliment, en HTML, hyper
      lisible »). `new-report <dossier>` crée onze fichiers (`0-header.md`, `summary.md`,
      `1-goal.md` … `9-gaps.md`), chacun avec un seul rédacteur. Rôle `audit/editor` : il lit
      toutes les parties, rend les contradictions au `lead` (qui les renvoie au rédacteur),
      écrit le résumé et les manques. `build-report <dossier> [titre]` assemble `report.md` et
      `report.html` (une page autonome, sans script ni ressource distante, tout le texte
      échappé) ; les titres viennent de la commande. Dix-neuvième passe de sécurité.
    - *Essais avec Qwen3.5 397B* (OVHcloud). Un `lead` a dépensé ses 3 millions de tokens à
      appeler 94 fois `list_agents` en attendant ses enfants : l'outil dit maintenant de finir
      son tour (le compte rendu arrive seul), et n'a plus été appelé à la passe suivante.
      Passe suivante sur `pallets/itsdangerous` : onze parties écrites chacune par son agent,
      aucune écrasée, `report.html` construit par le `lead` ; 54 citations, dont 52 se
      résolvent (deux plages dépassent d'une ligne la fin du fichier). Coût : 2,4 millions de
      tokens ; chaque agent spécialisé approche ses 400 000 (il lit large, et chaque tour
      renvoie toute sa conversation). Arrêtée avant le vérificateur. Formulaire du 2026-11-28 :
      envoi par e-mail et vers un salon, destinataires fixés par l'exploitant, tout fichier
      lisible par l'agent ; branche de travail : plus tard.
    - *Limites.* Les dépôts privés passent par un secret (décision 44), non essayé avec un vrai
      jeton. Il faut `git`, `bash` et `python3` là où tournent les agents (image `audit`).
      Le maître ne passe pas par un mandataire d'entreprise qui déchiffre (autorités de
      confiance embarquées seulement) : vu sur le banc d'essai, contourné par un relais local. `fetch-repo` clone depuis n'importe quel
      hôte https que l'agent nomme. Le contenu lu est plafonné (256 ko par fichier, 60 ko par
      réponse de commande). La licence de l'auditeur n'est pas déclarée dans son dépôt : même
      propriétaire, à régulariser.

44. **Les secrets des commandes** (2026-11-28, « voir comment passer des secrets aux agents
    enfants de manière sécurisée […] les gérer depuis le maître : ajouter, éditer, activer,
    désactiver, et décider vers quels agents enfants ils partent »). **Conception confirmée par
    le formulaire (Q15), écrite le 2026-11-28 (`src/secrets.rs`, v0.24.0), revue (dix-septième
    passe).** Écarts avec la proposition : le
    droit n'est pas une liste `secrets` à part mais un champ de la règle de la commande
    (`ExecRule.secrets`), que seul un rôle installé apporte à un agent (jamais la demande d'un
    modèle) ; un secret est lié à une commande, une variable et des rôles (un autre usage est un
    autre secret) ; un nom de rôle « paquet/rôle » ne peut être qu'un rôle installé que le
    créateur détient ; les variables d'un paquet commencent par son nom (`AUDIT_…`) ; l'hôte
    lié au jeton est un réglage d'accompagnement (`env`) que le script exige.
    - *Principe.* Un secret ne va jamais à un agent : il va à **une commande**, le temps d'un
      lancement. Le modèle ne le voit ni dans ses consignes, ni dans ses outils, ni dans une
      sortie. C'est la même règle que pour les clés des modèles, qui restent chez le maître.
    - *Chez le maître.* Un coffre `<state_dir>/secrets/` (un fichier par secret, lisible du
      seul compte du maître, comme `keys/`), interdit aux agents comme le reste de l'état. Une
      page « Secrets » et des routes `/api/secrets` : ajouter, remplacer la valeur, activer,
      désactiver, supprimer. La valeur ne s'écrit qu'une fois : aucune route ne la rend.
    - *Qui le reçoit.* Trois conditions, toutes vérifiées par le maître à chaque lancement :
      (1) la commande le demande par son nom : `secrets = ["AUDIT_READ_TOKEN"]` sur la
      commande dans `pack.toml`, donc une commande de paquet, vérifiée par empreinte, jamais
      une règle à arguments libres ; (2) l'exploitant a lié le secret à cette commande et à des
      rôles (« `depot-gitlab` → variable `AUDIT_READ_TOKEN` de `audit/fetch-repo`, pour le rôle
      `audit/lead` »), depuis la page ; (3) l'agent le détient dans ses droits (`secrets`,
      atténué comme le reste : un enfant n'en a jamais plus que son parent, et rien par défaut).
    - *Le trajet.* À chaque lancement d'une commande qui en demande, l'enfant fabrique une
      paire de clés jetable et demande le secret par le lien, comme il demande un modèle. Le
      maître vérifie les trois conditions, scelle la valeur pour cette clé et la renvoie : les
      agents intermédiaires relaient sans pouvoir lire. L'enfant pose la valeur dans
      l'environnement de cette seule commande, l'oublie, et retire toute occurrence de la
      valeur des sorties avant que le modèle ne les lise. Rien sur disque, rien dans la ligne
      de commande, rien dans le journal sauf le fait (« secret depot-gitlab remis à fetch-repo
      de l'agent X »).
    - *Désactiver* vaut au lancement suivant, partout. Aucun agent ne garde de copie.
    - *Ce que cela ne garantit pas, et qu'il faudra dire.* La commande qui reçoit le secret
      peut en faire ce qu'elle veut : d'où les seules commandes de paquet, relues et à root.
      Un secret remis sur une machine distante (ssh) y existe en mémoire, sous un compte que
      le maître ne tient pas. Le retrait des sorties ne voit que la valeur exacte, pas une
      valeur transformée. Et un jeton doit être lié à son hôte : `fetch-repo` clone où l'agent
      le dit, donc le lien de l'exploitant devra porter l'hôte permis (`gitlab.com`) et le
      script le vérifier avant d'employer le jeton.
    - *Écartés.* Le secret dans la configuration ou les consignes de l'enfant (lisible par le
      modèle et sur disque) ; une variable d'environnement de l'agent (héritée par toutes ses
      commandes) ; le maître qui clone lui-même pour ses enfants (ne vaut que pour git).

45. **Envoyer un travail au dehors : le paquet « notify »** (2026-11-28, « un autre agent ou
    quelque chose qui puisse envoyer un mail ou envoyer quelque part le rapport […] un agent
    d'envoi sur un channel, soit mail, soit autre chose » ; formulaire du 2026-11-28 : les deux
    canaux, destinataires fixés par l'exploitant, tout fichier lisible par l'agent).
    - *Deux commandes, un rôle.* `send-mail <fichier> [sujet]` : le fichier part en pièce
      jointe (10 Mio au plus), par SMTP chiffré seulement (465, ou STARTTLS). `send-webhook
      <fichier texte> [titre]` : le début du texte est publié dans un salon (webhook entrant
      Slack, Discord ou Teams, ou une adresse https qui prend du JSON) ; un webhook ne prend
      pas de fichier. Rôle `notify/sender`.
    - *Qui choisit quoi.* L'agent choisit le fichier, parmi ceux qu'il peut lire, et un sujet.
      L'exploitant choisit tout le reste, une fois, avec le secret (décision 44) : mot de passe
      et, à côté, serveur, compte, expéditeur, destinataires ; adresse du webhook et son genre.
      Rien dans une demande, un fichier ou un rapport ne change la destination ; les commandes
      ne suivent aucune redirection et n'emploient aucun mandataire.
    - *Risque accepté.* Un agent qui tient ces commandes peut envoyer aux destinataires tout
      fichier qu'il lit : ne donner le rôle qu'à des agents qui ne lisent rien que ces
      destinataires ne puissent voir. Dit dans `guide.md`.
    - *Limites.* Essayé contre de faux serveurs locaux, pas encore avec un vrai compte de
      messagerie ni un vrai salon. La page HTML part en pièce jointe, jamais dans le corps.

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
  store.rs      base SQLite de l'agent : mémoire, journal du maître
  tls.rs        certificats, TLS 1.3, épinglage par empreinte
  api.rs        écoute réseau : lien des enfants, API JSON du maître
  model/        format neutre ; connecteurs Anthropic et compatible OpenAI
tests/e2e.rs       la grappe de bout en bout, avec un faux serveur de modèles
tests/security.rs  les attaques de l'audit, rejouées : chacune doit échouer
```

### Lien parent-enfant

Trames JSON `{ "seq": n?, "msg": { "type": … } }` sur WebSocket chiffré (TLS 1.3), point d'entrée `/link`.

| Sens | Messages |
|---|---|
| Enfant → parent | `hello` (identité, aléa), `proof` (preuve, état), `tree` (état de sa branche), `report` (fin de mission, question, erreur), `activity` (journal), `model_request`, `service_request` (appel d'un modèle de service) |
| Parent → enfant | `challenge` (aléa, preuve), `welcome` / `rejected`, `instruction`, `model_response`, `service_response`, `stop_descendant`, `shutdown` |
| Les deux | `ping` / `pong`, `ack` |

Les messages portant un `seq` sont acquittés et renvoyés après une reconnexion ; le destinataire
écarte les doublons. Délais réglables (`[timing]`) : battement, mise en veille, extinction.

Présentation : `hello` → `challenge` → `proof` → `welcome`, en 10 s au plus. La preuve de chacun
est `HMAC-SHA256(secret, rôle ‖ aléa de l'enfant ‖ aléa du parent ‖ identifiant ‖ empreinte du certificat du parent)`. Huit
présentations simultanées au plus ; une requête portant un en-tête `Origin` (un navigateur) est
refusée. Trames : 16 Mio au plus pour un appel de modèle ou de service, 1 Mio pour le reste. Un compte rendu
n'est accepté que d'un enfant en mission, 20 au plus entre deux consignes, 20 000 caractères au plus.

### Outils d'un agent

| Outil | Condition |
|---|---|
| `read_file`, `list_dir` | au moins un dossier lisible |
| `write_file`, `make_dir` | au moins un dossier inscriptible |
| `list_provider_models` | le maître seul : la liste des modèles qu'un fournisseur connecté annonce, demandée avec la clé du maître |
| `run_command` | au moins une commande autorisée |
| `spawn_agent`, `send_to_agent`, `stop_agent`, `list_agents` | droit de créer des enfants |
| `ask_parent`, `finish_mission` | l'agent a un parent |
| `memory_write`, `memory_read` | toujours |
| `memory_search` | un modèle `embedding` et des appels accordés |
| `transcribe_audio` | un dossier lisible, un modèle `transcription` et des appels accordés |
| `describe_image` | un dossier lisible et un modèle de texte qui lit les images (`vision`) ; compté en tokens |
| `generate_image`, `speak_text` | un dossier inscriptible, un modèle `image` ou `speech` et des appels accordés |
| `forecast`, `decide` | un modèle `forecast` ou `decision` et des appels accordés |

### API JSON du maître

En HTTPS seulement. Authentification : `Authorization: Bearer <jeton>`. Le jeton vient de `TALIXIO_API_TOKEN`, sinon il
est créé au premier démarrage dans `<state_dir>/api-token`.

| Méthode | Chemin | Rôle |
|---|---|---|
| GET | `/`, `/app.js`, `/app.css`, `/sans.woff2`, `/mono.woff2`, `/icon.svg`, `/manifest.webmanifest` | L'interface web (sans jeton). |
| GET | `/api/health` | Vie du maître (sans jeton). |
| GET | `/api/tree` | La grappe : chaque agent avec nom, rôle, état, machine, OS, IP, droits, tokens, appels de service (`calls_used`), enfants. |
| GET | `/api/activity?after=N&limit=M` | Le journal après l'événement N. |
| GET | `/api/events?after=N` | Le journal en continu (Server-Sent Events). |
| POST | `/api/messages` `{ "text": … }` | Un message au maître. |
| POST | `/api/agents/{id}/stop` | Arrête un agent et sa descendance. |
| GET | `/api/releases` | La version qui tourne et les notes de version. |
| GET | `/api/models` | Les modèles du maître avec leur nature (`models` ; `default` : celui du maître, vide s'il n'en a pas ; `default_fixed` : fixé par la configuration) et le catalogue : fournisseurs connus, connectés ou non (`providers`, avec `linked` et `connect`), modèles, activés ou non (`catalog`) ; `activated` : les alias activés depuis l'interface. Jamais de clé. |
| POST | `/api/providers/{id}/connect` `{ "key": … }` | Essaie la clé chez le fournisseur du catalogue, puis la range chez le maître. Rend `{ connected, verified }`. |
| POST | `/api/providers/{id}/disconnect` | Retire les modèles activés d'ici et efface la clé. Refusé tant qu'un agent détient un de ces modèles. |
| POST | `/api/providers/{id}/oauth` `{ "callback": … }` | Commence une autorisation dans le navigateur ; rend `{ url }`. Le retour doit être la page du maître d'où vient la demande. |
| POST | `/api/providers/{id}/oauth/finish` `{ "code": … }` | Échange le code contre une clé, que seul le maître voit. |
| POST | `/api/models/activate` `{ "provider", "model", "alias"?, "calls"? }` | Active un modèle d'un fournisseur connecté d'ici ; rend `{ alias, nature, default, max_calls }` (`default` : il est devenu le modèle du maître, qui n'en avait pas). |
| POST | `/api/models/{alias}/deactivate` | Désactive un modèle activé d'ici. Refusé tant qu'un agent le détient. Si c'était le modèle du maître, il n'en a plus. |
| POST | `/bridge` | Hors de l'API de pilotage : le rappel du pont d'outils de `claude` (décision 32). Jeton d'un tour en cours, tiré pour lui ; ne permet que de poser les appels d'outils de ce tour. |
| GET, POST | `/api/tasks` | Les tâches planifiées (décision 33) ; POST en crée une : `name`, `text`, `kind` (`hours`, `daily`, `weekly`), `time` (`HH:MM`), `weekday` (0 : lundi), `hours`, `tz_offset_min`, `enabled`. |
| POST | `/api/tasks/{id}`, `/api/tasks/{id}/delete`, `/api/tasks/{id}/run` | Modifie, supprime, ou remet tout de suite la consigne au maître (l'échéance prévue ne bouge pas). |
| POST | `/api/models/{alias}/default` | Donne au maître ce modèle pour réfléchir (décision 31). Refusé si la configuration fixe `model`, si l'alias n'est pas dans ses droits ou ne tient pas une conversation. |
| GET | `/api/roles` | Le répertoire des rôles (500 fiches au plus). |

## 6. Questions ouvertes

| N° | Question | Piste |
|---|---|---|
| Q1 | ~~Où tourne le maître ?~~ | Tranché : sur une VM toujours allumée (décision 15). |
| Q2 | ~~Droits d'un enfant sur une autre machine~~ | Tranché : un plafond de droits par cible de déploiement (décision 17). |
| Q3 | ~~Chiffrement du lien~~ | Tranché : certificat du parent épinglé chez l'enfant (décision 9). |
| Q4 | Service ou démon (A5, A6) : systemd, launchd, service Windows ; un agent « hôte » par machine qui lance les autres. | Service : fait pour le maître (décision 28). Reste : agents permanents recréés au redémarrage, reprise des missions, agent « hôte » par machine distante. |
| Q5 | Assistant sur poste de travail : il observe une personne. | Visible et accepté par elle ; étape à part. |
| Q6 | Coffre des clés au niveau 0. | Fichier chiffré ou coffre du système, à choisir. En attendant : fichier de clé en 0600. À terme, les clés devraient vivre sous un autre compte que celui des agents. |
| Q7 | Stockage et mémoire des agents (S1–S3) : dossier, SQLite, ou les deux ? | Tranché pour la mémoire et le journal : SQLite embarqué, pour tous les agents (décision 14). Reste à faire : un dossier de travail par agent, ajouté d'office à ses droits (S1) ; l'arbre et les conversations sur disque (S3) ; effacement de la base d'un enfant à sa désinstallation ; mémoire partagée entre agents, si un usage le demande. |
| Q8 | Abonnement Claude Max au lieu d'une clé d'API (B5). | À vérifier avant tout : qu'un abonnement Max puisse servir à des agents tiers n'est pas acquis, ni techniquement ni dans les conditions d'Anthropic. La voie sûre est la clé d'API avec un solde. Si c'est permis : lire les limites renvoyées par le fournisseur à chaque réponse, et les traiter comme un budget qui se recharge. |
| Q9 | Android (P4) : piloter depuis le téléphone, ou y faire tourner un agent ? | Piloter : l'interface web, installable comme application (PWA), suffit ; elle demande HTTPS et un maître joignable depuis le téléphone, donc l'étape 2. Agent sur le téléphone : Rust se compile pour Android, mais le système arrête les process d'arrière-plan et enferme chaque application ; à n'étudier que si un usage le demande. |
| Q10 | Catalogue de fournisseurs et capacités image et voix (F1–F4). | Fait pour l'essentiel (décisions 25 et 26, v0.9.0 et v0.10.0 : on se connecte et on active depuis la page). Reste ouvert : comptage en argent (B4) ; mise à jour du catalogue depuis les fournisseurs ; essai chez de vrais fournisseurs. Notes de l'étude : Format de l'assistant relu (`src/ModelCatalog.php`, `Sovereign.php`, `Routing.php`, `Pricing.php`) : une table de fournisseurs (identifiant, nom, pays, niveau de confiance, adresse, mode de facturation) et une table de modèles (identifiant exact de l'API, fournisseur, nature `LLM`, `Vision`, `Voice`, `Image`, `Embedding`, prix par million de tokens ou par unité : minute, image). Usages liés à un modèle : `fast`, `standard`, `deep`, `voice`, `vision`, `embedding` ; effort et tokens de réflexion portés par le palier. À reprendre ici : `[providers.<nom>]` (adresse, type d'API, fichier de clé) et des modèles par nature, avec prix ; droits de la forme `fournisseur/modèle`. À savoir : l'assistant ne tient ni taille de contexte ni effort par modèle, ne compte que les tokens (minutes et images sont affichées, pas comptées), et n'appelle aucune API d'image ni de synthèse vocale : seules la transcription (`POST /audio/transcriptions`) et les embeddings existent. Image et synthèse sont donc à écrire, pas à reprendre. Côté maître, du code par nature : nouveaux messages du lien, tailles bornées autrement (un son ou une image pèse lourd), comptage en argent (B4), nouveaux outils chez l'agent. Nouvelle surface : revue de sécurité avant de coder. |
| Q11 | Se connecter par abonnement (Claude Pro/Max, ChatGPT) plutôt que par clé d'API ? | **Tranchée (2026-11-21) : oui, par la commande officielle `claude` seulement (décision 32).** D'abord écartée (« pas de dépendances, un seul exécutable »), puis demandée : l'abonnement Max doit pouvoir servir. Talixio reste un exécutable seul ; `claude` est un programme externe facultatif. Pour mémoire : L'assistant le fait pour Claude seulement, en pilotant la commande officielle `claude` (`setup-token`, puis `claude -p` à chaque tour, les outils exposés en MCP). Ici, cela demande d'installer cette commande à côté du maître, qui ne serait plus un exécutable seul, et un connecteur d'une autre nature (la commande tient elle-même la boucle d'outils). Le jeton d'abonnement ne doit pas servir à appeler l'API directement : les conditions d'Anthropic réservent cet usage à ses propres outils. Rien d'équivalent chez OpenAI dans l'assistant. |
| Q12 | Un maître lui-même dans Docker doit-il pouvoir lancer des agents dans d'autres conteneurs ? Comment ? | **Tranchée (2026-11-22) : oui, par le socket Docker de la machine (décision 36)**, « dans un premier temps » ; à terme, le maître ira dans une machine virtuelle. Écartés : Docker dans Docker (conteneur privilégié, pire pour une isolation moindre). Le maître installé sur la machine reste possible. |
| Q13 | Dépôt officiel de paquets : qui publie, où vit la clé. | Ouverte. Le mécanisme est prêt (décision 40) ; il manque une clé Talixio (sa partie publique dans `TRUSTED_KEYS`) et une adresse. Proposition : clé fabriquée et gardée hors ligne par Talixio, signature faite à la main à chaque publication (pas dans la CI, dont un secret volé signerait n'importe quoi), dépôt servi comme fichiers statiques (pages du dépôt GitHub ou site Talixio). |
| Q14 | Passage du programme lui-même en anglais et en deux langues : messages du journal, erreurs de l'API, ligne de commande, écran des paquets, notes de version, catalogue, configuration d'exemple, consignes données aux agents, commentaires du code. | Ouverte. L'interface web est faite (décision 42). Proposition pour la suite : (1) commentaires et noms de tests en anglais, sans effet sur le comportement ; (2) messages en anglais par défaut avec le français au choix (`TALIXIO_LANG`, `lang` dans la configuration), le journal gardant des codes stables que l'interface traduit ; (3) consignes des agents en anglais, à essayer avec de vrais modèles avant de changer, car elles pèsent sur leur conduite. |
| Q15 | Secrets des commandes (décision 44) : la conception proposée convient-elle ? | **Tranchée le 2026-11-28 (formulaire)** : (a) lien par rôle ; (b) refusé vers une cible ssh par défaut, permis secret par secret ; (c) hôte lié au jeton obligatoire (exigé par `fetch-repo`) ; (d) le maître aussi peut employer un secret, si l'exploitant le coche. |
| Q11 | Répertoire des rôles (R1–R3). | Une table SQLite chez le maître. Écriture : à chaque `spawn_agent`, le rôle remonte par le lien, de parent en parent ; à chaque fin de mission, son résultat. Lecture : un outil `find_role` (recherche par mots, puis par embeddings quand F1 existera), dont la demande remonte au maître comme une demande de modèle. Sécurité, à trancher avant de coder : un rôle enregistré contient des instructions écrites par un agent, donc un texte qui peut avoir été empoisonné et que d'autres agents reliront. Il doit être présenté comme une donnée à évaluer, jamais comme une consigne ; ne jamais porter de droits (les droits restent accordés par le parent, dans la limite des siens) ; et un rôle « approuvé » par l'humain doit se distinguer d'un rôle seulement observé. Fuite : un agent d'une branche lit ce qu'une autre branche a écrit ; prévoir de quoi cloisonner. |

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
2. **Le certificat fabriqué par le maître n'est pas reconnu par les navigateurs** : pour
   l'interface web, fournir un certificat d'une autorité reconnue (`[tls]`), ou vérifier
   l'empreinte affichée au démarrage. La ligne de commande, elle, épingle l'empreinte.
3. **L'injection par le sens** : aucun balisage n'empêche un modèle d'être convaincu par un texte.
   Les droits minimaux, les budgets et, à venir, la validation humaine (D9) limitent les dégâts.
4. La vérification d'un chemin puis son ouverture ne sont pas une seule opération : la fenêtre est
   réduite (ouverture sans suivre de lien, puis comparaison du fichier ouvert), pas supprimée.
   Seul le cloisonnement par le système la ferme.
5. Un enfant décrit lui-même sa propre descendance : le parent borne et nettoie, mais ne peut pas
   vérifier ce qu'il ne voit pas.
6. **Sur une cible SSH, le plafond de droits est tenu par l'agent lui-même**, comme en local
   (limite 1) : le vrai garde-fou est le compte distant. Lui donner un compte dédié, sans
   privilèges, qui ne voit que ce que l'agent doit voir. Ce compte peut lire et remplacer
   l'exécutable déposé : ne jamais le partager avec autre chose.
7. **Le compte du maître détient la clé SSH** : qui prend le maître prend les machines cibles,
   dans la limite de leurs comptes dédiés. Le fichier de clé est interdit aux outils des agents.
8. **Un conteneur garde le réseau** : il doit joindre le maître. Il peut donc aussi joindre le
   reste du réseau de la machine. À fermer par un réseau Docker dédié, côté machine. Avec
   `network` (décision 36), les agents sont dans le réseau du maître : ils s'y voient entre eux
   et voient l'API du maître, que gardent son jeton et le secret du lien ; ils gardent la sortie
   vers internet, dont leur travail a besoin.
9. **Lancer `docker` revient à être root sur la machine** : le maître d'une cible Docker doit
   être tenu pour tel. Le socket Docker n'est jamais montré à un conteneur d'agent. Un maître
   en conteneur qui a reçu le socket (décision 36) n'est plus enfermé par son conteneur : qui
   prend son process sort sur la machine. Accepté pour un premier temps, sur option explicite ;
   la suite prévue est le maître dans une machine virtuelle.
11. **Les modèles de service n'ont été essayés qu'avec de faux fournisseurs.** Les connecteurs
    suivent la forme publique des API (OpenAI, ElevenLabs) et celle des services de l'assistant
    (prévision, décision) ; le premier essai réel peut demander un ajustement.
12. **Ce qu'un agent envoie à un service sort de la grappe** : le son à transcrire, le texte à
    dire, ses notes (`memory_search`), l'état d'une décision partent chez le fournisseur du
    modèle. N'accorder un modèle de service qu'à un agent dont les données peuvent y aller, et
    préférer un fournisseur local ou souverain pour le reste.
14. **Le jeton de l'API vaut désormais une dépense** : qui le détient peut connecter un
    fournisseur et activer ses modèles (décision 26). Il ne lit aucune clé, mais il peut faire
    travailler la grappe sur une clé donnée. Le garder comme une clé.
15. **Les clés saisies dans l'interface sont en clair sur le disque du maître**
    (`<state_dir>/keys/`, fermées à son seul compte), comme un `api_key_file`. Pas de coffre
    (D6, Q6). Une commande accordée au maître lui-même (`exec`) tourne sous son compte : sans
    cloisonnement par l'OS, elle pourrait les lire. Limite 1, inchangée.
16. **OAuth sans `state`** (H3) : l'autorisation dans le navigateur repose sur PKCE seul. Ne pas
    ouvrir de lien vers la page du maître pendant une autorisation en cours.
17. **Le fournisseur « local » est un port de la machine du maître** : ne le connecter que si un
    serveur de modèles y tourne bien, sous un compte de confiance. Depuis la décision 31, un
    modèle local peut devenir celui du maître : qui tient ce port dicte alors ses appels
    d'outils, avec tous ses droits. Il n'est donc jamais donné d'office au maître, seulement
    par « Donner au maître ».
18. **Changer le modèle du maître déplace sa conversation** : le fil en cours (consignes,
    résultats d'outils, contenus de fichiers lus) part chez le nouveau fournisseur au message
    suivant. La page le dit avant de le faire ; le journal le note.
13. **Le quota de service compte des appels, pas de l'argent** : un appel d'image coûte plus
    qu'un appel de plongements. Régler `max_calls` nature par nature, au plus juste (B4 à venir).
10. Un agent déployé qui ne répond plus (machine éteinte, réseau coupé) s'éteint seul, mais sa
    désinstallation échoue si la cible est injoignable : l'échec est journalisé, pas rejoué.

### Huitième passe (2026-11-21) : l'abonnement Claude par la commande `claude`

Revue indépendante du connecteur, du pont et de la route `/bridge`. Aucun constat haut démontré :
le pont n'ouvre que le tour de son jeton, chaque appel d'outil repasse par les contrôles de
l'agent, un enfant ne prend pas la session d'un autre. Le cloisonnement reposait toutefois sur
les seules options passées à `claude`, sans contrôle de ce qu'il fait réellement.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| K1 | Moyenne à haute | Rien ne vérifiait, à l'exécution, que `claude` n'avait que les outils de l'agent : une option ignorée par une version future aurait rendu ses outils intégrés (shell, fichiers) sans que le maître le voie. | ✅ Le maître lit l'annonce de `claude` (outils, serveurs d'outils) et chaque appel : tout ce qui n'est pas à nous tue le tour. `--permission-prompts none` ajouté. Test (fausse commande qui sort de son rôle). |
| K2 | Moyenne | Seize tours pour tout l'arbre : un enfant pouvait les prendre tous ; les tours à l'écoute n'étaient pas comptés ; les sessions gardées n'avaient pas de borne. | ✅ Process vivants comptés par branche : quatre par branche, deux places gardées au maître, trente-deux sessions par branche. Test. |
| K3 | Moyenne | `~/.claude` (connexion, conversations de tous les agents) n'était pas dans les chemins interdits : un agent ayant le dossier personnel dans ses droits pouvait les lire. | ✅ `~/.claude`, `~/.claude.json`, `CLAUDE_CONFIG_DIR` interdits d'office, existants ou non. Test. |
| K4 | Moyenne | Conversation redonnée en clair : un résultat d'outil pouvait imiter une étiquette (`[Reçu]`) et se faire passer pour l'utilisateur. | ✅ Étiquettes marquées d'une valeur tirée au hasard à chaque rendu, annoncée en tête. Test. |
| K5 | Moyenne à basse | Session reprise sur le seul nombre de messages connus, sans vérifier que c'était la même conversation ; un tour expiré laissait sa session. | ✅ Empreinte des consignes et du début de la conversation ; session oubliée quand le tour expire. Test. |
| K6 | Moyenne à basse | Les tokens d'un tour en erreur ou abandonné ne sont pas comptés au budget. | 📌 Limite notée (décision 32) : `claude` ne rend son compte qu'avec ses messages. Les limites de l'abonnement bornent le reste. |
| K7 | Basse | À l'arrêt du maître, des process `claude` en attente pouvaient rester. | ✅ Tours vidés à l'arrêt. |
| K8 | Basse | Le pont rappelait toujours 127.0.0.1, même si le maître écoute sur une autre adresse précise. | ✅ L'adresse d'écoute est reprise quand elle est précise. |
| K9 | Basse | Un refus du maître n'était pas rendu à `claude`, qui attendait jusqu'au délai. | ✅ Le pont rend une erreur du protocole. |
| K10 | Basse | Filtre des secrets de l'environnement sensible à la casse (Windows ne l'est pas). | ✅ Comparaison sans casse. Ce que `claude_env` laisse passer reste à la charge de l'exploitant (noté). |
| K11 | Basse | `<état>/claude` pouvait être un lien symbolique posé avant le démarrage. | ✅ Refusé au démarrage. |
| K12 | Basse | Une invite commençant par « / » pouvait être prise pour une commande de `claude`. | ✅ Jamais en tête d'invite (et `--disable-slash-commands`). |

### Dix-neuvième passe (2026-11-28) : `new-report`, `build-report` et la page HTML du rapport

Revue indépendante avant publication, sur banc : parties et titres hostiles, 1,7 million de
compositions aléatoires passées au rendu avec une liste blanche de balises, page chargée dans
Chromium (aucune requête, aucun script, aucun événement). Conclusion : aucun texte d'un agent
n'arrive hors échappement dans `report.html` ; aucune sortie du dossier atteignable avec les
outils d'un agent. Le défaut réel était ailleurs : l'assemblage cachait au contrôle les
sections manquantes.

| # | Gravité | Constat | Traitement |
|---|---|---|---|
| H1 | moyen | Une partie supprimée, vidée, trop grosse ou remplacée par un lien devenait une phrase que `check-report` prenait pour du texte : plus de « EMPTY » (régression). | ✅ Ce qui manque est écrit entre chevrons, la forme que le contrôle lit comme vide ; marqueurs ajoutés au contrôle ; en-tête, résumé et parties trop grosses comptés manquants. |
| H2 | bas | Chaque partie était lue deux fois : `report.md` et `report.html` pouvaient différer si un agent écrivait pendant l'assemblage. | ✅ Une seule lecture. |
| H3 | bas | `report.md` garde le texte des parties tel quel (une image ou un lien y resterait actif dans un afficheur Markdown). | ✅ Dit par la commande et par le rôle : c'est `report.html` qu'on ouvre et qu'on envoie. |
| H4 | bas | Vérification puis réouverture d'une partie ; fichier temporaire rouvert s'il existait (lien dur). Non atteignable par un agent aujourd'hui. | ✅ Partie lue par un seul descripteur (`O_NOFOLLOW`, fichier ordinaire, taille bornée) ; fichier temporaire créé neuf (`O_EXCL`) ; `new-report` n'écrit que dans des fichiers neufs. |
| H5 | info | Espaces insécables et fines remplacées par « ? ». | ✅ Gardées comme espaces. |
| H6 | info | Texte sur la ligne d'ouverture d'un bloc de code absent de la page. | ✅ Gardé. |
| H7 | info | Le gabarit `report.md` du paquet n'est pas vérifié par empreinte au lancement (seul le programme l'est) : ne compte que si le dossier des paquets est inscriptible. | ❌ Noté. |

### Dix-huitième passe (2026-11-28) : `check-report` et les refus expliqués aux agents

Revue indépendante avant publication, sur banc (dépôts et rapports hostiles fabriqués, dépôt de
données construit comme par `fetch-repo`). Conclusion : rien de critique ni de haut ; ni
injection d'option vers git, ni lecture hors de l'arbre commité, ni exécution. La faiblesse
réelle : le contenu du dépôt pouvait se faire passer pour une ligne de verdict du contrôle, et le
contrôle pouvait être sauté ou tué sans le dire. Les messages de refus (`check_exec`,
`check_grant`, budget dans les consignes) ne montrent à un agent que ses propres règles.

| # | Gravité | Constat | Traitement |
|---|---|---|---|
| R1 | moyen | Un chemin suivi contenant un saut de ligne forgeait des lignes « exists » ou « WRONG » entières (rejoué). | ✅ Noms non imprimables ou trop longs écartés de la liste ; tout fragment venu du dépôt ou du rapport passe par un nettoyage (une ligne, caractères imprimables, longueur bornée). |
| R2 | bas/moyen | Retour chariot, ESC, U+2028 dans une ligne citée ou un sujet de commit. | ✅ Même nettoyage. |
| R3 | bas | Le titre de section, pris au rapport, était imprimé tel quel entre crochets. | ✅ Seul le numéro de section est imprimé. |
| R4 | moyen/bas | Au-delà de 400 citations, le reste était ignoré sans le dire. | ✅ Compté et signalé comme un défaut. |
| R5 | bas/moyen | Expressions quadratiques : une ligne de 60 000 caractères coûtait une minute, et le contrôle ne rendait rien. | ✅ Ligne de plus de 2 000 caractères signalée et non lue ; l'expression des citations ne repart plus de chaque position. |
| R6 | bas | Erreur de mémoire en citant beaucoup de gros fichiers. | ✅ Un seul fichier gardé en mémoire à la fois. |
| R7 | bas | `HOME` vide ou relatif : dépôt de données cherché sous le dossier courant. | ✅ Refusé ; propriétaire du dépôt de données vérifié. |
| R8 | bas | Un chemin inventé passait pour existant si son nom de fichier était unique. | ✅ Un chemin plus court n'est admis que s'il termine un seul chemin suivi, à un « / ». |
| R9 | info | Plage : seule la première ligne est montrée (désormais dit) ; faux « EMPTY » sur un texte entre chevrons (seuls les marqueurs du gabarit comptent) ; mot en lettres hexadécimales pris pour un commit (écarté) ; commit hors de l'historique audité compté présent (il doit être un ancêtre du commit audité) ; code de retour de `ls-tree` ignoré (vérifié). | ✅ |

### Dix-septième passe (2026-11-28) : les secrets des commandes (décision 44)

Revue indépendante du code avant publication, sur banc (maître en mémoire, vrai paquet, vraies
demandes forgées sur le lien, `git credential fill`). Conclusion : aucun moyen trouvé pour un
modèle seul d'obtenir une valeur ; la faille principale était au niveau du lien, pour un
process d'agent compromis. Neuf constats.

| # | Gravité | Constat | Traitement |
|---|---|---|---|
| S1 | moyen | Un agent qui relaie pouvait annoncer n'importe quel rôle pour un agent inventé, avec sa propre clé, et lire la valeur (rejoué). | ✅ Une demande relayée n'est admise que si l'enfant peut créer des agents et que le rôle est de ceux qu'il a pu donner (`roles_for_child`) ; rejoué dans `tests/security.rs`. (La présence de l'origine dans la descendance connue, d'abord exigée, a été retirée : l'arbre est déclaré par l'enfant et arrive après ses agents, d'où un refus à tort vu sur macOS.) Reste : un process compromis qui tient ces deux conditions obtient ce que sa descendance pourrait obtenir (noté). |
| S2 | moyen/bas | Le retrait d'un début de valeur en fin de sortie servait d'oracle (dit si un texte choisi commence la valeur). | ✅ Plus de recherche de préfixe : une sortie tronquée perd sa fin, sans condition ; une sortie entière n'est fouillée que pour la valeur exacte. |
| S3 | bas | Aucune limite sur les demandes de secrets ; rôle non borné ni nettoyé dans le journal. | ✅ 60 demandes par minute et par enfant ; champs bornés et refusés s'ils cachent un caractère. |
| S4 | bas | Liste des variables interdites par préfixe seulement (`HTTPS_PROXY`, `SSL_CERT_FILE`… pour un paquet nommé `https`, `ssl`) ; l'agent acceptait tout réglage d'accompagnement de forme correcte. | ✅ Les réglages permis sont portés par la règle (`ExecRule.env`) et filtrés ; liste allongée (`*_PROXY`, `SSL_`, `CURL_`, `OPENSSL_`, `GCONV_`, `MALLOC_`, `RES_`, `DOCKER_`, `AWS_`, `HTTP(S)_`). |
| S5 | bas | `fetch-repo` se fiait à la configuration du dépôt de données existant (si un agent pouvait écrire sous HOME). | ✅ Configuration réécrite à chaque lancement, propriétaire vérifié. Le dossier ne doit toujours être accordé à aucun agent. |
| S6 | bas | La valeur vit dans l'environnement de la commande et de ses enfants pendant le lancement : lisible dans `/proc/<pid>/environ` par un agent du même compte qui lit `/proc`. | ❌ Noté (limite) : pas de cloisonnement entre agents d'un même compte ; un conteneur par campagne. Piste : remise par un tube. |
| S7 | bas | Un secret survivait au remplacement de son paquet (même nom de commande, autre programme). | ✅ Le secret garde l'empreinte du programme ; si elle change, il n'est plus remis tant que l'exploitant ne l'a pas rouvert et enregistré (affiché dans la page). |
| S8 | info | Textes de la page trop affirmatifs (« jamais à un modèle », « effacé »). | ✅ Reformulés : ce qui est garanti (consignes, outils, journal, sortie telle quelle), où la valeur est gardée. Noté : une cible docker pilotant un démon distant compte comme locale. |
| S9 | info | `AUDIT_READ_URL` sans « / » final acceptait par le script un préfixe voisin (git, lui, ne donnait pas le jeton). | ✅ « / » final exigé. |

### Seizième passe (2026-11-28) : le paquet « audit » (décision 43)

Revue indépendante des quatre scripts, attaques rejouées (dépôts hostiles fabriqués, serveur
local, git 2.43). Neuf constats.

| # | Gravité | Constat | Traitement |
|---|---|---|---|
| F1 | critique | L'agent écrit dans son dossier, donc dans le `.git` du clone : `core.fsmonitor`, filtres, `core.worktree`, `insteadOf`, `gpg.program`, `alternates`… faisaient exécuter du code, écrire et lire hors des droits par les trois scripts (treize variantes rejouées). | ✅ Les données git sortent du dossier : `~/.local/state/talixio-audit/<empreinte du chemin>.git`, jamais dans un dossier donné aux agents (refusé sinon). `repo-git` et `repo-map` n'interrogent que ce dépôt-là, au commit extrait ; `blame` lit les objets. Rejoué : un `.git/config` posé dans le clone n'a plus d'effet. |
| F2 | critique (dès qu'un jeton existe) | Le jeton était offert à tout hôte que l'agent nomme. | ✅ `AUDIT_READ_URL` obligatoire avec le jeton : il n'est offert qu'à ce préfixe d'adresse (vérifié par le script, puis par git avec `credential.<url>.helper`), et n'existe que pour la commande réseau. |
| F3 | haut | `fetch-repo` reprenait tout dépôt existant sous `<dossier>/<nom>`, lien compris. | ✅ Seul un clone fait par `fetch-repo` (son dépôt à part existe, même adresse) est mis à jour ; sinon refus ; liens refusés. |
| F4 | moyen | `timeout` sortait git du groupe que le programme arrête. | ✅ `timeout --foreground`, 110 s ; un clone inachevé est retiré. |
| F5 | moyen | Ni mémoire ni taille bornées (une recherche montait à 11 Go). | ✅ en partie : mémoire bornée (2 à 4 Gio), fichiers de plus de 2 Mo comptés sans être lus, débit minimal, objets vérifiés. ❌ pas de quota sur la taille totale d'un clone. |
| F6 | bas | Un dépôt hostile faisait échouer `repo-map` (noms de fichiers, JSON inattendu). | ✅ listes en `-z`, structures vérifiées. |
| F7 | bas | Un fichier `HEAD` à la racine cassait `search` et `authors`. | ✅ `--` après la révision. |
| F8 | bas | Hôtes internes joignables (adresses en chiffres). | ✅ en partie : un nom avec une lettre et un point est exigé. Liste d'hôtes permis : avec les secrets (décision 44). |
| F9 | info | Motif dans `find`, réponse coupée sans le dire, clone tué à moitié. | ✅ |

Trouvé sain : premier clone d'un arbre hostile (rien ne s'exécute), liens de l'arbre non
suivis, redirections refusées, jeton absent des lignes de commande et des fichiers, validation
des adresses, noms, révisions et chemins. Non essayé : vrai serveur https hostile, systèmes de
fichiers insensibles à la casse, clone qui remplit le disque.

### Quinzième passe (2026-11-22) : l'écran des paquets (décision 41)

Revue indépendante, essayée dans un terminal sur des paquets hostiles. Garde du dossier en root,
nettoyage des erreurs et confirmation de base trouvés corrects. Deux constats hauts, quatre
moyens, six bas.

| Constat | Gravité | Traitement |
|---|---|---|
| Une partie du texte d'un paquet ne pouvait pas être atteinte à l'écran (défilement borné sur les lignes d'avant repli) : on installait ce qu'on n'avait pas pu lire. | haut | ✅ défilement borné sur les lignes affichées, d'une page moins une ligne ; l'écran dit « lu : n lignes sur m » puis « lu jusqu'au bout » (testé : tout le texte est atteignable à trois tailles). |
| Les caractères larges faisaient déborder une ligne, que la suivante recouvrait : texte masqué. | haut | ✅ tout ce qui s'affiche passe par `shown` : une colonne par caractère ; caractères larges, invisibles, de commande ou d'autres écritures deviennent « ? » (testé). Limite : un paquet écrit dans une autre écriture que latine se lit avec `pack show`, pas à l'écran. |
| Avec `--from`, le paquet était relu à l'installation : un tiers pouvait le changer entre la lecture et le « o ». | moyen | ✅ c'est le paquet lu et affiché qui est installé ; la marque se fonde sur son contenu, pas sur ce que la liste annonce. |
| Confirmation sans « o » délibéré : texte collé, touche tapée d'avance, Alt-o. | moyen | ✅ collage ignoré, file vidée une fois la question à l'écran, touches avec modificateur refusées (essayé : Alt-o ne confirme pas). |
| « ✓ installé » affiché à tort quand la vérification des paquets installés échouait ; compte rendu de l'action écrasé. | moyen | ✅ marque « ? » (inconnu), compte rendu gardé ; un paquet abîmé peut être retiré de l'écran (testé). |
| `clean` laissait passer des caractères invisibles (« étiquettes » U+E0000, sélecteurs, usage privé, U+00AD…) : un paquet pouvait porter des consignes que l'exploitant ne voit pas mais que l'agent reçoit. | moyen | ✅ ajoutés à `clean` (`src/lib.rs`) : un paquet qui en contient est refusé par `pack`, et ils sont retirés partout où un texte est nettoyé. |
| Fausses lignes fabriquées par des espaces insécables ; question coupée par un long chemin ; chemin non nettoyé ; avertissement « non signée » masqué par l'écran. | bas | ✅ espaces normalisés, question avant le chemin, tout passe par `shown`, la nature de la source est en tête d'écran. |
| Terminal non rendu après une panique. | bas | ✅ crochet de panique. Sur SIGTERM ou SIGHUP : ⬜ noté (`reset` le rend). |
| L'écran relit le dossier `--from` à chaque déplacement ; il se fige pendant un téléchargement. | information | ⬜ noté. |

### Quatorzième passe (2026-11-22) : le dépôt de paquets signé (décision 40)

Revue indépendante, essayée avec des dépôts en dossier. Signature, empreintes, ordre des contrôles
et traversée de chemin trouvés corrects. Un constat haut, trois moyens, sept bas.

| Constat | Gravité | Traitement |
|---|---|---|
| Avec `--key`, un dossier dont l'index avait été retiré était lu comme une source locale non signée, sans un mot (démontré). | haut | ✅ `--depot` et `--from` séparés ; `--key` exige `--depot` ; une source locale s'annonce « NON signée » (testé). |
| Un ancien index, toujours bien signé, remplaçait un paquet plus récent ; la date n'était ni montrée ni employée. | moyen | ✅ date affichée, futur refusé, retour en arrière refusé sauf `--allow-older` (testé). Gel et première installation : limites notées. |
| Séquences d'échappement dans le terminal par l'erreur d'analyse d'un `pack.toml` ou d'un index. | moyen | ✅ erreurs nettoyées, et toute erreur de la ligne de commande l'est à la sortie (testé). |
| `pack show --file` imprimait le fichier brut. | moyen | ✅ nettoyé. |
| Clé publique d'ordre faible acceptée : tout le monde sait signer pour elle (démontré). | bas | ✅ refusée (testé). |
| `https://` sans hôte : un nom de fichier devenait l'hôte. | bas | ✅ adresse analysée, hôte exigé, chemins ajoutés par segments (testé). |
| Tube nommé à la place d'un fichier du dépôt : lecture sans fin ; liens suivis. | bas | ✅ fichiers ordinaires seulement. |
| `keygen` laissait un fichier vide en cas d'échec ; droits posés après l'écriture. | bas | ✅ fermé avant l'écriture, retiré en cas d'échec. |
| `publish` publiait une clé privée rangée dans un paquet. | bas | ✅ refus (testé). |
| Serveur lent : jusqu'à une heure. | bas | ✅ cinq minutes en tout. |
| Noms réservés de Windows, point final, doublons à la casse près. | information | ✅ refusés. |
| `publish --out` suit un lien et n'est pas atomique. | bas | ⬜ noté : n'écrivez un dépôt que dans un dossier à vous. |

### Treizième passe (2026-11-22) : le compte dédié (décision 38)

Revue indépendante, essayée en conteneur (Debian 13, sans systemd). Un constat haut, trois moyens,
cinq bas, trois d'information.

| Constat | Gravité | Traitement |
|---|---|---|
| La vérification de la configuration lancée sous le compte (`check`) héritait de l'environnement, du terminal et des fichiers ouverts de root ; le maître, de même compte, pouvait la tracer (démontré : variable de root et `/etc/shadow` lus, terminal de root atteint). | haut | ✅ `check_as_account` : environnement vidé, entrée fermée, sorties dans un fichier à root relu puis nettoyé, nouvelle session, descripteurs fermés, 30 secondes au plus. Son verdict reste indicatif (le compte peut le fausser), sans gravité : la configuration est celle de root. Correction non rejouée avec l'outil de la revue. |
| Un compte `talixio-agent` préexistant était accepté tel quel (groupes docker ou sudo, shell, groupe partagé). | moyen | ✅ refusé s'il a un shell, un autre groupe que le sien, ou un groupe partagé (essayé). |
| Le contenu de `/opt/talixio` et `/etc/talixio` n'était pas vérifié : un dossier préexistant trop ouvert était corrigé en silence, des fichiers d'un tiers acceptés. | moyen | ✅ tout y est à root, sans lien, fermé en écriture aux autres, sinon refus (essayé) ; plus de correction silencieuse. |
| Installation non atomique : configuration remplacée avant un contrôle qui pouvait échouer ; le compte pouvait bloquer une réinstallation (`chmod 777` de son dossier). | moyen | ✅ contrôles avant toute écriture ; root remet le dossier d'état en 0700 (essayé). |
| `talixio.toml` remplacé par un lien : root lisait la cible et la rendait lisible au compte. | bas | ✅ lien refusé (essayé). |
| `pack install` / `remove` en root dans un dossier tenu par un autre compte : écritures et suppressions détournables par lien. | bas | ✅ en root, le dossier et ses ancêtres doivent être à root seul (essayé). |
| Configuration avec marque d'ordre des octets : refus obscur. | bas | ✅ marque retirée. |
| Dossier personnel du compte inscriptible par le maître (`~/.ssh`). | information | ✅ compte créé sans dossier personnel (`/nonexistent`). |
| Ajouter le compte au groupe `docker` annule la séparation. | information | ✅ dit à l'installation ; les cibles `docker` ne sont pas pour ce mode. |
| Le fichier d'essai est lisible par le compte pendant la vérification ; `pack install --from` en root lit par chemin un dossier source qu'un tiers pourrait tenir. | bas | ⬜ notés. Ne donnez à `--from`, en root, qu'un dossier à vous. |
| Durcissements de l'unité à évaluer (`PrivateTmp`, `ProtectHome`, `ProtectSystem=strict`). | information | ⬜ à faire avec un vrai systemd, pour ne pas casser le cloisonnement des agents. |

### Douzième passe (2026-11-22) : les paquets (décision 37)

Revue indépendante, avec essais. Un modèle, par ses seuls outils, ne peut ni s'ajouter une
commande ni élargir une commande de paquet. Un constat haut : la garde du dossier se contournait.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| P1 | Haute | La garde « le maître ne peut pas écrire ses paquets » ne regardait que les droits d'écriture : un dossier au compte du maître passait après un `chmod a-w`, et un dossier parent inscriptible permettait de remplacer le paquet entier, empreintes comprises. | ✅ Refus si le dossier, un fichier ou un dossier au-dessus, jusqu'à la racine, appartient au compte du maître ou lui est inscriptible, sauf montage en lecture seule. |
| P2 | Moyenne | Le contrôle répondait pour le compte réel sans ses capacités, et toute erreur valait « protégé ». | ✅ Droits effectifs ; seul un refus net du système vaut protection. |
| P3 | Moyenne | Sur une cible, l'agent pouvait fournir lui-même l'empreinte (le plafond n'en portait pas). | ✅ L'empreinte vient toujours du parent, jamais de la demande ; le plafond d'une cible reçoit celle du paquet au chargement. |
| P4 | Moyenne | `pack show` et les autres affichaient le manifeste tel quel (séquences de terminal), sans les consignes des rôles. | ✅ Champs d'une ligne, bornés, sans caractère de commande, refusés sinon ; consignes affichées. |
| P5 | Moyenne | Descriptions et arguments du manifeste entraient tels quels dans les consignes du maître. | ✅ Mêmes contrôles, à l'analyse du manifeste. |
| P6 | Basse à moyenne | Un rôle de paquet au nom courant détournait les créations d'agents de ce nom. | ✅ Un rôle installé s'appelle toujours « paquet/rôle » ; le résultat de `spawn_agent` dit ce qui a été donné. |
| P7 | Basse | Les fichiers voisins d'un script (liste de sites) ne sont revérifiés qu'au démarrage du maître ; vérification puis lancement par le chemin. | 📌 Limite : tient tant que la garde P1 tient. Empreinte du paquet entier chez l'agent : à faire. |
| P8 | Basse | Une règle de cible sous le dossier des paquets survivait au retrait du paquet ; chemins différents entre maître et image : pas d'empreinte. | ✅ Une telle règle doit être une commande installée, sinon le maître ne démarre pas. 📌 Les paquets doivent être au même chemin chez le maître et dans l'image. |
| P9 | Basse | Une règle écrite à la main sur un programme de paquet passait devant celle du paquet, sans empreinte. | ✅ Refusée au chargement. |
| P10 | Basse | Sous Windows, le contrôle ne voulait rien dire. | ✅ Dit comme tel : pas de protection vérifiée, `packs_unprotected` à poser en connaissance de cause. |
| P11 | Basse | `--from` prenait un paquet par le nom de son manifeste ; `remove` supprimait tout dossier portant un `pack.lock` ; installation interrompue. | ✅ Dossier et manifeste de même nom ; seuls les fichiers du paquet sont retirés ; installation à côté puis mise en place d'un coup. |
| P12 | Basse | `sha256` ni validé ni borné dans les droits déclarés. | ✅ 64 chiffres hexadécimaux, compté, nettoyé. |
| P13 | Basse | `tools.sh` : chemin relatif pris pour un volume, dossier temporaire laissé. | ✅ |
| — | Information | Un paquet installé exécute du code sous le compte du maître : « jamais d'arguments libres » ne borne que les scripts honnêtes. `pack.lock` vit dans le dossier qu'il décrit : il détecte l'accident, la garde P1 fait le reste. | 📌 D'où la signature, à faire avec le service de téléchargement. |

### Onzième passe (2026-11-22) : le maître en conteneur et ses conteneurs voisins (décision 36)

Revue indépendante, sur un vrai Docker. Rien ne permet à un agent d'ajouter une option à
`docker run` ni d'atteindre le socket, et le conteneur d'un agent garde toutes les protections de
la décision 19. Aucun constat haut ; deux moyens.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| V1 | Moyenne | « Jamais le volume entier » n'était pas tenu : `subpath` absent ou valant « . » montait tout le volume (clés du maître comprises) ; un lien posé par un conteneur qui écrit le dossier parent était suivi par Docker. | ✅ `subpath` obligatoire, chaque partie vérifiée (ni vide, ni commençant par un point) ; deux montages emboîtés du même volume sont refusés quand le parent est en écriture. |
| V2 | Moyenne | Le groupe qui ouvre le socket passe à tout ce que le maître lance : agents à côté de lui, commandes permises, `claude`. Une règle de commande trop large donne alors la machine, plus seulement le conteneur. | 📌 Risque accepté élargi et écrit (décision 36, limite 9). À terme : le socket tenu par un process à part, ou le maître dans une machine virtuelle. |
| V3 | Basse | Relancer le maître laissait tourner les conteneurs d'agents de l'ancien. | ✅ `tools.sh run` les retire d'abord. |
| V4 | Basse | `stop` oubliait un conteneur d'agent hors du réseau du maître ; `purge` disait « Effacé. » quand le volume restait. | ✅ Recherche par réseau et par volume ; `purge` dit ce qui reste. 📌 `network` doit valoir `<nom>-net`. |
| V5 | Basse | Une cible pouvait monter le socket Docker (`host`), ou monter par-dessus `/talixio-agent` et `/state`. | ✅ Refusés au chargement. |
| V6 | Basse | Avec `agent_in_image`, l'agent peut être d'une autre version que le maître si l'image n'est pas reconstruite. | 📌 Dit dans le README ; version dans la poignée de main : à faire. |
| V7 | Information | Les chemins interdits du socket ne couvrent pas un Docker sans droits d'administrateur. | 📌 Noté ; ce mode n'est pas essayé. |

### Dixième passe (2026-11-22) : le navigateur des agents (`lire-page`, image, filtre d'appels système)

Revue indépendante, avec un banc d'essai (serveur local, pages piégées). Deux constats hauts,
vérifiés en les jouant : le contrôle des sites ne valait que pour le premier saut. Corrigés à la
racine par un mandataire interne au script, seule sortie du navigateur.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| M1 | Haute | Les redirections 30x ne repassaient pas par le contrôle : un site de la liste qui redirige menait à n'importe quel site, et à tout service http joignable depuis la machine (métadonnées, réseau privé, ports locaux). | ✅ Tout le trafic de Chromium sort par un mandataire tenu par le script : `CONNECT nom:443` seulement, vers un nom que le script a approuvé (site de la liste, ou sous-ressource vue par le contrôle des requêtes), à l'adresse publique qu'il a lui-même résolue ; cette machine comprise (`<-loopback>`). Une redirection vers un site hors liste ne part donc pas, et le texte n'est rendu que si la page d'arrivée est sur un site de la liste. Contre-vérifié par le relecteur sur son banc ; le dernier resserrement (noms approuvés) a été fait après, d'après son reste, et essayé sur les vraies pages seulement. |
| M2 | Haute | Les tâches de fond (Worker, SharedWorker) échappaient au contrôle : WebSocket et http vers la machine. | ✅ Même mandataire (rien n'en sort) ; Worker, SharedWorker, WebSocket, WebTransport, EventSource retirés des pages. |
| M3 | Moyenne | WebRTC sortait en UDP et révélait l'adresse locale (un drapeau faisait l'inverse de ce qu'il fallait). | ✅ Drapeau retiré, politique « rien hors mandataire », `RTCPeerConnection` retiré des pages. |
| M4 | Moyenne | Chromium figé à une version de plus d'un an ; ni empreinte ni verrou des dépendances. | ✅ Version courante du pilote. 📌 À monter à chaque publication ; verrou et empreinte de l'image de base restent à faire. |
| M5 | Moyenne | Le filtre ouvrait `clone`, `unshare`, `setns`, `chroot` à tout le conteneur du maître. | ✅ Le navigateur n'est plus dans le conteneur du maître : seul le conteneur jetable de l'agent reçoit ce filtre, sans `setns`. 📌 Créer des espaces de noms reste une surface noyau de plus, dans ce conteneur-là. |
| M6 | Moyenne | Un nom public qui change d'adresse entre le contrôle et la connexion. | ✅ Le mandataire se connecte à l'adresse qu'il a contrôlée. |
| M7 | Basse | Une fenêtre ouverte par la page faisait planter le script. | ✅ Cas rattrapé ; `window.open` retiré. |
| M8 | Basse | La page d'erreur de Chromium rendue comme contenu, avec une adresse choisie par la page. | ✅ Hors d'un site de la liste, rien n'est rendu. |
| M9 | Basse | Caractères invisibles laissés dans la sortie (C1, « étiquettes », séparateurs) ; `lire-web` ne filtrait rien. | ✅ Retirés dans les deux scripts. |
| M10 | Basse | Les scripts tiers d'une page de la liste peuvent envoyer ce qu'ils voient (l'adresse lue) à tout nom public. | 📌 Limite, par nature d'un navigateur : l'agent qui lit le web n'a que ses relevés, ni clé ni catalogue. |
| M11 | Info | `navigateur.conf` vaut exécution de code si le dossier des scripts est inscriptible par un agent. | 📌 Le dossier est monté en lecture seule dans le conteneur ; à tenir hors de portée ailleurs. |

### Neuvième passe (2026-11-22) : tâches planifiées, `make_dir`, `list_provider_models`, `catalog check`, exemple « catalogue »

Revue indépendante de la v0.14.0. Aucun constat haut : aucun outil d'agent ne touche aux tâches,
les routes sont derrière le jeton de l'interface, `make_dir` repasse par les contrôles d'écriture
à chaque niveau, la clé d'un fournisseur ne sort pas de `list_provider_models`.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| L1 | Moyenne | `catalog check` ne disait rien d'un fournisseur connu dont l'adresse ou les hôtes changent : un agent pouvait proposer un catalogue qui envoie une clé ailleurs, avec un contrôle « valide ». | ✅ Tout changement de `endpoint`, `hosts`, `apis`, `keys_url`, OAuth ou `keyless` est dit en tête (« ATTENTION ») ; fournisseurs ajoutés montrés avec leur adresse ; modèles modifiés dits champ par champ. Le catalogue n'est de toute façon jamais chargé à chaud : c'est une personne qui le copie au dépôt. |
| L2 | Moyenne | Le rendu Markdown du dialogue pouvait boucler sans fin sur une ligne qu'aucun bloc ne prenait (titre indenté) : onglet figé par une réponse du modèle. | ✅ Le rendu avance toujours d'une ligne ; titres indentés reconnus. |
| L3 | Moyenne | `lire-web` laissait passer les plages de curl (`[1-3]`) : plusieurs adresses en un appel. | ✅ `--globoff`, crochets refusés. |
| L4 | Basse | `lire-web` : champs de `-w` décalés par un type de contenu à espace, pages jamais nettoyées. | ✅ Champs séparés par tabulation, lus sur la dernière ligne ; scripts, styles et commentaires retirés. |
| L5 | Basse | La sortie de `catalog check` reprenait des chaînes du fichier candidat sans les nettoyer. | ✅ Chaque élément nettoyé et borné à 200 caractères ; erreurs bornées. |
| L6 | Basse | Un lancement réécrivait toute la tâche : une modification simultanée était perdue, une tâche supprimée revenait ; plafond de 32 compté puis écrit en deux temps. | ✅ Un lancement n'écrit que ses deux dates, et rien si la tâche n'existe plus ; création comptée et écrite d'un seul geste. |
| L7 | Basse | Une modification sans `enabled` ni `tz_offset_min` réactivait la tâche et remettait son fuseau à UTC. | ✅ Un champ absent garde sa valeur. |
| L8 | Basse | `lire-web` : la taille gardée dépendait de curl seul ; l'adresse résolue n'est pas contrôlée. | ✅ Coupe à 3 Mo quoi que fasse curl. 📌 Limite : pas de contrôle de l'adresse résolue ; la liste des sites est tenue par l'exploitant, et ne doit porter que des sites publics. |
| L9 | Basse | Une redirection était rendue avec sa cible, texte choisi par le site. | ✅ La cible n'est dite que si elle reste dans la liste des sites. |
| L10 | Basse | `catalog check` : comparaisons quadratiques, pas de borne de taille. | ✅ Tables d'index, 1 Mo au plus. |
| L11 | Basse | `list_provider_models` rend du texte choisi par le fournisseur. | 📌 Limite : champs choisis, nettoyés et bornés (500 lignes, 80 000 octets), rendus comme résultat d'outil, donc comme une donnée ; le fournisseur est déjà celui à qui le maître confie sa clé. |
| L12 | Info | `make_dir` : chemin vérifié après le premier regard sur le disque ; un dossier créé puis refusé restait. | ✅ Chemin vérifié d'abord ; le dossier refusé est retiré. |
| L13 | Info | Une tâche planifiée survit au changement du jeton de l'interface. | 📌 Limite notée : qui a eu le jeton a pu laisser une consigne ; la page « Tâches » les montre toutes, à relire après un changement de jeton. |
| L14 | Info | `TALIXIO_TOOLS` avec « : » dans le chemin changeait le montage ; l'exemple exige l'image `claude` sans le dire. | ✅ Chemin refusé ; dit dans l'exemple. |

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

### Deuxième passe (2026-11-20) : déploiement, TLS, notes de version

Revue indépendante, à la lecture, du code de déploiement et du TLS. Dix constats.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| 1 | Haute | Les fichiers de clés n'étaient interdits qu'aux outils du maître : un enfant local qui recevait le dossier pouvait les lire. | ✅ Les interdits descendent de parent en enfant sur une même machine ; le dossier `.ssh` du compte est interdit d'office ; un fichier de clé déclaré mais introuvable arrête le démarrage. Rejoué dans `tests/security.rs`. |
| 2 | Haute | Une cible SSH qui ne lit pas, ou répond sans fin, bloquait le maître ou épuisait sa mémoire ; sa réponse était recopiée telle quelle au modèle. | ✅ Envoi et lectures menés ensemble sous un seul délai, sorties bornées, réponse nettoyée et tronquée. |
| 3 | Moyenne | Un agent déployé par SSH qui n'obéissait plus n'était que désinstallé, pas tué. | ✅ Son numéro de process est noté au lancement ; la désinstallation le tue, lui et son groupe. Rejoué dans `tests/deploy.rs`. |
| 4 | Moyenne | Docker : sans `user`, l'agent tournait sous le compte de l'image, souvent root. | ✅ `user` obligatoire, numérique, jamais 0. |
| 5 | Moyenne | Dossier distant : ni propriétaire ni droits vérifiés ; `//` et `/tmp` acceptés. | ✅ Le dossier doit appartenir au compte, ne pas être un lien, passe en 0700 ; `/tmp`, `//` et `/` final refusés ; fichiers créés fermés aux autres. |
| 6 | Basse | Le journal local d'un enfant n'est pas borné : un enfant peut remplir le disque du maître. | ✅ Rotation par le parent, à la taille fixée par le maître (décision 24). |
| 7 | Basse | À l'extinction, une cible injoignable privait les suivantes de nettoyage. | ✅ Désinstallations en parallèle, un délai par cible. |
| 8 | Basse | Un enfant déployé ne pouvait pas recevoir le droit de créer des enfants si le maître n'avait pas `local`. | ✅ Sur une cible, `local` désigne la machine de l'enfant. |
| 9 | Basse | Arrêt pendant un lancement : l'exécutable restait sur la cible. | ✅ Revérifié après le lancement ; un dépôt interrompu est nettoyé. Restent : le journal distant d'un enfant mort au démarrage n'est pas rapatrié ; les bases des petits-enfants nés sur la cible ne sont pas retirées. |
| 10 | Basse | Certificat du maître écrit en deux temps : un arrêt entre les deux empêchait tout redémarrage. | ✅ Une paire incomplète est refaite. |

Aussi : `ssh` est lancé sans aucun fichier de réglages (`-F /dev/null`). Examiné sans rien
trouver : injection vers `ssh`, `docker` ou le shell distant ; chemins de la désinstallation ;
plafond d'une cible ; secret du lien ; TLS (1.3 seul, signature vérifiée, empreinte comparée en
temps constant) ; moniteur ; API.

### Troisième passe (2026-11-21) : interface web, `/api/models`, `/api/roles`

Revue indépendante, à la lecture, de l'interface (v0.7.0), des deux routes et du répertoire des
rôles. Aucun constat critique. Dix constats, tous traités en v0.7.1.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| E1 | Haute | Le répertoire des rôles était réécrit agent par agent, dans la tâche du réseau, à chaque arbre reçu : un enfant qui renvoie sans cesse un gros arbre bloquait le maître et remplissait sa base (droits déclarés sans borne de taille). | ✅ Droits déclarés bornés (16 Kio par agent, sinon l'arbre est refusé) ; écriture à part du réseau, au plus toutes les 2 s par branche, en une transaction, et seulement si quelque chose a changé. Rejoué dans `tests/security.rs` et `store::tests`. |
| M1 | Moyenne | Une branche pouvait réécrire la fiche d'un agent d'une autre branche, et chasser par le nombre les fiches des autres. | ✅ Chaque fiche appartient à la branche qui l'a créée ; purge par branche d'abord, et les fiches portant des consignes partent en dernier. |
| M2 | Moyenne | Les droits et le modèle affichés pour un petit-enfant sont ceux que sa branche déclare : l'interface les montrait comme des faits. | ✅ Chaque parent compare ce qu'un descendant déclare à ce que son propre parent déclare pouvoir accorder ; l'écart est marqué (`unverified`) et affiché « déclaration incohérente ». Un descendant ne peut plus porter le nom du maître ni d'un de ses enfants directs. Reste déclaré, sans preuve : tokens, état et hôte des niveaux ≥ 2 (limite 5). |
| M3 | Moyenne | Un genre d'événement inventé par un enfant (`constructor`, `__proto__`) faisait tomber l'écran Journal. | ✅ Genres hors liste ramenés à `autre` par le maître ; côté page, tables lues sans propriétés héritées, et un écran qui échoue ne vide plus la page. |
| F1 | Basse | `/api/roles` rendait tout le répertoire à chaque lecture. | ✅ 500 fiches au plus. |
| F2 | Basse | Tout agent écrivait un répertoire des rôles dans sa propre base, que personne ne lit. | ✅ Réservé au maître ; consignes nettoyées et bornées. |
| F3 | Basse | Les marques invisibles et de retournement du sens de lecture passaient le nettoyage : un nom pouvait en imiter un autre. | ✅ Retirées par `clean`, appliqué aussi aux droits déclarés. |
| F4 | Basse | Outils de construction rangés parmi les dépendances de la page ; scripts d'installation exécutés en intégration continue. | ✅ `devDependencies` ; `npm ci --ignore-scripts`. |
| F5 | Basse | Réponses de l'API sans consigne de cache. | ✅ `Cache-Control: no-store` et `nosniff` sur toute réponse. |
| — | Basse | Hors périmètre : une erreur de connexion à un fournisseur citait l'adresse appelée, donc une clé si elle y figurait en paramètre. | ✅ Adresse retirée des messages d'erreur. |

Examiné sans rien trouver : injection de HTML ou de script dans la page (tout passe par React,
aucun `dangerouslySetInnerHTML`, CSP sans `unsafe-inline`) ; fuite de clés par `/api/models` ;
accès sans jeton ; rangement du jeton dans le navigateur.

Catalogue des fournisseurs (v0.8.0) : pas de nouvelle route ni d'écriture ; des données fixes,
embarquées, ajoutées à `/api/models` (sous jeton). Relu par l'auteur, sans revue indépendante :
le rattachement d'un modèle à un fournisseur compare le nom de machine entier (ou un sous-domaine
exact), testé contre les noms qui n'en ont que la fin ; le filtre passé dans l'adresse de la page
(`#modeles?f=…`) n'est que comparé à des identifiants. Se connecter et activer depuis la page
ouvrirait une vraie surface (saisie d'une clé, changement des droits du maître) : à décider et à
faire revoir avant de coder.

Limite assumée du signalement M2 : un parent sous Windows ne sait pas juger des chemins POSIX
(et inversement) ; la descendance d'une autre famille de système est alors marquée, à tort.

### Quatrième passe (2026-11-21) : modèles de service et lecture d'images

Revue indépendante du message `service_request`, des sept outils et des connecteurs. Aucun
constat critique ; deux défauts réels, reproduits par exécution, puis rejoués en test
(`tests/services.rs`).

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| G1 | Haute | Un appel qui échouait après être parti chez le fournisseur (réponse trop grosse, image rendue par adresse, délai) n'était pas compté : un enfant au quota de 1 pouvait faire dépenser sans borne. | ✅ Tout appel parti est compté, abouti ou non ; seul un refus avant envoi est gratuit. L'information (`spent`) suit la réponse sur le lien, à chaque niveau. |
| G2 | Moyenne | Le maître suivait les redirections d'un fournisseur : requête rejouée vers une adresse interne, dont la réponse finissait dans un fichier de l'agent ; la clé `xi-api-key` suivait. | ✅ Aucune redirection suivie, pour tous les modèles (texte compris) ; un 3xx est une erreur. |
| G3 | Basse | `speak_text` écrivait n'importe quelle réponse comme un son. | ✅ Type annoncé et premiers octets vérifiés. |
| G4 | Basse | `transcribe_audio` envoyait n'importe quel fichier lisible (8 Mio) au fournisseur. | ✅ Seul un son reconnu part, vérifié chez l'agent et chez le maître. |
| G5 | Basse | Images jointes par un enfant à une demande de texte : ni nombre, ni taille, ni format, ni modèle `vision` exigés. | ✅ Revérifié à chaque niveau. |
| G6 | Basse | Un appel de service pouvait attendre sans fin si le lien tombait au mauvais moment. | ✅ Inscription puis vérification du lien ; attente bornée ; compté, car il a pu aboutir. |
| G7 | Basse | Comptes divergents entre niveaux si le lien tombait pendant un appel. | ✅ Réglé par G1 et G6. |
| G8 | Basse | `spawn_agent` acceptait de confier d'autres modèles de conversation « en plus ». | ✅ Refusé : seuls des modèles de service ou `vision`. |
| G9 | Information | Une réponse de plongements de 8 Mio est analysée d'un bloc (mémoire) ; le texte rendu par un fournisseur entre dans le résultat d'outil sans balise, comme pour `read_file` ; somme de tokens sans saturation. | 🟡 Somme saturée. Le reste est noté : borné par le nombre d'appels simultanés, et couvert par la limite 3 (injection par le sens). |

### Cinquième passe (2026-11-21) : connexion aux fournisseurs depuis l'interface

Revue indépendante, à la lecture, des six routes de connexion et d'activation, du rangement des
clés, de l'OAuth et des droits du maître modifiés à chaud. Aucun constat critique ni haut : ni
fuite de clé, ni clé envoyée ailleurs qu'à l'adresse du catalogue, ni traversée de chemin, ni
droit en plus pour un enfant. Deux défauts moyens, traités et rejoués dans `tests/connect.rs`.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| H1 | Moyenne | Le nombre d'appels donné à l'activation remplaçait celui de la configuration, pour toute la nature, et restait après la désactivation : avec le seul jeton, on portait à un million les appels d'un modèle branché sur la clé de l'administrateur, ou on les coupait. | ✅ Un nombre fixé par la configuration ne se change pas d'ici. Les appels donnés d'ici partent avec le dernier modèle de leur nature activé d'ici, et ne reviennent au démarrage que pour un modèle repris. Test. |
| H2 | Moyenne | Un alias activé d'ici puis repris dans la configuration restait proposé à « Désactiver », qui retirait alors le modèle de la configuration jusqu'au redémarrage (le modèle du maître compris). | ✅ Le maître tient la liste de ce qu'il a réellement activé d'ici : elle seule se retire. La configuration gagne et l'activation en double est oubliée. Test. |
| H3 | Basse | OAuth : le retour `?code=` n'est lié à la demande que par PKCE, sans `state`. Un lien forgé ouvert pendant une autorisation en cours la fait échouer ; il ne connecterait le maître au compte d'un autre que si le fournisseur acceptait un code sans vérifier le défi. | 🟡 La marque de la page expire maintenant en dix minutes, comme chez le maître. Pas de `state` : le comportement d'OpenRouter sur une adresse de retour portant déjà une requête n'a pas pu être essayé. Limite 16. |
| H4 | Basse | Course de quelques microsecondes entre la création d'un enfant et une désactivation : l'enfant pouvait naître avec un alias retiré, puis s'en servir si l'alias était réactivé vers un autre modèle. | ✅ À chaque appel d'un enfant, le parent revérifie qu'il détient encore le modèle. |
| H5 | Basse | Une clé remplacée ne valait qu'au redémarrage pour les modèles déjà activés ; un fournisseur sorti du catalogue ne se déconnectait plus. | ✅ Les connecteurs sont refaits avec la nouvelle clé ; la déconnexion vaut pour tout fournisseur connecté d'ici. |
| H6 | Basse | `catalog_file` n'était pas contrôlé : identifiant servant de nom de fichier, adresses en clair, liens quelconques. | ✅ Identifiants en minuscules, chiffres et tirets ; adresses où part une clé en HTTPS ou sur la machine ; liens en HTTPS. Test, y compris sur le catalogue embarqué. |
| H7 | Information | Des erreurs de disque rendues par l'API citaient des chemins du dossier d'état ; une clé devenue illisible bloquait le démarrage ; un échec d'écriture laissait des appels accordés. | ✅ Message générique, détail sur la sortie d'erreur du maître ; modèle laissé de côté avec une note ; rien n'est accordé avant l'écriture. |
| H8 | Information | Quatre routes lisent le corps (2 Mo au plus) avant de vérifier le jeton, comme `/api/messages`. Le fournisseur « local » pointe sur un port de la machine : un process local qui l'occupe se ferait passer pour le serveur de modèles. | Accepté, noté : limite 17. |

Non vérifié par la revue : exécution sous Windows, comportement réel d'OpenRouter, effet d'un
proxy du système sur les appels du maître.

### Sixième passe (2026-11-21) : service, image Docker, publication et installation

Revue indépendante, à la lecture, avec un essai sous `systemd-analyze`. Aucun constat critique
exploitable sans erreur ou complicité de l'administrateur.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| I1 | Moyenne | Un chemin finissant par `\` joignait deux lignes de l'unité systemd : avec `--env-file`, la ligne `User=` était avalée et le maître tournait en root, sans message. | ✅ `\` refusé dans tout chemin hors Windows ; `User=` écrit en premier, avant toute ligne portant un chemin. Tests. |
| I2 | Moyenne | Le jeton d'exemple de la commande Docker était accepté tel quel : recopié puis exposé, il ouvrait le maître à qui a lu le README. | ✅ Un jeton donné mais faible ou recopié de l'exemple arrête le démarrage ; la documentation tire le jeton au hasard. Essayé. |
| I3 | Moyenne à basse | Installée en root, l'unité du système pointait vers des chemins qu'un autre compte pouvait remplacer (fichier d'environnement relu par root, exécutable, configuration). | ✅ Propriétaires et droits vérifiés sur chaque chemin et ses parents ; compte d'uid 0 refusé. Essayé. |
| I4 | Basse à moyenne | La publication à la main pouvait partir de n'importe quelle branche et devenir « la dernière version » ; une étiquette existante pouvait être reprise avec d'autres exécutables. | ✅ Seulement depuis une étiquette ou la branche principale ; refus si l'étiquette existe. 🟡 Protection des étiquettes et approbation d'un environnement : réglages du dépôt, à faire par son propriétaire. |
| I5 | Basse | `.dockerignore` excluait une liste : une clé ou une configuration posée dans le dossier entrait dans l'étage de construction. | ✅ Liste d'inclusion : seuls les fichiers que la compilation lit. |
| I6 | Basse | Actions et image de base désignées par étiquette mobile ; jeton laissé au dépôt cloné pendant la compilation. | 🟡 Jeton retiré du clone (`persist-credentials: false`). Épinglage par empreinte : à faire. |
| I7 | Basse | Durcissement systemd possible sans gêner les enfants. | 🟡 Ajouté à l'unité du système (capacités, noyau, horloge, groupes de contrôle). Unité du compte : seulement ce qui ne dépend pas des espaces de noms. Non essayé sur un service réel. |
| I8 | Basse | `--user` ne refusait que le mot « root » ; sans `HOME`, l'unité du compte s'écrivait dans le dossier courant ; `install.sh` lançait l'exécutable installé pour afficher sa version. | ✅ Uid vérifié ; dossier personnel absolu exigé ; plus aucun lancement. |

### Septième passe (2026-11-21) : le maître reçoit son modèle depuis l'interface

Revue indépendante, à la lecture. Aucun constat haut : la route ne donne au maître ni droit en
plus, ni modèle de service, et ne contourne pas un `model` fixé par la configuration ; un
enfant n'a aucune voie vers le modèle du maître ; un maître sans modèle n'ouvre rien de plus.

| N° | Gravité | Constat | Traitement |
|---|---|---|---|
| J1 | Moyenne | Le don d'office (« le premier modèle activé devient celui du maître ») valait aussi pour un maître qui avait perdu son modèle : sa conversation partait chez un autre fournisseur sans geste. | ✅ D'office seulement si le maître n'a jamais eu de modèle depuis son démarrage. Test. |
| J2 | Moyenne à basse | « Donner au maître » ne disait pas que la conversation en cours change de fournisseur. | ✅ Confirmation dans la page, ligne au journal, limite 18. |
| J3 | Moyenne à basse | Un modèle du fournisseur local, que tout programme de la machine peut imiter, pouvait devenir d'office le modèle du maître. | ✅ Jamais d'office pour un fournisseur local ; limite 17 complétée. |
| J4 | Basse | Un alias donné au maître puis repris par la configuration (autre fournisseur) était suivi en silence au redémarrage ; `master` survivait à la déconnexion d'un fournisseur dont le modèle n'était plus actif. | ✅ `master` effacé dans les deux cas. |
| J5 | Basse | Un agent permanent créé sans `model` suivait le modèle du maître à chaque redémarrage. | ✅ Le modèle résolu est écrit dans la demande gardée. |
| J6 | Basse | Un alias de la configuration hors du format des alias de l'interface ne pouvait pas être donné au maître ; `[models.""]` était accepté. | ✅ Seule l'appartenance aux droits compte ; alias vide refusé au chargement. |

Non vérifié par la revue : un fil mêlant les blocs d'outils de deux fournisseurs (essayé
seulement entre deux modèles du même faux fournisseur).

Non vérifié par la revue de la sixième passe : tout Windows (guillemets de `schtasks`, création de la tâche sans
élévation), `launchctl`, la construction de l'image, l'exécution du flux de publication.

