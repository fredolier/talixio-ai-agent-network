# Exigences et critères d'acceptation

Liste exhaustive de ce qui a été demandé pour Talixio Agent Network, tirée des messages de
l'utilisateur (depuis le 20 novembre 2026), relus un à un. Les demandes venues après la première
vérification portent les numéros D110 et suivants. Chaque demande porte la phrase exacte
qui la fonde, la référence de la spécification (`SPEC.md`) qui la traite, et le critère qui
permet de dire si elle est tenue.

- **Demandé** : exprimé comme une exigence. **À étudier** : évoqué, à trancher.
- L'état de chaque demande n'est pas écrit ici : il est établi par une vérification
  indépendante, dans [`VERIFICATION.md`](VERIFICATION.md). Ce document-ci ne change que si une
  demande s'ajoute ou si un critère est précisé.
- Les citations viennent d'une dictée vocale : elles sont reprises telles quelles.
- Rien de ce document ne vient d'ailleurs que des messages de l'utilisateur. Ce qui a été
  construit sans avoir été demandé est listé dans `VERIFICATION.md`.


## 1. Agent et exécutable

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D1 | Les agents sont écrits en Rust. | « des agents Rust » | A1 | Le dépôt est un projet Cargo ; `cargo build --release` produit `talixio-agent` sans autre langage à l'exécution. |
| D2 | Chaque agent est un petit processus qui tourne sur n'importe quelle machine et n'importe quel système d'exploitation. | « c'est un petit process qui tourne sur n'importe quelle machine, n'importe quel OS » | A2 | Les tests passent sur Linux, macOS et Windows (intégration continue verte sur les trois). |
| D3 | On donne à chaque agent des instructions et la liste des commandes qu'il a le droit de jouer. | « auquel on donne des instructions, et on donne aussi des commandes qu'il peut jouer » | D1, D2 | La configuration d'un agent porte des instructions et une liste de règles de commande ; une commande hors liste est refusée par `run_command` (test). |
| D4 | Chaque agent a un nom et un rôle. | « Chacun aura un nom, un rôle. » | G11 | `/api/tree` renvoie `name` et `role` pour chaque agent ; ils sont obligatoires à la création d'un enfant. |
| D5 | L'adresse IP de chaque agent doit pouvoir être récupérée (les agents sont répartis sur plusieurs machines). | « Il faut qu'ils puissent récupérer l'adresse IP » | G11 | `/api/tree` renvoie, pour chaque agent, sa machine, son OS et ses adresses IP ; l'interface les affiche. |
| D6 | L'agent déployé se réduit à un seul exécutable et quelques fichiers de configuration. | « je m'attends à juste un exécutable et quelques fichiers de configuration » | A1 | Lancer le maître ne demande que l'exécutable et un fichier TOML ; un enfant ne demande que l'exécutable (sa configuration lui est transmise). |
| D7 | La taille de l'exécutable doit être optimisée. | « Optimise la taille de l'exécutable. » | A4 | Profil de compilation optimisé en taille (`opt-level = "z"`, LTO, strip) ; la taille de l'exécutable est notée dans la spec. |
| D8 | La partie interface fait exception à la règle « un exécutable + config » : elle peut embarquer du HTML et du CSS. | « Sauf pour la partie interface. » | A1, P2 | L'interface est servie par le maître ; ses fichiers sont embarqués dans l'exécutable (pas de dossier à déployer à côté). |
| D9 | L'exécutable doit être compilé statiquement, en minimisant les dépendances à la glibc, car il tournera sur des distributions variées. | « Il faudrait une compilation statique » | décision 21 | `cargo build --release --target x86_64-unknown-linux-musl` réussit ; `file` dit « static » ; les tests passent sur cette cible (intégration continue). |
| D10 | L'intelligence du système réside dans les prompts, la communication et les outils donnés à chaque agent ; la technique doit rester simple. | « toute l'intelligence ce sera dans les prompts » | §1 | Non mesurable : orientation de conception. Vérifier seulement que le prompt système décrit rôle, droits, mission et interlocuteurs. |

## 2. Droits et sécurité

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D11 | Un agent ne doit pas avoir accès à tout : la gestion des droits doit être très rigoureuse. | « il faut être très rigoureux dans le sens où il ne faut pas qu'il ait accès à tout » | D1 | Sans section `[capabilities]`, un agent ne peut lire, écrire, exécuter ni créer d'enfant (test). |
| D12 | Des outils sont à mettre en place : lire des fichiers, éventuellement écrire des fichiers, éventuellement exécuter des commandes. | « il aura le droit de lire, peut-être d'écrire des fichiers, peut-être d'exécuter des commandes » | §5 Outils | Les outils `read_file`, `list_dir`, `write_file`, `run_command` existent ; chacun est refusé sans le droit correspondant (tests). |
| D13 | Chaque agent ne reçoit qu'une partie des outils et des droits, différente selon l'agent. | « Ils ne les auront pas tous, donc des agents auront certains droits, mais pas tous. » | D1, D2 | Un agent ne voit dans sa liste d'outils que ceux que ses droits permettent ; deux agents de droits différents ont des listes différentes. |
| D14 | Le droit de créer (instancier) de nouveaux agents est lui-même un droit accordé ou non à chaque agent. | « suivant les droits qu'on aura donnés à ces agents, ils auront droit ou il aura droit de créer ou d'instancier des nouveaux agents » | D3 | Sans `capabilities.spawn`, l'outil `spawn_agent` n'est pas proposé et son appel est refusé. |
| D15 | L'agent 0 a à peu près tous les droits et décide de ce que chaque enfant peut faire. | « l'agent 0, il aura à peu près tous les droits et il pourra dire bah, tiens, toi tu peux faire ça » | D2 | Les droits du maître viennent du fichier de configuration écrit par l'humain ; c'est lui qui accorde ceux de ses enfants. |
| D16 | Un enfant peut à son tour créer des agents et leur donner des droits, et ainsi de suite à chaque niveau. | « pourra aussi créer d'autres trucs et lui donner des droits, etc. » | G1, D3 | Un enfant doté de `spawn` crée à son tour un enfant ; profondeur et nombre sont bornés par `max_depth` et `max_children` (test). |
| D17 | Toute la configuration d'un agent est faite par l'agent (son créateur), sans intervention manuelle. | « Donc toute la configuration se fait par l'agent. » | G1 | Créer un enfant ne demande aucun fichier ni geste humain : tout passe par les arguments de `spawn_agent`. |
| D18 | Un agent peut demander à son parent de l'aider pour la configuration. | « Il peut éventuellement demander à son parent de l'aider dans la configuration » | G3 | L'outil `ask_parent` existe et sa question arrive au parent (test de bout en bout). |
| D19 | Un agent spécialisé ne reçoit que les outils et commandes strictement nécessaires à son rôle. | « On lui mettra les outils qui va bien, les commandes, juste les commandes qu'il faut » | D2 | Les droits d'un enfant sont ceux, et seulement ceux, que le parent a écrits dans `spawn_agent` ; rien n'est hérité d'office. |
| D20 | Les secrets (clés) sont dans un coffre (Vault) situé uniquement au niveau supérieur, le niveau 0 ; les enfants passent par leur parent, et la demande remonte de parent en parent jusqu'en haut. | « on a le volt qu'au niveau supérieur, le premier niveau 0 » | D6, Q6 | Les clés des modèles n'existent que chez le maître ; l'environnement et la configuration d'un enfant n'en contiennent aucune (test) ; les appels de modèle remontent de parent en parent. Un coffre chiffré dédié : à vérifier séparément. |
| D21 | La sécurité est primordiale : code, connexions, cloisonnement, et garantie que l'agent ne fasse pas n'importe quoi. | « la sécurité qui sera juste primordiale » | §7 | Une revue de sécurité indépendante existe pour chaque surface (lien, outils, déploiement, interface) et ses constats sont traités ou inscrits. |
| D22 | Aucune demi-mesure sur la sécurité : les liens entre agents doivent être chiffrés (obligatoire). | « on peut pas se permettre d'avoir des demi-mesures pour la sécurité là. Donc ça, c'est un must. » | G6 | Une connexion en clair au lien ou à l'API échoue ; un enfant refuse un parent dont le certificat n'est pas celui attendu (tests). |
| D23 | L'agent reçoit une configuration chiffrée, qu'il ne peut déchiffrer qu'avec une clé donnée par son parent. | « il faudrait que l'agent reçoive une configuration chiffrée et qu'il ne peut décoder qu'avec une clé que le parent lui donne » | décision 23 | L'enveloppe transmise à l'enfant ne contient ni droits ni instructions lisibles ; elle ne s'ouvre qu'avec la clé remise par le parent (tests). |
| D24 | Un agent ne doit pas pouvoir modifier lui-même sa configuration. | « Comme ça, il ne peut pas modifier lui-même la configuration. » | décision 23 | Un octet changé dans la configuration scellée, ou dans l'enveloppe, et l'agent refuse de démarrer (test). Aucun outil ne permet à un agent de changer ses propres droits. |
| D110 | Un enfant ne peut jamais lire directement sa configuration : il doit passer par son parent, qui lui remet la clé de déchiffrement. | « chaque enfant ne peut pas lire directement la configuration et qu'il doit absolument passer par le parent qui lui donne une clé de déchiffrement » | décision 23 | (1) Ce que l'enfant reçoit au lancement ne contient en clair que son identifiant, l'adresse et le certificat du parent et son jeton de présentation : ni rôle, ni instructions, ni droits (test). (2) Sans la clé, la configuration ne s'ouvre pas ; avec une autre clé non plus (test). (3) La clé n'existe que chez le parent, n'est écrite nulle part chez l'enfant, et ne lui est remise que sur le lien chiffré, après authentification mutuelle. (4) Elle n'est remise qu'une fois : une seconde demande est refusée (test). (5) Parent injoignable : l'enfant ne démarre pas. |
| D25 | Un agent ne doit disposer d'aucune liberté au-delà de ce que son parent lui a donné ; ce point doit être « béton ». | « Il ne faut pas laisser de liberté supplémentaire à l'agent, autre que ce que le parent lui a donné. » | D2, D5 | `check_grant` refuse tout droit que le parent n'a pas (test d'escalade) ; chaque appel d'outil revérifie les droits. Limite à juger : sans cloisonnement par le système (D5), une commande autorisée a les droits du compte. |

## 3. Grappe, lien et déploiement

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D26 | Chaque agent peut être l'orchestrateur (maître) d'autres agents, créés avec des rôles et des droits différents. | « chaque agent peut être l'orchestrateur ou le master d'autres agents » | G1 | Tout agent doté de `spawn` peut créer et diriger des enfants, pas seulement le maître. |
| D27 | Un enfant ne peut communiquer qu'avec son parent : c'est son seul moyen de communication. | « un enfant ne peut discuter qu'avec son parent » | G2 | Un enfant n'a qu'une connexion, vers son parent ; aucun outil ne lui permet de s'adresser à un autre agent que son parent ou ses propres enfants. |
| D28 | Les agents qui créent des agents forment une grappe, déployée en étoile. | « ça fait une grappe, ça fait donc un déploiement en étoile » | §1 | La grappe est un arbre : chaque agent a exactement un parent ; `/api/tree` le montre. |
| D29 | L'agent maître doit pouvoir recevoir tous les messages des enfants. | « un agent maître qui va pouvoir recevoir tous les messages des enfants » | G2 | L'activité de tout descendant apparaît dans le journal du maître (test de bout en bout). |
| D30 | Un nouvel agent peut être créé sur la machine elle-même, dans un conteneur Docker, ou via SSH. | « soit sur la machine elle-même, ou dans un Docker, ou en SSH » | G5 | `spawn_agent` accepte `local`, une cible `ssh` et une cible `docker`. |
| D31 | Lever les limitations constatées sur les déploiements : commencer à traiter Docker et SSH. | « ça vaudrait le coup de commencer à regarder ça pour Docker et SSH » | G5 | Les déploiements SSH et Docker sont codés et testés ; noter séparément ce qui est testé pour de vrai et ce qui l'est avec des programmes factices. |
| D32 | Les coupures de connexion entre agents enfants et parents doivent être gérées. | « Il faut gérer aussi si les connexions se coupent entre les agents fils et parents. » | G7, G8 | Après une coupure, l'enfant se reconnecte et aucun message n'est perdu (numérotation, acquittement). |
| D33 | Un enfant ne doit jamais rester en autonomie sans pouvoir joindre son parent : il se met en veille, puis s'éteint (meurt) si le signal ne revient jamais. | « il devrait soit se mettre en veille en attendant, et si jamais le signal ne revient jamais, il s'éteint, il meurt » | G7 | Parent tué : l'enfant passe en veille, puis s'éteint après le délai `die_after_secs` (test de bout en bout). |
| D34 | Hypothèse de réseau retenue : le parent installe et lance l'enfant par le moyen de son choix ; une fois lancé, l'enfant doit pouvoir voir (joindre) le parent. | « une fois que l'enfant est lancé, il doit pouvoir voir le parent » | G4 | C'est l'enfant qui ouvre la connexion vers le parent ; le parent n'a jamais besoin de joindre l'enfant. |
| D35 | Un parent peut ouvrir un port HTTPS bien sécurisé, ou un canal TCP sur un port particulier, pour que l'enfant le joigne ; le choix technique est laissé à l'assistant. | « je serais pas choqué qu'un des parents ouvre un port HTTPS, donc bien sécurisé évidemment » | G6 | Le parent écoute sur un port TLS ; adresse et nom annoncé sont réglables (`listen`, `advertise`). |
| D36 | Un parent peut demander à un enfant de s'arrêter. | « Et un parent peut demander aussi à l'enfant de s'arrêter » | G9 | `stop_agent` et `POST /api/agents/{id}/stop` arrêtent un agent et sa descendance (test). |
| D37 | Un parent peut demander à un enfant de se désinstaller complètement. | « ou de se désinstaller même carrément » | G10 | Arrêter un agent déployé retire de la cible son exécutable, sa base et son journal (test). |
| D38 | L'agent de niveau 0 tournera sur un serveur (une VM) de l'utilisateur, et non sur son poste. | « je mettrai ça sur un serveur que j'ai » | décision 15 | Le maître peut écouter hors de sa machine et être piloté à distance ; la spec dit qu'il tourne sur une VM. |
| D39 | Les enfants remontent des diagnostics à leur parent, qui décide de la suite (par exemple envoyer un autre agent corriger). | « remonter des diagnostics aux parents » | G3 | Les comptes rendus d'un enfant (`finish_mission`, `ask_parent`) arrivent dans la conversation du parent, qui peut créer un autre agent en réponse. |

## 4. Missions et autonomie

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D40 | Chaque agent est autonome : une fois une directive, un objectif ou un chat reçu du parent, il travaille tant que l'objectif n'est pas atteint. | « tant que la tâche ou l'objectif n'est pas atteint, il faudrait qu'il fasse ce qu'il a à faire » | M1 | Un agent qui reçoit une mission enchaîne les appels d'outils jusqu'à `finish_mission`, sans relance humaine. |
| D41 | Pour atteindre son objectif, l'agent itère, questionne, et peut demander des informations à son parent. | « il faudra qu'il itère, qu'il questionne, potentiellement qu'il demande à son parent des informations » | M1, G3 | L'agent dispose de `ask_parent` pendant sa mission. |
| D42 | Un agent chargé d'une mission peut créer lui-même l'équipe de sous-agents nécessaire, sur son poste ou sur un autre. | « il va créer l'équipe, tous les sous-agents qu'il faut, alors soit sur le poste, soit sur un autre poste » | G1, G5 | Un agent en mission peut créer des enfants, en local ou sur une cible. |
| D43 | Tant qu'il n'a pas réussi, l'agent doit chercher une autre solution pour y arriver. | « tant qu'il n'a pas réussi, ben, il devra trouver une solution pour le faire » | M1 | Le prompt demande de chercher une autre voie avant de déclarer la mission impossible. L'accès au web n'est pas un outil : exigence M5 à part. |
| D44 | Chaque mission comporte un objectif, et cet objectif doit être mesurable. | « il faut qu'il ait un objectif mesurable aussi » | M2 | `spawn_agent` exige `objective` et `success_criteria` ; sans critère, la création est refusée. |
| D45 | Quand l'objectif mesuré est atteint, ou quand l'agent constate qu'il ne peut pas y arriver, il se met en pause et rend la main à son parent. | « il se met en pause pour le parent » | M3 | `finish_mission` accepte `achieved` ou `impossible`, envoie le compte rendu au parent, puis l'agent attend. |
| D46 | Le parent peut redonner des instructions à un agent en pause. | « le parent peut lui redonner des instructions » | M3 | `send_to_agent` relance un enfant en attente avec une nouvelle consigne. |
| D47 | Cas d'usage : créer des agents spécialisés pour un projet. | « si j'ai un projet, je pourrais créer des agents spécialisés » | — | Cas d'usage : réalisable si D16, D19 et D44 passent. |
| D48 | Cas d'usage : demander un agent de surveillance/audit de sécurité d'une VM ; le parent se connecte à la VM, y installe l'enfant spécialisé, qui vérifie, diagnostique et remonte. | « crée-moi un agent de surveillance de la sécurité de telle VM » | G5 | Cas d'usage : réalisable si le déploiement SSH passe sur une vraie machine. À marquer « non démontré » tant qu'aucun essai réel n'a eu lieu. |
| D49 | Cas d'usage : le parent peut ensuite envoyer un nouvel agent pour corriger les problèmes de sécurité, ou optimiser les systèmes de fichiers, etc. | « le parent va décider de renvoyer un nouvel agent qui va lui faire le fixe des problèmes de sécurité » | G1 | Cas d'usage : réalisable si D39 passe. |
| D50 | Cas d'usage : optimiser quelque chose sur un serveur ; promouvoir une marque. | « si un agent on lui demande de promouvoir une marque » | M5 | Cas d'usage : « promouvoir une marque » suppose l'accès au web, qui n'existe pas. |

## 5. Modèles, fournisseurs, capacités

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D51 | Les agents peuvent se connecter à n'importe quel type de modèle, comme dans Talixio AI Assistant (dépôt fredolier/talixio-ai-assistant donné en référence). | « ils peuvent se connecter à n'importe quel type de modèle, comme on l'a déjà fait sur Talixio AI Assistant » | A3 | Fournisseurs `anthropic` et `openai` (et tout serveur compatible OpenAI) ; tests des deux formats de requête. |
| D52 | Reprendre le principe de réglage de l'assistant : une liste de fournisseurs, chacun offrant des modèles (image, voix, LLM). | « j'aime assez la façon dont on a fait ça dans l'assistant. On a toute une liste de fournisseurs. » | F1 | Un catalogue fournisseurs → modèles par nature (texte, image, voix) existe dans la configuration. |
| D53 | On doit pouvoir mettre à disposition des agents tel modèle de tel fournisseur. | « on pourrait mettre à disposition aux agents tel modèle de tel provider, moi j'aime bien » | F2 | Un droit désigne tel modèle de tel fournisseur ; un agent ne peut appeler que ceux-là (test). |
| D54 | À la création d'un enfant, le parent lui attribue un modèle, un effort, un contexte et des prompts. | « il faudra bien leur donner un modèle, un effort, un contexte, des prompts » | F3 | `spawn_agent` permet de fixer, par enfant : modèle, effort, taille de contexte, instructions. |
| D55 | Chaque agent a des capacités propres (en plus de ses droits sur les commandes), différentes d'un agent à l'autre. | « ils auront chacun des capacités différentes aussi, en plus des droits sur certaines commandes » | F2, F3 | Les capacités (texte, image, voix) font partie des droits d'un agent et diffèrent d'un agent à l'autre. |

## 6. Budgets et quotas

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D56 | Faire attention aux quotas et aux tokens. | « il faut faire attention aux quotas, aux tokens » | B1 | Chaque appel de modèle est compté ; `/api/tree` montre les tokens consommés par agent. |
| D57 | Avec une clé API prépayée, chaque agent a un budget. | « chaque agent aura un budget » | B1, B4 | Un agent a un plafond (`max_tokens`) appliqué par son parent ; un budget en argent existe aussi. |
| D58 | Chaque agent doit pouvoir répartir son budget entre ses enfants. | « il faudra qu'il soit capable de dispatcher ce budget à ses sous-agents » | B2 | Un parent ne peut pas donner à ses enfants plus que ce qui lui reste ; la somme des budgets des enfants est bornée. |
| D59 | Avec le compte Anthropic Claude Code Max, il faut un moyen de voir les quotas pour pouvoir les répartir. | « il faudra qu'on ait un moyen de voir les quotas » | B5, Q8 | Les quotas d'un abonnement sont lisibles par le maître. À défaut : la question est tranchée et écrite dans la spec. |
| D60 | La notion de quota doit figurer dans les instructions données aux agents lorsqu'ils répartissent le travail : pas de consommation libre. | « il faut qu'il ait la notion de quota, qu'il ne peut pas aller freestyle sur tout » | B3 | Le prompt de chaque agent lui dit son budget et ce qui lui reste, et lui demande de le répartir. |
| D61 | Quand un agent atteint un certain quota, il prévient son parent et s'arrête. | « quand il a atteint un certain quota, ben, il prévient son parent et il s'arrête » | B3 | À un seuil réglable, l'agent prévient son parent et s'arrête. |
| D62 | Tout cela fonctionne en cascade : chaque parent remonte à son propre parent, jusqu'en haut. | « tout ça, ça doit se passer en cascade. Le parent remonte au parent, du parent, etc. » | B6 | L'alerte de budget d'un petit-enfant remonte jusqu'au maître ; la consommation d'un descendant est comptée dans celle de chacun de ses ascendants. |

## 7. Stockage, mémoire, cache

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D63 | Réfléchir à la façon dont les agents stockent leurs fichiers de travail (répertoire ou sous-répertoire, base SQLite, ou les deux), en restant simple. | « reste simple, mais réfléchis à ça » | S1, Q7 | La question est tranchée dans la spec ; chaque agent a un dossier de travail à lui. |
| D64 | Les agents ont besoin d'une petite mémoire et d'un endroit où garder des instructions. | « les agents ont besoin d'avoir une petite mémoire, des instructions, ce genre de choses » | S2 | Outils `memory_write` et `memory_read` ; les notes sont relues au démarrage (test). |
| D65 | Inclure par défaut la possibilité d'utiliser SQLite dans l'agent, si ce n'est pas trop lourd. | « je pense qu'inclure SQLite par défaut, ça pourra toujours servir » | décision 14 | SQLite est compilé dans l'exécutable (`rusqlite`, `bundled`) et utilisé par tout agent ; le surpoids est noté. |
| D66 | Pour la mémoire des agents, préserver le cache du LLM : les nouvelles informations sont ajoutées à la fin du contexte. | « Il faut bien que on rajoute à la fin les nouvelles informations, sinon le cache il n'est pas pris en compte côté LLM » | décision 16 | Le prompt système d'un agent ne change pas pendant sa vie ; notes et messages nouveaux s'ajoutent à la fin de la conversation ; la requête Anthropic porte `cache_control`. |
| D67 | Les rôles, prompts et directives créés par les agents remontent à l'agent 0, dans une sorte de base de données. | « il faudrait remonter ça à l'agent 0, dans une sorte de base de données » | R1 | Les rôles créés dans la grappe sont enregistrés chez le maître, dans une base qui dure. |
| D68 | Cette base est interrogeable par les enfants et les enfants des enfants, pour retrouver des rôles existants selon le contexte. | « qui peut être interrogée par les enfants et les enfants des enfants pour trouver des rôles qui existent déjà selon les contextes » | R2 | Un outil permet à tout agent de chercher un rôle existant. |
| D69 | Prévoir quelque chose pour l'apprentissage (réutilisation de ce que la grappe a déjà défini). | « il y a quelque chose à faire aussi pour l'apprentissage » | R3 | Le résultat des missions est rattaché aux rôles. |

## 8. Journaux

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D70 | L'agent fait lui-même la rotation de ses journaux pour en limiter la taille. | « faire un sorte de log rotate directement par l'agent pour limiter la taille des logs et des journaux » | décision 24 | Un journal qui dépasse la limite tourne (`.1`) et repart de zéro (test). |
| D71 | Les réglages de cette rotation sont donnés par le parent, et viennent donc in fine de l'agent 0. | « c'était aux parents de lui donner ça, donc qui viendrait de l'agent 0 in fine » | décision 24 | La limite vient de `log_max_bytes` du maître et se retrouve dans la configuration de chaque descendant. |

## 9. Pilotage : API, interface web, téléphone

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D72 | Au niveau supérieur, une interface utilisateur montre la grappe d'agents : tout ce qui est déployé et instancié. | « il faudrait que je puisse voir cette grappe d'agents » | P2 | L'interface montre l'arbre des agents en direct, avec leur état. |
| D73 | Une interface web suffit (pas d'intégration dans une autre application), et elle doit être jolie. | « Une interface web est suffisante, un peu jolie évidemment. » | P2 | L'interface est une page web servie par le maître, utilisable sans autre application ; captures dans le dépôt. |
| D74 | S'appuyer sur shadcn pour l'interface et soigner le rendu (pas « sloppy »). | « je veux bien que tu passes sur du CHACDN » | P2, décision 22 | L'interface utilise shadcn/ui. À juger : elle en reprend l'allure mais n'utilise pas la bibliothèque. |
| D75 | Fournir une API JSON, pour que l'assistant de l'utilisateur (Talixio AI Assistant) puisse éventuellement s'y connecter. | « il faudra surtout une interface JSON API aussi pour que éventuellement mon assistant puisse se connecter » | P1 | L'API JSON est documentée dans la spec et couverte par le test de bout en bout ; elle suffit à piloter le maître. |
| D76 | Montrer une maquette graphique de l'interface proposée. | « T'as une maquette à me montrer là de ce que tu as proposé graphiquement? » | — | Demande ponctuelle : une maquette a été publiée et montrée. |
| D77 | Faire quelque chose de joli (interface). | « fais un truc joli » | P2 | Subjectif : captures à l'appui, bureau et téléphone, clair et sombre. |
| D111 | L'interface utilise la bibliothèque shadcn/ui elle-même, personnalisée, la moins « sloppy » possible, avec une identité propre. | « ce sera utiliser la librairie, ouais, la bibliothèque elle-même […] Il faut la customiser et la rendre moins sloppy possible que notre interface ait une identité propre à elle » | P2, décision 22 | `web/components.json` et `web/src/components/ui/` viennent de shadcn/ui ; le thème n'est pas celui par défaut (couleurs, polices, formes définies dans `web/src/index.css`) ; captures dans `docs/images/`. |
| D112 | Voir dans l'interface les fournisseurs d'API et les modèles disponibles. | « travaille aussi d'abord sur les fournisseurs d'API et les modèles disponibles dans l'interface, que je puisse les visualiser » | F1, P2 | `GET /api/models` (test) ; écran « Modèles » : modèles regroupés par fournisseur, sans aucune clé ; pour chacun, qui l'utilise et à qui il est accordé. |
| D113 | Voir les rôles dans l'interface. | « et également les rôles » | R1, P2 | `GET /api/roles` (test) ; écran « Rôles » : chaque agent créé, à tout niveau, avec rôle, modèle, parent et droits ; le répertoire survit à un redémarrage du maître. |
| D114 | Voir toutes les commandes qui existent : le maître en détient la liste et peut les déléguer ou non aux enfants. L'utilisateur veut maîtriser cela et le voir. | « toutes les commandes qui existent, puisque du coup c'est le maître qui aura la liste des commandes et qui pourra les déléguer ou non aux enfants. Donc je veux bien maîtriser tout ça et le voir dans l'interface » | D2, D3, P2 | Écran « Commandes » : chaque règle du maître, telle qu'elle est vérifiée ; pour chacune, les agents à qui elle est confiée, telle quelle ou restreinte ; les règles à arguments libres sont signalées. Un enfant ne peut recevoir qu'une règle que son parent détient (test d'escalade). |
| D115 | Mesurer la « sloppiness » de l'interface avec un outil existant. | « Peux-tu scorer la sloppiness de ton interface? Je crois qu'il existe des repos GitHub qui permettent de faire ça » | — | Une mesure est faite avec un outil public nommé, et son résultat est consigné. À défaut : dire lequel n'a pas pu être exécuté et pourquoi. |
| D116 | Une table de tous les fournisseurs auxquels on peut se connecter : européen ou non, connecté ou non. | « on a vraiment une liste, une table avec tous les fournisseurs auxquels on peut se connecter […] s'ils sont européens, pas européens. On peut se connecter, on voit si on est connecté » | F1 | L'écran « Fournisseurs » liste chaque fournisseur du catalogue avec pays, origine (Europe, hors d'Europe, local), niveau de confiance et état de connexion réel. 🟡 Se connecter depuis la page n'est pas fait : la page donne les lignes de configuration. |
| D117 | Une page des modèles par fournisseur, avec leurs qualités, et ceux qu'on veut utiliser, qu'on peut activer. | « une autre page qui liste tous les modèles par fournisseur […] quels sont les modèles qu'on veut utiliser, qu'on peut activer. Et j'aimais bien cette truc avec des qualités » | F1, F2 | L'écran « Modèles » groupe les modèles par fournisseur, avec nature, qualité de 1 à 10, prix indicatif, et dit lesquels sont activés chez le maître (alias, usage, accord). 🟡 Activer depuis la page n'est pas fait. |
| D118 | Les modèles de prévision et de décision figurent au catalogue. | « Rajoute les modèles de prédiction et de prévision et de décision. » | F1 | L'écran « Modèles » liste les modèles de nature Prévision et Décision de l'assistant, avec leurs fournisseurs ; un filtre par nature les isole. 🟡 Aucun agent ne peut encore les appeler. |

## 10. Notes de version

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D78 | Tenir des notes de version (release notes), de version en version. | « commence à faire des release notes aussi, sinon on va s'y perdre » | P5 | `releases.toml` a une entrée par version ; un test refuse une version sans note. |
| D79 | Les notes de version doivent apparaître dans l'interface web. | « elles devraient apparaître dans la web UI aussi » | P5 | L'interface a un écran « Nouveautés » alimenté par `/api/releases`. |
| D80 | Leur présentation doit être soignée. | « Fait quelque chose de joli. » | P5 | Subjectif : capture à l'appui. |

## 11. Plateformes Linux / macOS / Windows

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D81 | Prévoir une version Windows et une version Mac, pas seulement Linux. | « Pensez à une version Windows aussi, hein. C'est pas que du Linux, et Mac aussi. » | A2 | Le code compile et les tests passent sur Windows et macOS (intégration continue). |
| D82 | La version Windows doit avoir le même niveau de sécurité que la version Linux. | « met à jour la version Windows avec le même niveau de sécurité que sur Linux » | A2 | Chaque protection de `src/os.rs` a une écriture Windows, et un test qui passe sur Windows : fichiers secrets fermés aux autres comptes, lien non suivi, descendance d'une commande tuée avec elle, process fermé aux autres. Les écarts restants sont listés. |
| D83 | Sous Linux, l'agent doit fonctionner sur des distributions diverses. | « l'agent risque de se retrouver sur des distributions diverses et variées » | décision 21 | Voir D9. |

## 12. Qualité : tests, couverture, revues

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D84 | Lancer un agent Fable (effort élevé) pour une passe de sécurité : code, connexions, cloisonnement. | « tu lances un agent Fable, sans doute High, pour faire une passe sur la sécurité » | D11 | Les revues de sécurité sont consignées dans la spec (§7), avec constats et traitement. |
| D85 | Connaître le taux de couverture des tests (et savoir si c'est mesurable en Rust). | « Quel taux de couverture tu as? » | — | Le taux de couverture est mesuré et noté, avec ce qui le fausse. |
| D86 | Optimiser, refaire des revues de code et tester. | « optimise, refais des codes review, teste » | — | Revues et tests refaits après chaque surface nouvelle ; consignés dans le plan. |
| D87 | Définir une liste de critères d'acceptation pour chaque fonctionnalité, permettant de dire si elle est passée ou non. | « il faudrait une liste de critères d'acceptance pour chacune d'entre elles » | — | Ce document. |
| D88 | Lancer ensuite un agent indépendant qui vérifie, fonctionnalité par fonctionnalité, si elle est implémentée ou non. | « il faudra lancer un agent indépendant pour vérifier l'état d'implémentation ou non de chaque feature » | — | Le rapport de vérification indépendant existe (`docs/VERIFICATION.md`). |
| D89 | Vérifier que tout a été fait comme convenu et que rien n'a été inventé. | « vérifier qu'on a bien tout fait comme on a dit et pas inventé des choses » | — | Le rapport signale aussi ce qui a été fait sans avoir été demandé. |

## 13. Méthode de travail : spec, plan, commits, langue

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D90 | Discuter du besoin avec l'utilisateur avant de construire. | « Faudrait en discuter avant non ? !!! » | — | Méthode de travail. |
| D91 | Formaliser tout dans des documents et des specs, pour ne rien oublier. | « n'oublie pas de formaliser tout ça dans des specs pour que tu puisses suivre aussi, ne rien oublier » | — | `docs/SPEC.md` existe et est tenu à jour à chaque commit de code. |
| D92 | Tenir un planning avec des tâches, formalisé dans des documents. | « Fais-toi un planning avec des tâches. » | — | `docs/PLAN.md` existe, avec tâches cochées et journal. |
| D93 | Ce suivi est dans le dépôt, pas dans la mémoire de Claude, pour pouvoir reprendre à tout moment. | « ne mets pas ça dans ta mémoire, Claude. Mets ça dans le repo. » | — | Rien du suivi n'est hors du dépôt. |
| D94 | Les specs listent de manière exhaustive toutes les fonctionnalités convenues, en relisant toute la conversation. | « il faudrait que tu listes de manière exhaustive toutes les fonctionnalités qu'on s'est données » | — | Ce document couvre les 109 demandes. |
| D95 | Tout committer et pousser. | « n'oublie pas de tout committer évidemment » | — | L'arbre de travail est propre et la branche est poussée. |
| D96 | Consigne ponctuelle de la nuit : aller le plus loin possible, mais s'arrêter après environ une heure pour ménager les quotas. | « travaille encore une petite heure et après tu t'arrêtes » | — | Consigne de session, sans objet pour le produit. |

## 14. Idées « à étudier » (évoquées, non décidées)

| N° | Demande | Citation | SPEC | Critère d'acceptation |
|---|---|---|---|---|
| D97 | Installer l'agent en service ou en démon sur les machines, dans certains cas. *(à étudier)* | « soit un démon, soit un service sur les machines, à voir, faut étudier » | A5, Q4 | Étudié et tranché dans la spec. |
| D98 | Le premier agent installé sur une VM ou une machine serait un service, servant de relais pour lancer d'autres agents en démon. *(à étudier)* | « c'est lui qui sert de relais pour éventuellement lancer d'autres agents en démon » | A6, Q4 | Étudié et tranché dans la spec. |
| D99 | Déployer par d'autres moyens distants que local/Docker/SSH. *(à étudier)* | « En remote ou d'autres moyens de communication qui permettraient potentiellement de se connecter sur une machine distante » | G5 | Étudié et tranché dans la spec. |
| D100 | Cloisonner les commandes d'un agent par chroot ou un autre mécanisme. *(à étudier)* | « peut-être avec du chroot ou autre chose » | D5 | Étudié et tranché dans la spec ; le cloisonnement par le système est fait ou planifié. |
| D101 | Maintenir aussi, dans certains cas, un canal ouvert par le parent vers l'enfant (lien dans les deux sens). *(à étudier)* | « il y a certainement une partie des cas où il faudra maintenir un canal pour que le parent puisse discuter avec l'enfant » | G4 | Étudié et tranché dans la spec. |
| D102 | Un agent installé sur un poste de travail, qui observe le travail d'une personne, cherche à l'aider, la sollicite, discute avec elle. *(à étudier)* | « qui va écouter le travail d'une personne » | Q5 | Étudié et tranché dans la spec. |
| D103 | Un agent sur Android, ou plutôt une application (PWA) ou simplement l'interface web, pour dialoguer avec l'agent de niveau 0 depuis le téléphone. *(à étudier)* | « sera peut-être simplement la web UI qui me permettra d'interagir avec l'agent niveau 0 » | P4, Q9 | Étudié et tranché dans la spec ; l'interface est utilisable sur téléphone. |
| D104 | N'activer SQLite que sur le maître. *(à étudier)* | « qu'il soit activé que sur le maître, pourquoi pas » | décision 14 | Tranché dans la spec. |
| D105 | Savoir si les enfants sont strictement identiques à l'agent 0 ou légèrement différents. *(à étudier)* | « les enfants sont légèrement différents de l'agent 0, il faut voir » | décision 14 | Tranché dans la spec : même exécutable pour tous. |
| D106 | Prévoir du code spécifique côté serveur (agent 0) pour la gestion des fournisseurs et modèles. *(à étudier)* | « il y a peut-être du code spécifique à prévoir pour la partie serveur, il faut vérifier » | Q10 | Étudié et écrit dans la spec. |
| D107 | Donner à certains agents des capacités de voix ou d'image, en plus du LLM. *(à étudier)* | « peut-être des capacités de voix, ou d'image, ou LLM » | F4, Q10 | Étudié et écrit dans la spec. |
| D108 | Distribuer l'agent par paquets Debian plutôt qu'en binaire statique. *(à étudier)* | « à moins que tu passes par des installations Debian ou toi. Mais là pareil, il faut étudier ce que ça donne. » | décision 21 | Tranché dans la spec. |
| D109 | Héberger l'interface comme un site servi par Nginx. *(à étudier)* | « peut-être un site hébergé avec du Nginx, j'en sais rien » | décision 22 | Tranché dans la spec. |

## Points que l'utilisateur doit confirmer

Relevés à la relecture : la dictée ou la formulation laisse un doute.

1. **D20, coffre des clés** : « Vault » désigne-t-il un logiciel de coffre dédié, ou seulement le fait que les clés ne vivent qu'au niveau 0 ?
2. **D74** : tranché le 21 novembre, c'est la bibliothèque elle-même (voir D111).
3. **D60** : la notion de quota doit-elle être dans les instructions des agents, dans le code, ou les deux ? Compris : les deux.
4. **D82** : la parité de sécurité est demandée pour Windows ; compris comme valant aussi pour macOS.
5. **D69, apprentissage** : au-delà du répertoire de rôles, rien n'est défini.
6. **D5** : compris comme « l'adresse IP de chaque agent est connue du maître et affichée ».
