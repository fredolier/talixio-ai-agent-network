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
| G5 | Déploiement d'un enfant : sur la machine, dans Docker, par SSH, ou par un autre moyen d'accès distant. | 🟡 local, SSH et Docker (le maître seul déploie hors de sa machine ; même processeur ; exécutable statique pour ne pas dépendre de la distribution). Essayé avec un faux `ssh` ; pas encore sur une vraie machine ni un vrai démon Docker |
| G6 | Lien chiffré et authentifié dès qu'il quitte la machine (port HTTPS ou TCP ouvert par le parent). | ✅ TLS 1.3 partout (lien et API) ; l'enfant n'accepte que le certificat de son parent, épinglé au lancement ; authentification mutuelle liée à ce certificat |
| G7 | Lien coupé : l'enfant se met en veille ; si le parent ne revient pas, il s'éteint avec sa descendance. Jamais d'enfant autonome sans parent. | ✅ |
| G8 | Aucun message perdu pendant une coupure. | ✅ numérotation et acquittement |
| G9 | Un parent peut arrêter un enfant. | ✅ |
| G10 | Un parent peut désinstaller un enfant de la machine où il l'a déployé. | ✅ à l'arrêt de l'enfant ou du maître : exécutable, base et journal retirés de la cible ; conteneur supprimé |
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

### Fournisseurs, modèles et capacités

Demandé le 2026-11-20, sur le modèle de Talixio AI Assistant. Rien n'est codé.

| N° | Exigence | État |
|---|---|---|
| F1 | Le maître tient un catalogue de fournisseurs ; chaque fournisseur offre des modèles de plusieurs natures : texte (LLM), image, voix, et d'autres. | 🟡 Catalogue embarqué (`catalog.toml`, repris de l'assistant : 13 fournisseurs, 61 modèles, prévision et décision comprises, avec pays, Europe ou non, niveau de confiance, nature, qualité de 1 à 10, prix indicatifs), servi par `GET /api/models`. Écran « Fournisseurs » : une table, connecté ou non. Écran « Modèles » : par fournisseur, activé (alias du maître, qui l'utilise, à qui il est accordé) ou non. Un modèle configuré est rattaché à son fournisseur par la machine qui le sert ; jamais la clé. Reste : se connecter et activer depuis la page (aujourd'hui dans la configuration, voir Q10), appeler l'image, la voix, la prévision et la décision (F4 ; listés pour comparer), mise à jour du catalogue depuis les fournisseurs. |
| F2 | Un agent reçoit le droit d'utiliser tel modèle de tel fournisseur, nature par nature. Ses enfants n'en reçoivent jamais plus. | 🟡 fait pour les modèles de texte (`capabilities.models`) |
| F3 | À la création d'un enfant, le parent choisit son modèle, son niveau d'effort, la taille de son contexte, ses instructions, et ses capacités (texte, image, voix), en plus de ses droits sur les dossiers et les commandes. | 🟡 modèle, instructions et droits ; l'effort et le contexte sont fixés par l'alias, pas par enfant |
| F4 | Les appels d'image et de voix passent par le maître, comme ceux de texte : les clés ne descendent jamais, et chaque appel est compté dans le budget. | ⬜ voir Q10 |

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
| S2 | Chaque agent a une petite mémoire qui dure : ses consignes, ce qu'il a appris, où il en est. Elle survit à un redémarrage. | 🟡 chaque agent a sa base SQLite et y tient ses notes (`memory_write`, `memory_read`), relues dans son prompt au démarrage. Celle du maître survit aux redémarrages ; celle d'un enfant dure le temps de sa vie, tant que les agents permanents n'existent pas |
| S3 | Le journal et l'état de la grappe survivent à un redémarrage du maître. | 🟡 journal sur disque (SQLite), repris au démarrage ; l'arbre et les conversations ne le sont pas encore |

### Pilotage

| N° | Exigence | État |
|---|---|---|
| P1 | API JSON au niveau du maître, utilisable par Talixio AI Assistant. | ✅ |
| P2 | Interface web soignée, faite avec la bibliothèque shadcn/ui personnalisée, à l'identité propre : la grappe en direct, chaque agent avec nom, rôle, machine, IP, état ; le fil des messages ; le dialogue avec le maître ; les commandes et à qui elles sont confiées ; les fournisseurs et modèles ; les rôles. | ✅ servie par le maître à `/` (React, shadcn/ui, Tailwind ; décision 22) : Grappe, Dialogue, Journal, Commandes, Modèles, Rôles, Nouveautés ; clair et sombre ; téléphone. Reste : revue de sécurité indépendante de l'interface |
| P3 | Vue de la grappe et du journal en ligne de commande. | ✅ |
| P5 | Notes de version, tenues à chaque version, lisibles en ligne de commande et dans l'interface web. | 🟡 `releases.toml` embarqué, `talixio-agent releases`, `GET /api/releases` ; écran « Nouveautés » de l'interface |
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
20. **Les notes de version ont une source unique**, `releases.toml`, embarquée dans l'exécutable.
    Chaque version y ajoute une entrée ; un test refuse une version sans note.

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
| Enfant → parent | `hello` (identité, aléa), `proof` (preuve, état), `tree` (état de sa branche), `report` (fin de mission, question, erreur), `activity` (journal), `model_request` |
| Parent → enfant | `challenge` (aléa, preuve), `welcome` / `rejected`, `instruction`, `model_response`, `stop_descendant`, `shutdown` |
| Les deux | `ping` / `pong`, `ack` |

Les messages portant un `seq` sont acquittés et renvoyés après une reconnexion ; le destinataire
écarte les doublons. Délais réglables (`[timing]`) : battement, mise en veille, extinction.

Présentation : `hello` → `challenge` → `proof` → `welcome`, en 10 s au plus. La preuve de chacun
est `HMAC-SHA256(secret, rôle ‖ aléa de l'enfant ‖ aléa du parent ‖ identifiant ‖ empreinte du certificat du parent)`. Huit
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
| `memory_write`, `memory_read` | toujours |

### API JSON du maître

En HTTPS seulement. Authentification : `Authorization: Bearer <jeton>`. Le jeton vient de `TALIXIO_API_TOKEN`, sinon il
est créé au premier démarrage dans `<state_dir>/api-token`.

| Méthode | Chemin | Rôle |
|---|---|---|
| GET | `/`, `/app.js`, `/app.css`, `/sans.woff2`, `/mono.woff2`, `/icon.svg`, `/manifest.webmanifest` | L'interface web (sans jeton). |
| GET | `/api/health` | Vie du maître (sans jeton). |
| GET | `/api/tree` | La grappe : chaque agent avec nom, rôle, état, machine, OS, IP, droits, tokens, enfants. |
| GET | `/api/activity?after=N&limit=M` | Le journal après l'événement N. |
| GET | `/api/events?after=N` | Le journal en continu (Server-Sent Events). |
| POST | `/api/messages` `{ "text": … }` | Un message au maître. |
| POST | `/api/agents/{id}/stop` | Arrête un agent et sa descendance. |
| GET | `/api/releases` | La version qui tourne et les notes de version. |
| GET | `/api/models` | Les modèles du maître (`models`, `default`) et le catalogue : fournisseurs connus, connectés ou non (`providers`), modèles, activés ou non (`catalog`). Jamais de clé. |
| GET | `/api/roles` | Le répertoire des rôles (500 fiches au plus). |

## 6. Questions ouvertes

| N° | Question | Piste |
|---|---|---|
| Q1 | ~~Où tourne le maître ?~~ | Tranché : sur une VM toujours allumée (décision 15). |
| Q2 | ~~Droits d'un enfant sur une autre machine~~ | Tranché : un plafond de droits par cible de déploiement (décision 17). |
| Q3 | ~~Chiffrement du lien~~ | Tranché : certificat du parent épinglé chez l'enfant (décision 9). |
| Q4 | Service ou démon (A5, A6) : systemd, launchd, service Windows ; un agent « hôte » par machine qui lance les autres. | À étudier à l'étape 3. |
| Q5 | Assistant sur poste de travail : il observe une personne. | Visible et accepté par elle ; étape à part. |
| Q6 | Coffre des clés au niveau 0. | Fichier chiffré ou coffre du système, à choisir. En attendant : fichier de clé en 0600. À terme, les clés devraient vivre sous un autre compte que celui des agents. |
| Q7 | Stockage et mémoire des agents (S1–S3) : dossier, SQLite, ou les deux ? | Tranché pour la mémoire et le journal : SQLite embarqué, pour tous les agents (décision 14). Reste à faire : un dossier de travail par agent, ajouté d'office à ses droits (S1) ; l'arbre et les conversations sur disque (S3) ; effacement de la base d'un enfant à sa désinstallation ; mémoire partagée entre agents, si un usage le demande. |
| Q8 | Abonnement Claude Max au lieu d'une clé d'API (B5). | À vérifier avant tout : qu'un abonnement Max puisse servir à des agents tiers n'est pas acquis, ni techniquement ni dans les conditions d'Anthropic. La voie sûre est la clé d'API avec un solde. Si c'est permis : lire les limites renvoyées par le fournisseur à chaque réponse, et les traiter comme un budget qui se recharge. |
| Q9 | Android (P4) : piloter depuis le téléphone, ou y faire tourner un agent ? | Piloter : l'interface web, installable comme application (PWA), suffit ; elle demande HTTPS et un maître joignable depuis le téléphone, donc l'étape 2. Agent sur le téléphone : Rust se compile pour Android, mais le système arrête les process d'arrière-plan et enferme chaque application ; à n'étudier que si un usage le demande. |
| Q10 | Catalogue de fournisseurs et capacités image et voix (F1–F4). | Format de l'assistant relu (`src/ModelCatalog.php`, `Sovereign.php`, `Routing.php`, `Pricing.php`) : une table de fournisseurs (identifiant, nom, pays, niveau de confiance, adresse, mode de facturation) et une table de modèles (identifiant exact de l'API, fournisseur, nature `LLM`, `Vision`, `Voice`, `Image`, `Embedding`, prix par million de tokens ou par unité : minute, image). Usages liés à un modèle : `fast`, `standard`, `deep`, `voice`, `vision`, `embedding` ; effort et tokens de réflexion portés par le palier. À reprendre ici : `[providers.<nom>]` (adresse, type d'API, fichier de clé) et des modèles par nature, avec prix ; droits de la forme `fournisseur/modèle`. À savoir : l'assistant ne tient ni taille de contexte ni effort par modèle, ne compte que les tokens (minutes et images sont affichées, pas comptées), et n'appelle aucune API d'image ni de synthèse vocale : seules la transcription (`POST /audio/transcriptions`) et les embeddings existent. Image et synthèse sont donc à écrire, pas à reprendre. Côté maître, du code par nature : nouveaux messages du lien, tailles bornées autrement (un son ou une image pèse lourd), comptage en argent (B4), nouveaux outils chez l'agent. Nouvelle surface : revue de sécurité avant de coder. |
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
   reste du réseau de la machine. À fermer par un réseau Docker dédié, côté machine.
9. **Lancer `docker` revient à être root sur la machine** : le maître d'une cible Docker doit
   être tenu pour tel. Le socket Docker n'est jamais montré à un conteneur d'agent.
10. Un agent déployé qui ne répond plus (machine éteinte, réseau coupé) s'éteint seul, mais sa
    désinstallation échoue si la cible est injoignable : l'échec est journalisé, pas rejoué.

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
