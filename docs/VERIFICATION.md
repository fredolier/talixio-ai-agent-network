# Vérification indépendante des exigences

- **Date** : 2026-11-21
- **Commit vérifié** : `c024a69` (v0.6.1). La branche a avancé deux fois pendant la
  vérification. La lecture du code et les deux essais ont porté sur `83a638b` ; `347098c` ne
  change que deux lignes de tests (`src/policy.rs`, `tests/e2e.rs`) ; `c024a69` ne change que le
  numéro de version, `releases.toml`, `SPEC.md` (A2) et `PLAN.md`. `cargo test` a été rejoué sur
  les trois ; `clippy`, musl et la compilation Windows sur `347098c`, dont le code est celui de
  `c024a69`.
- **Référence** : les 109 demandes de [`EXIGENCES.md`](EXIGENCES.md).
- **Règle suivie** : seuls le code, les tests et ce qui a été exécuté ici font preuve. `SPEC.md`
  et `PLAN.md` n'ont servi que pour les critères du type « tranché dans la spec ».

## Ce qui a été exécuté

Environnement : Linux x86_64. Pas de `ssh`, pas de démon Docker, ni Windows ni macOS.
L'intégration continue GitHub n'a pas été consultée.

| Commande | Résultat |
|---|---|
| `cargo test` (à `83a638b`, `347098c`, puis `c024a69`) | 33 tests, 0 échec : 27 unitaires, `deploy` 1, `e2e` 1, `security` 4 |
| `cargo clippy --all-targets -- -D warnings` | aucun avertissement |
| `cargo build --release` | réussi ; 5 291 928 octets, lié dynamiquement |
| `cargo build --release --target x86_64-unknown-linux-musl` | réussi ; 5 444 288 octets ; `file` : « static-pie linked » ; `ldd` : « statically linked » |
| `cargo test --target x86_64-unknown-linux-musl` | 33 tests, 0 échec |
| `cargo check --target x86_64-pc-windows-gnu --all-targets` | compile (code et tests) ; rien n'est exécuté sous Windows |
| `cargo llvm-cov --summary-only` (sur une copie) | 53,9 % des lignes ; `main.rs` 0 %, `agent.rs` 9 %, `tools.rs` 24 %, `spawn.rs` 31 % (les process maître et enfants lancés par les tests ne sont pas comptés) |
| `talixio-agent --help`, `--version`, `releases --last` (exécutable statique) | fonctionnent ; version 0.6.0 au moment de l'essai (0.6.1 depuis `c024a69`) |
| `talixio-agent child` avec une enveloppe invalide | refuse de démarrer (code 1) |
| `docker info`, `ssh -V` | pas de démon Docker, pas de `ssh` : aucun essai réel possible |

Deux tests ont été écrits pour cette vérification, sur une copie du dépôt (rien n'a été ajouté
au dépôt) :

- **Trois niveaux** (maître → « chef » → « lecteur », vrais process, faux serveur de modèles) :
  création d'un petit-enfant, question au parent, compte rendu en cascade, relance d'un agent en
  pause, coupure du lien (process figé par `SIGSTOP` puis repris) avec un message envoyé pendant
  la coupure, arrêt du petit-enfant par l'API. Tout passe, à une exception près : voir D29.
- **Sans droits** : un agent sans `[capabilities]` se voit refuser lecture, écriture, commande et
  création d'enfant ; `spawn_agent` refuse une mission sans objectif ou sans critère.

Dans les tableaux, « essai » désigne ces deux tests.

## Synthèse

| Verdict | Nombre |
|---|---|
| Tenu | 69 |
| Partiel | 21 |
| Non fait | 9 |
| Non vérifiable ici | 5 |
| Sans objet | 5 |
| **Total** | **109** |

Niveau de preuve des déploiements, demandé à part (D31) :

| Mode | Preuve |
|---|---|
| Local | Test réel : vrais process maître et enfants (`tests/e2e.rs`, essai à trois niveaux). |
| SSH | Programme factice : un faux `ssh` exécute sur place la commande distante (`tests/deploy.rs`) ; l'enfant lancé est réel. Aucun essai avec un vrai `ssh` ni une vraie machine. |
| Docker | Lecture du code, plus un test unitaire de la liste d'arguments (`docker_args`). `spawn_docker` et `docker rm` ne sont jamais exécutés, même avec un programme factice. |
| Windows | Compilation seule, ici. Exécution : non vérifiable. |
| macOS | Rien : ni compilé ni exécuté ici. |

## 1. Agent et exécutable

| N° | Verdict | Preuve |
|---|---|---|
| D1 | Tenu | `cargo build --release` produit `talixio-agent` ; aucun autre langage à l'exécution (l'interface est du HTML/JS statique embarqué). |
| D2 | Non vérifiable ici | Dépend de l'intégration continue. Prouvé ici : tests verts sous Linux (glibc et musl), compilation Windows. À savoir : `tests/deploy.rs` et un test de `security.rs` sont `cfg(unix)`, donc vides sous Windows ; le `README.md` dit encore « Testé sous Linux seulement ». |
| D3 | Tenu | `RootConfig.instructions`, `Capabilities.exec` ; refus hors liste testé (`tools_stay_inside_the_rights`). |
| D4 | Tenu | `e2e.rs` vérifie `name` et `role` dans `/api/tree`. Réserve : `spawn_agent` exige l'argument `role` mais accepte un rôle vide (essai). |
| D5 | Tenu | `NodeInfo.host`, `os`, `ips` ; ligne « Adresses » de l'interface (`web/app.js`). Une seule adresse, celle de sortie, déclarée par l'agent lui-même (`node.rs:188-200`). |
| D6 | Tenu | `root --config` ne lit qu'un TOML ; l'enfant reçoit tout sur son entrée standard (`spawn.rs:31-54`). |
| D7 | Tenu | `Cargo.toml` : `opt-level = "z"`, LTO, `strip` ; 5,3 Mo mesurés ; taille notée dans la spec (décision 21). |
| D8 | Tenu | `include_str!` dans `api.rs:51-55` ; `e2e.rs` vérifie que les cinq fichiers sont servis. |
| D9 | Tenu | Compilation musl réussie ici, exécutable statique, 33 tests verts sur cette cible. x86_64 seulement. |
| D10 | Sans objet | Orientation. Le prompt système relevé pendant l'essai décrit rôle, droits, interlocuteurs et mission (`agent.rs:213-311`). |

## 2. Droits et sécurité

| N° | Verdict | Preuve |
|---|---|---|
| D11 | Tenu | Essai « sans droits » : tout est refusé. Le dépôt n'a pas de test qui exerce les outils d'un agent sans aucun droit. |
| D12 | Tenu | `tools.rs:37-175` ; refus hors droits testés (`security.rs`), refus sans droit vérifiés par l'essai. |
| D13 | Tenu | Outils relevés pendant l'essai : le « chef » a `spawn_agent`, `send_to_agent`, `stop_agent`, `list_agents` ; le « lecteur » ne les a pas. |
| D14 | Tenu | `tools.rs:80` (outil non proposé) et `tools.rs:390` (appel refusé), vérifiés par l'essai. |
| D15 | Tenu | Droits du maître lus dans `[capabilities]` ; il accorde ceux des enfants par `spawn_agent`. |
| D16 | Tenu | Essai à trois niveaux. Bornes : `policy.rs:182-188` (test `child_cannot_exceed_parent`), `tools.rs:502`. Le dépôt n'a aucun test de bout en bout avec un petit-enfant. |
| D17 | Tenu | Tout passe par les arguments de `spawn_agent`. Les cibles SSH et Docker, elles, sont écrites à la main dans la configuration du maître. |
| D18 | Tenu | Essai : la question du « lecteur » arrive dans la conversation du « chef », qui répond. Le critère annonce un test de bout en bout : le dépôt n'en a pas pour `ask_parent`. |
| D19 | Tenu | `tools.rs:403-406` : droits absents = aucun. Seul le modèle est repris du parent par défaut. |
| D20 | Partiel | Clés chez le maître seul (`node.rs:65`, `Providers`), environnement nettoyé (vérifié dans `e2e.rs`), fichiers de clés interdits aux enfants (`keys_stay_out_of_reach_of_children`), appels relayés sur trois niveaux (essai). Pas de coffre : la clé est dans un fichier en 0600 ou une variable d'environnement. |
| D21 | Partiel | Deux passes consignées (SPEC §7), constats rejoués dans `tests/security.rs`. Pas de revue de l'interface web (la spec le dit), ni de la configuration scellée et de `os.rs` (v0.6.0). Que les revues aient été faites par un agent indépendant n'est pas vérifiable. |
| D22 | Tenu | TLS 1.3 seul (`tls.rs`). Tests : connexion en clair refusée au lien (`security.rs`) et à l'API (`e2e.rs`), mauvais certificat refusé. |
| D23 | Tenu | `config.rs:369-432` ; tests `une_configuration_scellee…` et remise unique de la clé (`security.rs`). Restent en clair dans l'enveloppe, par nécessité : identifiant, adresse et certificat du parent, secret du lien. |
| D24 | Tenu | Test unitaire d'altération sur `Envelope::open`. Aucun outil ne modifie les droits. Pas de test au niveau du process ; l'agent applique lui-même ses droits, en mémoire. |
| D25 | Partiel | `check_grant` et la revérification à chaque appel sont testés. Mais aucun cloisonnement par le système : une commande autorisée a tous les droits du compte (`policy.rs:3-5`, SPEC D5 non fait). Le « béton » demandé n'y est pas. |

## 3. Grappe, lien et déploiement

| N° | Verdict | Preuve |
|---|---|---|
| D26 | Tenu | Essai : un enfant doté de `spawn` crée et dirige son propre enfant. |
| D27 | Tenu | Un seul lien montant (`node.rs:63`) ; outils limités au parent et aux enfants. Non garanti par le système : une commande autorisée pourrait parler à qui elle veut. |
| D28 | Tenu | `parent_id` unique ; arbre renvoyé par `/api/tree`. |
| D29 | Partiel | L'activité des descendants remonte (`e2e.rs`, essai). **Défaut constaté** : les premiers événements d'un petit-enfant sont perdus. Le maître écarte tout événement dont l'auteur n'est pas encore dans l'arbre qu'il connaît (`link.rs:606-617`), et l'arbre lui parvient avec 100 ms de retard (`link.rs:362-363`). Pendant l'essai, trois événements du « lecteur » manquaient au journal du maître : réception de la mission, appel `ask_parent`, question posée. |
| D30 | Tenu | `tools.rs:409-416` accepte `local` et toute cible `ssh` ou `docker` de la configuration. |
| D31 | Partiel | Voir le tableau des niveaux de preuve : SSH avec un programme factice, Docker jamais exécuté. Seul le maître déploie ; même système et même processeur exigés (`spawn.rs:144-148`). |
| D32 | Tenu | Essai : lien coupé 5 s, message envoyé pendant la coupure, reçu après la reconnexion. Le dépôt ne teste que la file (`outbox_keeps_unacknowledged_frames`), pas la reconnexion. La file est bornée à 5000 messages. |
| D33 | Tenu | `e2e.rs` : maître tué, l'enfant survit d'abord puis s'éteint (partie `cfg(unix)`). |
| D34 | Tenu | L'enfant ouvre la connexion (`link.rs:149`) ; le parent n'appelle jamais l'enfant. |
| D35 | Tenu | Écoute TLS ; `listen` et `advertise` (`config.rs:26-29`), utilisés dans `deploy.rs`. |
| D36 | Tenu | `stop_agent` exercé dans `deploy.rs` ; `POST /api/agents/{id}/stop` vérifié par l'essai sur un petit-enfant (aucun test du dépôt ne l'appelle). |
| D37 | Partiel | SSH : exécutable, base et fichier de process retirés, process tué (`deploy.rs`, faux `ssh`). Docker : `docker rm --force` jamais exécuté. Enfant local : sa base et son journal restent sur disque (constaté). Les bases des petits-enfants nés sur une cible restent aussi. Il n'y a pas de désinstallation distincte de l'arrêt. |
| D38 | Tenu | `listen` réglable, commandes à distance (`--url`, `--fingerprint`) ; décision 15 de la spec. |
| D39 | Tenu | Essai : les comptes rendus arrivent dans la conversation du parent, qui agit en retour. |

## 4. Missions et autonomie

| N° | Verdict | Preuve |
|---|---|---|
| D40 | Tenu | Boucle `agent.rs:123-166` et relance `agent.rs:98-120`. Bornes : 60 appels par tour, 3 relances, puis compte rendu « impossible » d'office. |
| D41 | Tenu | `ask_parent` disponible pendant la mission (essai). |
| D42 | Partiel | Un agent en mission crée des enfants sur sa machine seulement : `tools.rs:417-420` réserve le déploiement ailleurs au maître. La demande dit « soit sur le poste, soit sur un autre poste ». |
| D43 | Tenu | Phrase du prompt (`agent.rs:273-274`). Critère faible : rien d'autre ne pousse l'agent à chercher une autre voie. |
| D44 | Tenu | `tools.rs:397-401`, vérifié par l'essai. Le caractère « mesurable » n'est pas contrôlable : un texte non vide suffit. |
| D45 | Tenu | `finish_mission` (`tools.rs:299-311`) : `achieved` ou `impossible`, compte rendu, pause (`e2e.rs`). |
| D46 | Tenu | Essai : `send_to_agent` relance un enfant en pause, qui rend compte de nouveau. |
| D47 | Tenu | Cas d'usage : D16, D19 et D44 passent. |
| D48 | Non vérifiable ici | Aucun essai SSH réel. Non démontré. |
| D49 | Tenu | Cas d'usage : D39 passe. |
| D50 | Partiel | « Optimiser un serveur » : possible dans la limite des commandes autorisées. « Promouvoir une marque » : aucun accès au web. |

## 5. Modèles, fournisseurs, capacités

| N° | Verdict | Preuve |
|---|---|---|
| D51 | Tenu | `model/anthropic.rs`, `model/openai.rs`, tests unitaires des deux formats. Seul le format OpenAI est exercé de bout en bout ; aucun essai avec un vrai modèle. |
| D52 | Non fait | Une liste plate d'alias (`[models.<alias>]`) ; ni catalogue de fournisseurs, ni nature de modèle. |
| D53 | Tenu | `capabilities.models` ; refus testé (« modèle non autorisé », `security.rs`). Modèles de texte seulement ; un enfant ne reçoit qu'un seul modèle (`tools.rs:408`). |
| D54 | Partiel | Par enfant : modèle et instructions. L'effort est attaché à l'alias, pas à l'enfant. Aucun réglage de taille de contexte. |
| D55 | Partiel | Les droits et le modèle diffèrent d'un agent à l'autre. Aucune capacité d'image ni de voix. |

## 6. Budgets et quotas

| N° | Verdict | Preuve |
|---|---|---|
| D56 | Tenu | Comptage par le parent (`node.rs:429-453`), `tokens_used` dans `/api/tree` (`e2e.rs`). |
| D57 | Partiel | `max_tokens` appliqué par le parent, appels simultanés compris (`a_budget_cannot_be_overrun_by_parallel_calls`). Aucun budget en argent. |
| D58 | Partiel | `policy.rs:176-181` : le budget d'un enfant est borné par le plafond du parent, pas par ce qui lui reste, et la somme des budgets distribués n'est pas bornée. La dépense réelle reste plafonnée par le compteur de chaque ascendant. |
| D59 | Non fait | Aucune lecture de quotas. La spec (Q8, B5) laisse la question ouverte : elle n'est pas tranchée. |
| D60 | Non fait | Les prompts relevés pendant l'essai ne disent rien du budget ni du reste (`agent.rs:213-311`). Aucun outil ne les donne à l'agent. |
| D61 | Partiel | Aucun seuil, aucune alerte préalable. Budget épuisé : l'appel échoue, l'agent envoie une erreur à son parent et se met en pause (`agent.rs:62-68`). |
| D62 | Partiel | La consommation remonte : pendant l'essai, le « chef » (135) inclut le « lecteur » (60). Aucune alerte ne remonte jusqu'au maître. Le maître affiche sa seule consommation (45), pas celle de la grappe (`node.rs:345`). |

## 7. Stockage, mémoire, cache

| N° | Verdict | Preuve |
|---|---|---|
| D63 | Partiel | Tranché pour la mémoire et le journal (décision 14). Aucun dossier de travail par agent (SPEC S1 ouvert). |
| D64 | Tenu | `memory_write`, `memory_read` ; notes relues dans le prompt au démarrage (`agent.rs:295-309`) ; persistance testée (`store.rs`). En pratique, seul le maître retrouve ses notes : un enfant recréé a un nouvel identifiant, donc une base vide. |
| D65 | Tenu | `rusqlite` avec `bundled` ; une base par agent (`node.rs:243`, `node.rs:288`) ; surpoids noté (décision 14). |
| D66 | Tenu | Essai : un seul prompt système par agent sur toute sa vie ; messages ajoutés en fin de fil ; `cache_control` dans la requête Anthropic (`model/anthropic.rs:67`). Effet sur le cache non mesuré (aucun vrai modèle). |
| D67 | Non fait | Aucun code. |
| D68 | Non fait | Aucun outil de recherche de rôle. |
| D69 | Non fait | Aucun code. |

## 8. Journaux

| N° | Verdict | Preuve |
|---|---|---|
| D70 | Tenu | `rotate_log` et son test (`spawn.rs:233-244`). Voir les écarts : la rotation est faite par le parent, sauf par SSH. |
| D71 | Tenu | `log_max_bytes` du maître, recopié dans chaque configuration d'enfant (`tools.rs:474`, `node.rs:318`). Lecture du code ; aucun test. |

## 9. Pilotage : API, interface web, téléphone

| N° | Verdict | Preuve |
|---|---|---|
| D72 | Tenu | `web/app.js` : arbre et états rafraîchis toutes les 1,5 s. Lecture du code et capture du dépôt ; aucun navigateur ici. |
| D73 | Tenu | Page servie par le maître (`e2e.rs`) ; deux captures dans `docs/images/`. |
| D74 | Partiel | Aucune bibliothèque shadcn/ui : CSS écrit à la main, à son allure (`web/app.css`). |
| D75 | Tenu | API décrite dans SPEC §5 ; `e2e.rs` couvre `health`, `tree`, `activity`, `messages`, `releases`. Ni `/api/events` ni l'arrêt par l'API ne sont testés dans le dépôt. |
| D76 | Non vérifiable ici | Aucune trace de la maquette dans le dépôt. |
| D77 | Sans objet | Subjectif. Le critère demande des captures bureau et téléphone, clair et sombre : le dépôt n'a que deux captures de bureau, en clair. |
| D78 | Tenu | `releases.toml` : sept versions, jusqu'à la 0.6.1 ; test `les_notes_sont_lisibles_et_a_jour`. |
| D79 | Tenu | Écran « Nouveautés » (`web/app.js`), `/api/releases` vérifié dans `e2e.rs`. |
| D80 | Sans objet | Subjectif. Aucune capture de cet écran dans le dépôt. |

## 10. Plateformes Linux / macOS / Windows

| N° | Verdict | Preuve |
|---|---|---|
| D81 | Non vérifiable ici | Windows : compile ici. macOS : rien. Les tests sur ces systèmes dépendent de l'intégration continue. |
| D82 | Partiel | Chaque fonction de `os.rs` a son écriture Windows, et trois tests couvrent fichiers secrets, liens et descendance. Mais `harden_process` n'a aucun test, sur aucun système ; le test du lien symbolique rend la main sans rien vérifier si le compte Windows ne peut pas créer de lien (`os.rs:508`) ; `tests/deploy.rs` et le test d'outils sont `cfg(unix)`. La liste des restes existe depuis `c024a69` (SPEC A2) mais omet les deux premiers points. Exécution sous Windows non vérifiable ici : la spec l'affirme « verte » en intégration continue, ce qui ne fait pas preuve. |
| D83 | Tenu | Voir D9. |

## 11. Qualité : tests, couverture, revues

| N° | Verdict | Preuve |
|---|---|---|
| D84 | Tenu | Deux passes consignées dans SPEC §7, constats et traitement. L'identité de l'auditeur n'est pas vérifiable. |
| D85 | Tenu | 52 % noté dans `PLAN.md`, avec ce qui le fausse ; 53,9 % remesuré ici. |
| D86 | Partiel | Revues après le cœur et après le déploiement. Aucune après l'interface web ni après la v0.6.0. |
| D87 | Tenu | `EXIGENCES.md` : un critère par demande. Plusieurs sont plus faibles que la demande (voir plus bas). |
| D88 | Tenu | Ce document. |
| D89 | Tenu | Section « Ce qui n'a pas été demandé ». |

## 12. Méthode de travail

| N° | Verdict | Preuve |
|---|---|---|
| D90 | Sans objet | Méthode. |
| D91 | Partiel | `SPEC.md` existe, mais n'est pas à jour : la décision 10 (React) contredit la décision 22 ; l'architecture ignore la commande `releases` et le déploiement ; les messages `unseal` et `key` manquent au tableau du lien. A2 (Windows) contredisait `os.rs` jusqu'à `c024a69`, qui l'a corrigé pendant cette vérification. Les commits `b02f0f8` et `347098c` touchent `src/` sans toucher la spec. |
| D92 | Tenu | `PLAN.md` : tâches cochées et journal. Les cases Windows et intégration continue, en retard sur le code, ont été mises à jour par `c024a69`. |
| D93 | Tenu | Aucun fichier de mémoire trouvé hors du dépôt sur cette machine. |
| D94 | Non vérifiable ici | Les 43 messages d'origine ne sont pas dans le dépôt : l'exhaustivité ne peut pas être contrôlée. Le critère est circulaire. |
| D95 | Tenu | Avant l'écriture de ce fichier : arbre propre, branche alignée sur `origin`. Deux fichiers `.profraw` de mesure de couverture sont committés par erreur à la racine. |
| D96 | Sans objet | Consigne de session. |

## 13. Idées « à étudier »

| N° | Verdict | Preuve |
|---|---|---|
| D97 | Non fait | SPEC A5 et Q4 : « à étudier à l'étape 3 ». Ni étudié ni tranché. |
| D98 | Non fait | Idem (A6, Q4). |
| D99 | Non fait | G5 cite « un autre moyen d'accès distant » sans étude ni décision. |
| D100 | Partiel | Pistes listées (D5 : compte dédié, Landlock/seccomp, conteneur, chroot) et planifiées en étape 3. Aucun choix arrêté, rien de fait. |
| D101 | Tenu | Décision 2 : une seule connexion, ouverte par l'enfant, sert dans les deux sens. |
| D102 | Partiel | Q5 : une ligne (« visible et accepté par elle ; étape à part »). Reporté, pas tranché. |
| D103 | Tenu | Q9 tranché (interface web installable) ; mise en page téléphone (`web/app.css:110`) et manifeste. Non essayé sur un téléphone. |
| D104 | Tenu | Décision 14 : SQLite pour tous les agents. |
| D105 | Tenu | Décisions 1 et 14 : même exécutable. |
| D106 | Tenu | Q10. |
| D107 | Tenu | Q10 et F4 : étudié et écrit. Rien n'est codé. |
| D108 | Tenu | Décision 21 : pas de paquets Debian. |
| D109 | Tenu | Décision 22 : fichiers embarqués, servis par le maître. Nginx n'y est pas nommé. |

## Écarts entre critère et demande

Critères plus faibles ou différents de la phrase d'origine :

- **D20** : la demande parle d'un coffre (« Vault »). Le critère se contente de « les clés ne
  vivent que chez le maître » et renvoie le coffre à plus tard.
- **D25** : la demande veut un point « béton ». Le critère admet lui-même la limite : sans
  cloisonnement par le système, les droits sont tenus par le process qu'ils sont censés tenir.
- **D37** : « se désinstaller complètement » devient « arrêter un agent déployé retire trois
  fichiers ». Rien pour un enfant local, et pas d'ordre de désinstallation distinct de l'arrêt.
- **D42** : le critère dit « en local ou sur une cible » sans préciser que seul le maître déploie
  sur une cible.
- **D43** : une phrase dans le prompt suffit au critère.
- **D53, D55** : le critère parle de « tel modèle de tel fournisseur » et de capacités « texte,
  image, voix » ; le code ne connaît que des alias de modèles de texte.
- **D58** : le critère demande une somme bornée par le reste du parent ; le code ne borne que
  chaque enfant, par le plafond du parent.
- **D61, D62** : la demande dit « prévient son parent et s'arrête » à un seuil, en cascade. Le
  code n'agit qu'une fois le budget épuisé, et le message s'arrête au parent direct.
- **D70** : la demande dit « directement par l'agent ». Pour un enfant local ou en conteneur,
  c'est son parent qui fait tourner son journal ; seul un agent déployé par SSH tourne le sien,
  et ce chemin n'est pas testé. Le journal de sortie du maître lui-même n'est pas borné.
- **D74** : la demande est d'utiliser shadcn. Le critère prévoit déjà que ce n'est pas le cas.
- **D2, D81, D82** : tout repose sur une intégration continue « verte » que rien, dans le dépôt,
  ne permet de constater. Le commit `347098c` corrige d'ailleurs des tests pour Windows, et
  `c024a69` déclare l'intégration continue verte : ce n'est pas vérifiable ici.
- **D97 à D109** : « tranché dans la spec » est tenu pour huit idées, mais trois ne sont que
  citées (D97, D98, D99) et deux reportées (D100, D102).
- **Critères annonçant un test qui n'existe pas** dans le dépôt : D11 (agent sans droits), D18
  (`ask_parent` de bout en bout), D16 (petit-enfant), D32 (reconnexion), D36 (arrêt par l'API).
  Les comportements sont justes : ils ont été vérifiés par les essais de cette vérification.

## Ce qui n'a pas été demandé

Conséquences nécessaires d'une demande (à garder) :

| Élément | Où | Rattaché à |
|---|---|---|
| Authentification mutuelle du lien par défi-réponse, jeton d'API, écran de connexion | `link.rs`, `api.rs`, `web/` | D21, D22 |
| Balise à marqueur secret autour de chaque message (contre l'injection) | `agent.rs:189-211` | D21 |
| Langage des règles de commande (`*`, `**`, `{read}`, `{write}`, `cwd`) | `policy.rs` | D3, D11 |
| Chemins interdits d'office : dossier d'état, fichiers de clés, `~/.ssh` | `config.rs:288-300` | D20, D21 |
| Plafond de droits par cible de déploiement | `config.rs:71-74`, `tools.rs:421-444` | D25, D30 |
| Durcissement du process et des commandes (groupes de process, mémoire fermée) | `os.rs` | D21, D82 |
| Options de fermeture du conteneur Docker | `spawn.rs:177-196` | D21, D30 |
| Commande cachée `child`, messages `unseal` et `key` | `main.rs`, `protocol.rs` | D6, D23 |
| Journal d'activité sur disque (SQLite, 200 000 événements) | `store.rs`, `node.rs:204-237` | D29 |
| Intégration continue, avec `clippy` | `.github/workflows/ci.yml` | D2, D81 |
| Manifeste d'application et mise en page téléphone | `web/` | D103 |

Ajouts sans demande correspondante :

| Élément | Où | Remarque |
|---|---|---|
| Commandes `tree`, `send`, `watch`, `stop` | `main.rs` | Pilotage en ligne de commande ; seule l'interface web et l'API étaient demandées. |
| Commande `releases`, rendu en couleurs | `main.rs`, `releases.rs` | Les notes étaient demandées dans l'interface web (D79), pas au terminal. |
| `GET /api/events` (flux continu) | `api.rs:165-184` | Ni utilisé par l'interface, ni testé. |
| `GET /api/health`, sans jeton | `api.rs:56` | Utilisé par les tests. |
| Outil `list_agents` | `tools.rs:145` | Mineur. |
| Relais vers un autre modèle quand Claude décline (`refusal_fallback`), activé par défaut | `model/anthropic.rs` | Change le modèle qui répond, sans que ce soit demandé. |
| Nouvelles tentatives sur les erreurs des fournisseurs | `model/mod.rs` | Technique. |
| Certificat fourni par l'exploitant (`[tls]`), option `--public-cert` | `config.rs`, `main.rs` | Utile aux navigateurs. |
| « Rester connecté sur cet appareil » : jeton gardé dans le navigateur | `web/app.js` | Choix de sécurité non demandé. |
| Suppression des journaux d'enfants inchangés depuis sept jours | `spawn.rs:255-257` | Efface des fichiers sans réglage ni demande. |
| Seul le maître déploie hors de sa machine | `tools.rs:417-420` | Restriction non demandée, contraire à D42. |
| Limites fixes : 60 appels par tour, 3 relances, commandes de 120 s, lecture de 256 Kio, 200 notes, 20 comptes rendus, 16 appels simultanés | `agent.rs`, `tools.rs`, `store.rs`, `node.rs` | Non réglables. |
| Compte rendu « impossible » envoyé d'office après trois relances | `agent.rs:113-118` | L'agent n'a rien conclu lui-même. |
| Filtre du journal, thème sombre automatique | `web/` | Confort. |
| Fichiers `.profraw` à la racine | dépôt | Résidus à retirer. |

Rien de plus n'a été trouvé : pas d'outil, de route ou de mode de déploiement caché.

## Ce qui manque le plus

1. **Le cloisonnement par le système** (D25, D100). Tant qu'il manque, les droits d'un agent
   sont une promesse du process, et une commande autorisée a tous les droits du compte.
2. **Les budgets** (D57 à D62) : l'agent ignore son budget, rien ne l'alerte avant
   l'épuisement, rien ne remonte en cascade, et il n'y a pas de budget en argent.
3. **Un essai réel** : aucun vrai modèle, aucune vraie machine SSH, Docker jamais lancé
   (D31, D48). Le cas d'usage central, l'audit d'une VM, n'est pas démontré.
4. **Le défaut du journal** (D29) : les premiers événements d'un petit-enfant sont perdus.
5. **Le répertoire des rôles et l'apprentissage** (D67 à D69), et **le catalogue de
   fournisseurs** avec image et voix (D52, D55) : rien n'est codé.
6. **Le coffre des clés** (D20) et **la revue de sécurité de l'interface web** (D21).
7. **Le déploiement par un enfant sur une autre machine** (D42).
8. **La spec à remettre à jour** (D91), et les tests du dépôt à compléter pour les cinq
   comportements que cette vérification a dû essayer elle-même.

## Suites données (ajout de l'auteur du code, après le rapport)

Ce qui précède est le rapport du vérificateur, laissé tel quel. Ce qui suit n'en fait pas partie.

- **D29, défaut du journal** : corrigé en v0.6.2. Les événements d'un descendant encore inconnu
  sont gardés (64 au plus par enfant) jusqu'à l'arbre qui l'annonce. Le test du vérificateur est
  entré au dépôt (`tests/tree.rs`) ; il échoue sans le correctif et passe avec.
- **D2, D81** : l'intégration continue est verte sur `ubuntu-latest`, `macos-latest`,
  `windows-latest` et musl (passage 44878356494, commit `347098c`), test de bout en bout compris
  sous Windows.
- **D16, D18, D32, D36** : le test que leur critère annonçait existe maintenant (`tests/tree.rs`).
- Fichiers `.profraw` retirés ; `README.md` corrigé.
- Tout le reste (partiel, non fait, ajouts non demandés) est à arbitrer : rien n'a été changé.
