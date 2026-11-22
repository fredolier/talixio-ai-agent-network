# Planning

Suivi des tâches. À tenir à jour dans le même commit que le travail fait.
Les exigences citées (A1, D3…) sont celles de [SPEC.md](SPEC.md).

## Pour reprendre

1. Lire `docs/SPEC.md`, puis ce fichier.
2. `cargo test` doit passer (tests unitaires et `tests/e2e.rs`).
3. Continuer à la première tâche non cochée de l'étape en cours.

**Étape en cours : 1 — reste l'essai avec un vrai modèle. Ensuite : étape 2.**

## Étape 1 — le cœur, sur une seule machine

- [x] Droits : validation, atténuation parent → enfant, contrôle des chemins et des commandes (D1–D4)
- [x] Modèles : format neutre, Anthropic, compatible OpenAI (A3)
- [x] Lien parent-enfant : identification par jeton, battements, messages garantis (G2, G4, G8)
- [x] Veille puis extinction d'un enfant sans parent (G7)
- [x] Relais des appels de modèle jusqu'au maître, contrôle de l'alias et du budget (D6, D7)
- [x] Boucle de l'agent et outils : fichiers, commandes, enfants (G1, G9)
- [x] Missions : objectif et critère mesurable, relance, compte rendu, pause (M1–M3, G3)
- [x] Journal d'activité remonté au maître (D8)
- [x] API JSON du maître et commandes `tree`, `send`, `watch`, `stop` (P1, P3)
- [x] Test de bout en bout avec un faux serveur de modèles
- [x] Tests qui rejouent les attaques de l'audit (`tests/security.rs`)
- [x] Exécutable optimisé en taille (A4)
- [x] Traiter les constats de la première passe de sécurité (D11, SPEC §7)
- [ ] Essai réel avec un vrai modèle (aucune clé disponible dans l'environnement de développement)

## Étape 2 — sortir de la machine

- [x] Lien et API chiffrés : TLS 1.3, certificat du parent épinglé chez l'enfant, écoute distante permise (G6)
- [ ] Écoute du lien séparée de celle de l'API ; socket Unix pour les enfants locaux ; correspondant qui envoie au compte-gouttes (SPEC §7)
- [ ] Budgets : l'agent voit son budget et son reste, le répartit entre ses enfants sans dépasser ce qui lui reste, alerte son parent à un seuil, en cascade ; budget en argent (B1–B4, B6) ; quotas d'abonnement à étudier (B5, Q8)
- [x] SQLite embarqué : mémoire par agent, journal du maître sur disque (S2, S3)
- [ ] Stockage, suite : dossier de travail par agent (S1) ; arbre et conversations sur disque (S3) ; effacement à la désinstallation
- [x] Plafond de droits par cible de déploiement (Q2, décision 17)
- [x] Déploiement Docker (G5) — arguments vérifiés en test ; **pas encore essayé avec un vrai démon**
- [x] Déploiement SSH : copie de l'exécutable, lancement, désinstallation (G5, G10) — essayé de bout en bout avec un faux `ssh` (`tests/deploy.rs`) ; **pas encore sur une vraie machine**
- [x] Exécutable statique (musl), indépendant de la distribution de la cible (décision 21) — compilé et testé
- [ ] Essai réel de SSH et de Docker sur les VM ; cibles d'un autre processeur (ARM) : un exécutable par architecture, choisi d'après la cible
- [x] Notes de version : `releases.toml`, `talixio-agent releases`, `GET /api/releases` (P5)
- [ ] Répertoire des rôles chez le maître, interrogeable par toute la grappe, avec le résultat des missions (R1–R3, Q11)
- [ ] Couverture des tests : première mesure à 52 % des lignes (`cargo llvm-cov`), sous-estimée car les process maître et enfants lancés par les tests ne sont pas comptés ; à faire : les compter (arrêt propre, variable de profil transmise sous `cfg(coverage)`), puis couvrir `agent.rs`, `tools.rs`, `spawn.rs`, `main.rs`
- [x] Catalogue de fournisseurs et de modèles, embarqué, visible dans l'interface (F1)
- [x] Modèles de service appelables par les agents : transcription, voix, image, vision, plongements, prévision, décision ; quota en appels par nature ; outils ; tests avec faux fournisseurs (F2, F4, décision 25)
- [x] Revue de sécurité indépendante des modèles de service : 8 constats et une note, tous traités (SPEC §7, quatrième passe)
- [ ] Modèles de service, suite : essai chez de vrais fournisseurs ; comptage en argent (B4) ; effort et contexte par enfant (F3) ; fournisseur déclaré une seule fois dans la configuration (`[providers.<nom>]` ; depuis l'interface, c'est fait)
- [x] Se connecter à un fournisseur et activer un modèle depuis l'interface, à chaud : clé collée, ou OAuth là où il existe sans préparation (décision 26, D120)
- [ ] Connexion depuis l'interface, suite : essai chez de vrais fournisseurs ; fournisseurs dont l'adresse dépend du compte (Infomaniak) ; changer le nombre d'appels sans réactiver ; mise à jour du catalogue
- [x] Interface web embarquée : arbre en direct, détail et arrêt d'un agent, dialogue avec le maître, journal, nouveautés ; téléphone et manifeste d'application (P2, P4, décision 22) — essayée dans un navigateur, bureau et téléphone
- [ ] Interface, suite : budgets, validations, réglages ; notifications ; hors ligne
- [x] Passe de sécurité indépendante sur le déploiement et le TLS : 10 constats, 9 traités (SPEC §7, deuxième passe)
- [x] Configuration scellée des enfants, clé remise une fois par le parent (décision 23) ; rotation des journaux (décision 24)
- [x] Test du constat 1 de la deuxième passe : clés interdites aux enfants (`tests/security.rs`)
- [ ] Suites de la deuxième passe : rapatrier le journal distant ; retirer les bases des petits-enfants
- [x] Passe de sécurité indépendante sur l'interface web (SPEC §7, troisième passe)

## Étape 3 — durcir et durer

- [ ] Cloisonnement par l'OS : utilisateur dédié, Landlock/seccomp, conteneur (D5) — **prioritaire** : sans lui, une commande autorisée a tous les droits du compte
- [ ] Validations humaines des actions sensibles, jusqu'à l'interface (D9)
- [ ] Coffre des clés au niveau 0, clés sous un autre compte que celui des agents (D6, Q6)
- [ ] Conversations longues : résumé ou compaction (M4)
- [x] Le maître en service du système : `service install`, `uninstall`, `status` (A5, décision 28, D122)
- [x] Image Docker minimale du maître, configuration par défaut (décision 29, D123)
- [x] Le maître démarre sans modèle ni clé et reçoit son modèle depuis l'interface (décision 31, D124)
- [x] Abonnement Claude par la commande officielle `claude` : connecteur, pont d'outils, seconde image Docker (décision 32, D125)
- [ ] Image Docker, suite : la construire et la publier avec chaque version ; essai réel
- [ ] Service, suite : essai sur une machine qui redémarre (Linux, macOS, Windows) ; vrai service Windows
- [x] Agents permanents du maître recréés à son redémarrage, avec leur mémoire et des droits revérifiés (décision 30, D122)
- [x] Revue de sécurité indépendante du service, de l'image Docker, de la publication et de l'installation (SPEC §7, sixième passe)
- [ ] Agents permanents, suite : reprise de la conversation (M4), petits-enfants, mémoire des agents déployés ailleurs, agent hôte par machine (A6, Q4), marque dans l'interface, compteur de tokens repris
- [x] Windows : protections propres au système (Job Objects, listes de droits, liens), tests verts en intégration continue (A2)
- [ ] Windows : essai sur un poste réel ; agents Windows comme cibles de déploiement ; rejouer sous Windows les tests de déploiement (A2)
- [x] macOS : compilé et testé en intégration continue (A2)
- [ ] macOS : essai sur un poste réel (A2)
- [x] Toutes les plateformes en intégration continue (Linux, Linux statique, Windows, macOS)
- [x] Une version téléchargeable par système et une installation en une ligne (décision 27, D121)
- [ ] Versions, suite : signature des exécutables (macOS, Windows) et des empreintes

## Étape 4 — nouveaux usages

- [ ] Accès au web pour les agents (M5)
- [ ] Assistant sur poste de travail (Q5)
- [ ] Branchement de Talixio AI Assistant sur l'API (P1)

## Journal

| Date | Fait |
|---|---|
| 2026-11-20 | Étape 1 : cœur écrit, tests verts, exécutable de 3,3 Mo. Première passe de sécurité lancée. |
| 2026-11-20 | Audit rendu : 17 constats vérifiés, tous traités et rejoués en test (SPEC §7). Nouvelle syntaxe des règles de commande, authentification mutuelle du lien, budgets par réservation. Exigences de stockage et de mémoire (S1–S3, Q7), de budgets et de quotas (B1–B6, Q8) ajoutées. |
| 2026-11-20 | SQLite embarqué (exécutable : 4,6 Mo) : mémoire des agents, journal du maître repris au redémarrage. Décidé : le maître tourne sur une VM. |
| 2026-11-20 | v0.3.0 : TLS 1.3 partout, certificat épinglé. v0.4.0 : cibles de déploiement SSH et Docker avec plafond de droits, désinstallation, notes de version. Exigences F1–F4 (catalogue de fournisseurs, image, voix) inscrites. |
| 2026-11-20 | Exécutable statique (musl) compilé et testé : 5,3 Mo. Couverture mesurée. Exigences R1–R3 (répertoire des rôles) inscrites ; catalogue de l'assistant relu (Q10). |
| 2026-11-20 | v0.5.0 : interface web embarquée, servie par le maître. |
| 2026-11-20 | Deuxième passe de sécurité (déploiement, TLS) : 10 constats, 9 traités. |
| 2026-11-20 | Compilation croisée Windows réussie ; écarts de sécurité Windows et macOS inscrits (A2). |
| 2026-11-21 | v0.6.0 : configuration scellée des enfants ; rotation des journaux. |
| 2026-11-21 | `EXIGENCES.md` : les 109 demandes relevées dans la conversation, avec critères d'acceptation. Intégration continue : Linux, macOS et musl verts ; sous Windows, les protections système passent, deux tests écrits avec des chemins POSIX corrigés. |
| 2026-11-21 | v0.6.1 : intégration continue verte sur Linux, macOS, Windows et musl ; Windows au niveau de Linux pour les protections système (A2, restes notés). Vérification indépendante des 109 demandes lancée. |
| 2026-11-21 | Vérification indépendante rendue (`VERIFICATION.md`) : 69 tenues, 21 partielles, 9 non faites, 5 non vérifiables sur place, 5 sans objet. Défaut trouvé et corrigé (v0.6.2) : premiers événements d'un petit-enfant perdus au journal du maître ; test à trois niveaux ajouté (`tests/tree.rs`). Fichiers de couverture committés par erreur retirés. |
| 2026-11-21 | v0.7.0 : interface refaite avec shadcn/ui (React, thème propre), embarquée construite ; écrans Commandes, Modèles, Rôles ; `GET /api/models`, `GET /api/roles`, répertoire des rôles tenu par le maître (R1 en partie). Exigences D110 à D115 inscrites. Mesure de « sloppiness » : outils tiers non exécutés (refus du garde-fou de session), passe manuelle sur un catalogue public. À faire : revue de sécurité indépendante de l'interface et des deux routes. |
| 2026-11-21 | v0.7.1 : revue de sécurité indépendante de l'interface et des deux routes rendue (SPEC §7, troisième passe) : un constat haut, trois moyens, cinq bas, tous traités et rejoués en test. Demandé ensuite : une table des fournisseurs (européen ou non, connecté ou non) et une page des modèles par fournisseur avec leurs qualités, à la manière de l'assistant. |
| 2026-11-21 | v0.8.0 : catalogue des fournisseurs et des modèles repris de l'assistant, embarqué ; écrans « Fournisseurs » (table : pays, Europe, confiance, connecté) et « Modèles » (par fournisseur, qualité, prix, activé ou non). D116, D117 inscrites. À décider : se connecter et activer depuis la page (saisie de clé, droits du maître modifiés à chaud). |
| 2026-11-21 | v0.8.1 : modèles de prévision et de décision ajoutés au catalogue (10 modèles ; fournisseurs TypeSafe AI et Nixtla), listés pour comparer. |
| 2026-11-21 | v0.9.0 : les agents appellent les modèles de service (transcription, voix, image, vision, plongements, prévision, décision) par le maître, comptés en appels par nature (`max_calls`). Sept outils, un message du lien (`service_request`), quatre familles d'API. Tests : `tests/services.rs` (faux fournisseur), scénario de prévision par un vrai enfant dans `tests/e2e.rs`. Revue de sécurité indépendante lancée, rapport attendu. D119 inscrite. Pas d'essai réel, faute de clés. |
| 2026-11-21 | v0.10.0 : se connecter à un fournisseur et activer ses modèles depuis l'interface, à chaud (décision 26, D120) : `src/connect.rs`, six routes, table des modèles et droits du maître modifiables pendant qu'il tourne, OAuth PKCE pour OpenRouter, clé collée pour les autres, `catalog_file`. Tests : `tests/connect.rs` (vrai maître, vrai enfant, faux fournisseur). Défaut trouvé par le test et corrigé avant livraison : après redémarrage, un modèle déconnecté restait dans les droits du maître. Revue de sécurité indépendante rendue : deux constats moyens (quota de la configuration écrasable, « Désactiver » pouvant retirer un modèle de la configuration), quatre bas, tous traités sauf le `state` OAuth, noté en limite (SPEC §7, cinquième passe). Pas d'essai réel, faute de clés. |
| 2026-11-21 | v0.11.0 : le maître en service du système (`service install`, décision 28, D122) : systemd, launchd, tâche planifiée Windows ; jamais root, aucun secret dans la définition. Image Docker minimale du maître (`FROM scratch`, 6,5 Mo, compte sans droit ; décision 29, D123), essayée hors Docker dans une racine vide. Publication d'une version aussi à la main (la pose d'étiquette est refusée depuis la session de développement). |
| 2026-11-21 | Agents permanents (décision 30) : `spawn_agent` avec `permanent`, demande gardée par le maître et rejouée à son démarrage ; test `tests/permanent.rs` (maître tué et relancé quatre fois). Revue de sécurité indépendante du service, de l'image et de la publication : deux constats moyens (ligne `User=` avalée par un chemin finissant par `\\`, jeton d'exemple accepté), six bas ; traités, restes notés (SPEC §7, sixième passe). |
| 2026-11-21 | Versions téléchargeables : publication par étiquette (cinq exécutables, empreintes), scripts d'installation en une ligne pour Linux, macOS et Windows (décision 27, D121). |
| 2026-11-21 | v0.9.1 : revue de sécurité indépendante des modèles de service rendue : un constat haut (quota contournable par des appels échoués après envoi), un moyen (redirections suivies), six bas ; tous traités et rejoués en test (SPEC §7, quatrième passe). |
| 2026-11-21 | CI Windows rouge sur les tests du service : ils employaient des chemins d'Unix, qui ne sont pas absolus sous Windows. Tests d'Unix réservés à Unix ; un test propre à Windows (tâche planifiée, chemins refusés) ajouté. Aucun changement du code livré. |
| 2026-11-21 | v0.12.0 : le maître démarre sans modèle ni clé et reçoit son modèle depuis l'interface (décision 31, D124) : `model` et `[models]` facultatifs, route `/api/models/{alias}/default`, bouton « Donner au maître », image Docker sans clé au lancement. Défaut trouvé par le test : `[models]` restait obligatoire. Revue de sécurité indépendante : deux constats moyens (conversation déplacée sans geste après la perte du modèle, fournisseur local donné d'office), quatre bas ; tous traités (SPEC §7, septième passe). Non essayé : Docker, vrai fournisseur. |
| 2026-11-21 | Écran d'entrée : dit aussi où est le jeton quand le maître est lancé avec `TALIXIO_API_TOKEN` (Docker), cas où `.talixio/api-token` n'existe pas. |
| 2026-11-21 | OVHcloud sans clé (`keyless` au catalogue, « Connecter sans clé ») ; sans clé, plus d'en-tête d'autorisation factice. **Premier essai chez un vrai fournisseur** : maître lancé sans modèle ni clé, OVHcloud connecté sans clé par l'API, `gpt-oss-120b` activé, réponse reçue. Parcours de l'assistant relu : seul Claude s'y connecte sans clé, par la commande `claude` (abonnement) ; à décider (Q11). |
| 2026-11-21 | Q11 tranchée : pas de connexion par abonnement ; clés d'API et OVHcloud sans clé, aucune dépendance, un seul exécutable. |
| 2026-11-21 | v0.13.0 : l'abonnement Claude par la commande officielle `claude` (décision 32, D125, Q11 rouverte et tranchée) : connecteur `claude-code` (`src/model/cli.rs`), pont d'outils (`mcp-bridge`, `POST /bridge`), clé de session liée au lien authentifié, fournisseur « Claude (abonnement) », seconde image Docker. Essai réel avec la vraie commande : maître, reprise de session, refus de droits, enfant. Test : `tests/claude.rs` (fausse commande, `examples/faux_claude.rs`). Revue de sécurité indépendante lancée, rapport attendu : ses constats suivront. Non essayé : construction de l'image, jeton `setup-token` d'un abonnement Max. |
| 2026-11-21 | v0.13.1 : revue de sécurité indépendante du connecteur `claude` rendue : aucun constat haut démontré, six moyens, six bas ; onze traités, un noté en limite (SPEC §7, huitième passe). Principal : le maître contrôle maintenant à l'exécution que `claude` n'a et n'appelle que les outils de l'agent, et tue le tour sinon. Options revérifiées avec la vraie commande (annonce : aucun outil intégré). |
| 2026-11-21 | `tools.sh` : construire et lancer le maître dans Docker en une commande (`up`, `up claude`, `token`, `logs`, `status`, `stop`, `purge`). Mêmes options de cloisonnement que la documentation ; jeton de l'interface tiré une fois, gardé hors du dépôt (600) ; jeton d'abonnement jamais écrit ; secrets passés par leur nom, jamais sur la ligne de commande. Essayé avec une fausse commande `docker` (pas de Docker dans la session) : à essayer en vrai. |
| 2026-11-21 | Image `claude` : plus de jeton à passer au lancement. `claude` se connecte lui-même dans le conteneur (`./tools.sh login`, soit `docker exec -it talixio claude auth login`) et garde sa connexion dans le volume (`CLAUDE_CONFIG_DIR=/data/.claude`, interdit d'office aux agents). `CLAUDE_CODE_OAUTH_TOKEN` reste possible. Non essayé (ni Docker ni abonnement dans la session). |
| 2026-11-21 | v0.13.2, après le premier essai de l'utilisateur dans Docker avec son abonnement (le maître a répondu) : le dialogue montre que le maître travaille, met en forme le Markdown de ses réponses (rendu en éléments React, aucun HTML du modèle interprété, aucun lien cliquable) et affiche ses erreurs ; « Connecter » vérifie `claude auth status` ; « sans clé » retiré pour l'abonnement. Vérifié dans la vraie interface avec la vraie commande. |
| 2026-11-21 | v0.13.3 : un parent ne conclut pas pendant que ses enfants travaillent (G12, D126) : `finish_mission` refusé avec le nom des enfants au travail, tant qu'il ne les a pas arrêtés lui-même ; consignes des agents complétées ; scénario ajouté à `tests/tree.rs`. |
| 2026-11-22 | v0.14.0 : tâches planifiées (décision 33, D128 : `src/schedule.rs`, cinq routes, page « Tâches »), outils `make_dir` et `list_provider_models`, `catalog show` / `catalog check`, exemple `examples/catalogue` (décision 34, D127 : collecteur haiku, rédacteur sonnet, vérificateur opus, script `lire-web` borné par une liste de sites), `TALIXIO_TOOLS` dans `tools.sh`. |
| 2026-11-22 | Passe réelle de l'exemple « catalogue » avec l'abonnement Claude : maître en sonnet, trois enfants créés l'un après l'autre avec leur modèle (haiku, sonnet, opus, visibles dans la grappe), 11 relevés, `catalog.toml` rendu valide par `catalog check` (1 ajout, 1 retrait), aucune objection du vérificateur. Deux `spawn_agent` refusés d'abord (droits plus larges que ceux du maître, budget trop haut), corrigés par le maître seul. Le collecteur a pris 2,8 M de tokens sur 5 M : les grilles officielles des fournisseurs ne se lisent pas sans navigateur. |
| 2026-11-22 | v0.14.1 : revue de sécurité indépendante de la v0.14.0 rendue (SPEC §7, neuvième passe) : aucun constat haut, trois moyens, neuf bas ou d'information ; douze traités, deux notés en limite. |
| 2026-11-22 | v0.15.0 : un navigateur pour les agents (décision 35, D129), demandé après la première passe où aucune grille de prix officielle n'avait pu être lue : script `lire-page` (Chromium par Playwright, mêmes bornes que `lire-web`), image `docker/Dockerfile.navigateur`, `./tools.sh up navigateur`, filtre `docker/chromium-seccomp.json`. **Docker essayé pour de vrai, pour la première fois** : images `claude` et `navigateur` construites, `tools.sh run`, `token`, `status` ; `lire-page` lit les grilles d'Anthropic et de Mistral dans le conteneur fermé (aucune capacité, pas de nouveaux privilèges), bac à sable de Chromium actif. Deux défauts trouvés par l'essai : `apk` hors du PATH de l'image, et le Chromium d'Alpine arrêté par son propre bac à sable (appel `pwritev2`) : image passée sur Debian avec le Chromium de Playwright. CI Windows de la v0.14.1 rouge (fins de ligne du catalogue dans un test) : corrigée. |
| 2026-11-22 | Navigateur retiré de l'image du maître, à la demande (« au lieu de polluer celui du maître ») : image `talixio-navigateur` pour les agents seuls, cible `docker` avec `memory` et `seccomp`, `examples/catalogue/cible-navigateur.toml`. **Première vraie exécution d'une cible Docker** (jusque-là essayée avec de faux `docker`) : maître sur la machine, agent « lecteur » (haiku) créé dans un conteneur en lecture seule sans capacité, lien établi, grille de prix d'Anthropic lue par `lire-page` (bac à sable de Chromium actif), fichier écrit dans le montage, compte rendu reçu, conteneur retiré. Défaut d'usage trouvé : le refus d'une règle sans `args` parlait des droits du parent, pas du plafond de la cible ; message corrigé. Reste (Q12) : un maître lui-même dans Docker ne lance pas encore de conteneur. |
| 2026-11-22 | Revue de sécurité indépendante de `lire-page` (SPEC §7, dixième passe), sur banc d'essai : deux constats hauts (redirections et tâches de fond hors contrôle), corrigés par un mandataire interne au script, seule sortie de Chromium ; contre-vérification faite, un reste fermé ensuite (le mandataire n'ouvre que les noms approuvés). Deuxième passe réelle de l'équipe « catalogue », avec le navigateur (version d'avant ces corrections) : 15 relevés ; prix lus chez Anthropic, Google, OVHcloud, DeepSeek, ElevenLabs, Infomaniak ; catalogue valide, 2 ajouts, 1 retrait, 8 modifications ; le vérificateur a refusé la première version (3 objections fondées), le rédacteur a corrigé ; le budget de 6 M de tokens s'est épuisé pendant la seconde vérification. Deux défauts de la consigne trouvés et corrigés : le plafond d'un cinquième bloquait une trentaine d'ajouts confirmés (il ne vaut plus que pour retraits et modifications) ; budget de l'exemple porté à 12 M. Scaleway refuse le navigateur piloté (403). |
| 2026-11-22 | v0.16.0 : un maître en conteneur lance ses agents dans des conteneurs voisins (décision 36, Q12 tranchée, D130) : `TALIXIO_DOCKER=1 ./tools.sh up claude` (socket de la machine, réseau à part), cible `docker` avec `network`, `agent_in_image` et montages `volume`/`subpath`, image `talixio-navigateur` avec l'agent et les scripts. Essayé pour de vrai : maître en conteneur, agent voisin en lecture seule et sans capacité, page lue avec Chromium, relevé écrit dans le sous-dossier du volume, compte rendu reçu, `docker` et le socket refusés aux outils, `stop` retire tout. Modèle scripté pour cet essai (OVHcloud sans clé répondait 429) ; images d'essai dérivées (Docker Hub répondait 429) : les Dockerfile du dépôt à l'identique restent à essayer. |
| 2026-11-22 | v0.16.1 : revue de sécurité indépendante des conteneurs voisins rendue (SPEC §7, onzième passe), sur un vrai Docker : aucun constat haut, deux moyens (volume entier atteignable par `subpath` absent, « . » ou lien ; groupe du socket hérité par tout ce que le maître lance), quatre bas. Cinq corrigés, deux notés en limite (risque élargi et écrit ; version de l'agent dans l'image). |
| 2026-11-22 | v0.17.0 : les paquets (décision 37, D131), demandés après le premier lancement du maître en Docker chez l'utilisateur : `talixio-agent pack list/show/install/remove/verify` (`src/pack.rs`), paquet « catalogue » livré dans l'exécutable (`packs/catalogue` : scripts, trois rôles, consigne ; l'ancien `examples/catalogue/outils`), `packs_dir` chez le maître avec vérification SHA-256 au démarrage et refus d'un dossier inscriptible, empreinte par commande (`sha256` dans la règle, revérifiée à chaque lancement), rôles installés dans `spawn_agent`, `./tools.sh pack`. Test `tests/packs.rs`. Essayé pour de vrai dans Docker (modèle scripté) : paquet installé par `tools.sh`, agent créé par son rôle dans un conteneur voisin, commande vérifiée puis lancée, maître qui refuse de redémarrer sur un fichier modifié. Non fait : TUI, service de téléchargement, signature. |
| 2026-11-22 | v0.17.1 : revue de sécurité indépendante des paquets rendue (SPEC §7, douzième passe) : un constat haut (garde du dossier contournable par `chmod` ou par un dossier parent), quatre moyens, huit bas. Douze corrigés et rejoués (tests, essai réel dans Docker avec montage en lecture seule), un noté en limite (fichiers voisins d'un script). |
| 2026-11-22 | v0.17.2 : identifiant de paquet (SHA-256 de son contenu), que la configuration du maître peut fixer (`[packs]`), à la demande (« qu'on ne contourne pas par le nom »). Testé dans `tests/packs.rs`. À faire, convenu : compte dédié et fichiers à root posés par `service install`, rôles installés dans la page « Rôles », service de téléchargement signé, TUI. |
| 2026-11-22 | v0.18.0 : le compte dédié (décision 38, D132), demandé (« un utilisateur talixio-agent et tous les fichiers à root ») : `sudo talixio-agent service install` crée le compte, pose l'exécutable et les paquets dans `/opt/talixio`, la configuration dans `/etc/talixio` (à root), l'état dans `/var/lib/talixio` (au compte) ; commande `check` ; `pack install` sans `--dir` en root. Essayé dans un conteneur Debian (sans systemd : l'unité est écrite mais pas démarrée). Revue de sécurité lancée. Suite, dans l'ordre convenu : rôles installés dans la page « Rôles », service de téléchargement signé, TUI. |
| 2026-11-22 | v0.19.0 : les rôles installés dans la page « Rôles » (décision 39, D133) : `/api/roles` rend `installed`, la page les montre avec paquet, version, identifiant, commandes et consignes ; le journal note le paquet à la création d'un agent. Suite : service de téléchargement signé, puis TUI. |
| 2026-11-22 | v0.19.1 : revue de sécurité indépendante du compte dédié rendue (SPEC §7, treizième passe) : un constat haut (la vérification sous le compte héritait de l'environnement, du terminal et des fichiers ouverts de root), trois moyens, cinq bas ; neuf traités et rejoués en conteneur, deux notés. Reste : essai avec un vrai systemd. |
| 2026-11-22 | v0.20.0 : dépôt de paquets signé (décision 40, D131.6) : `src/depot.rs`, `pack keygen`, `pack publish`, `pack list/show/install --depot https://… --key`. Fichiers statiques, index signé Ed25519, hébergeur non cru. Testé en dossier (unitaires et ligne de commande) ; pas encore avec un vrai dépôt https. Q13 ouverte : clé et adresse du dépôt officiel. Revue de sécurité indépendante rendue avant publication (SPEC §7, quatorzième passe) : un constat haut (repli silencieux vers le non signé), trois moyens, sept bas ; onze corrigés, un noté. Suite : TUI. |
| 2026-11-22 | v0.21.0 : l'écran des paquets, `pack tui` (décision 41, D131.5) : `src/tui.rs`, `./tools.sh pack tui`. Essayé dans un terminal (tmux). Les quatre étapes convenues (compte dédié, rôles dans l'interface, dépôt signé, TUI) sont faites. Restent : essai avec un vrai systemd, un vrai dépôt https, Q13 (clé et adresse du dépôt officiel). |
