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
- [ ] Catalogue de fournisseurs, modèles par nature (texte, image, voix), effort et contexte par enfant (F1–F4, Q10)
- [x] Interface web embarquée : arbre en direct, détail et arrêt d'un agent, dialogue avec le maître, journal, nouveautés ; téléphone et manifeste d'application (P2, P4, décision 22) — essayée dans un navigateur, bureau et téléphone
- [ ] Interface, suite : budgets, validations, réglages ; notifications ; hors ligne
- [x] Passe de sécurité indépendante sur le déploiement et le TLS : 10 constats, 9 traités (SPEC §7, deuxième passe)
- [ ] Suites de la deuxième passe : borner les journaux locaux des enfants ; test du constat 1 (clés interdites aux enfants) ; rapatrier le journal distant ; retirer les bases des petits-enfants
- [ ] Passe de sécurité indépendante sur l'interface web

## Étape 3 — durcir et durer

- [ ] Cloisonnement par l'OS : utilisateur dédié, Landlock/seccomp, conteneur (D5) — **prioritaire** : sans lui, une commande autorisée a tous les droits du compte
- [ ] Validations humaines des actions sensibles, jusqu'à l'interface (D9)
- [ ] Coffre des clés au niveau 0, clés sous un autre compte que celui des agents (D6, Q6)
- [ ] Conversations longues : résumé ou compaction (M4)
- [ ] Agents permanents ; installation en service ou démon, agent hôte par machine (A5, A6, Q4)
- [ ] Windows : le code se compile ; restent les protections propres au système (Job Objects pour tuer la descendance, ACL des fichiers de clés, liens), un essai réel, et des agents Windows comme cibles de déploiement (A2)
- [ ] macOS : compiler et essayer sur une vraie machine ; remplacer PDEATHSIG (A2)
- [ ] Compilation de toutes les plateformes en intégration continue (Linux statique, Windows, macOS)

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
