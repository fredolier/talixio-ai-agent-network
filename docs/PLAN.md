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
- [ ] Agents permanents ; installation en service ou démon, agent hôte par machine (A5, A6, Q4)
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
| 2026-11-21 | v0.10.0 : se connecter à un fournisseur et activer ses modèles depuis l'interface, à chaud (décision 26, D120) : `src/connect.rs`, six routes, table des modèles et droits du maître modifiables pendant qu'il tourne, OAuth PKCE pour OpenRouter, clé collée pour les autres, `catalog_file`. Tests : `tests/connect.rs` (vrai maître, vrai enfant, faux fournisseur). Défaut trouvé par le test et corrigé avant livraison : après redémarrage, un modèle déconnecté restait dans les droits du maître. Revue de sécurité indépendante : SPEC §7, cinquième passe. Pas d'essai réel, faute de clés. |
| 2026-11-21 | Versions téléchargeables : publication par étiquette (cinq exécutables, empreintes), scripts d'installation en une ligne pour Linux, macOS et Windows (décision 27, D121). |
| 2026-11-21 | v0.9.1 : revue de sécurité indépendante des modèles de service rendue : un constat haut (quota contournable par des appels échoués après envoi), un moyen (redirections suivies), six bas ; tous traités et rejoués en test (SPEC §7, quatrième passe). |
