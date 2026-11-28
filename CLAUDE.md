# Talixio Agent Network — notes de travail

Agents IA autonomes en grappe, en Rust. Un seul exécutable (`talixio-agent`).

- **Avant tout travail** : lire `docs/SPEC.md` (exigences, décisions, questions ouvertes), puis
  `docs/PLAN.md` (tâches, étape en cours).
- **Après tout travail** : mettre à jour l'état des exigences dans `SPEC.md`, cocher les tâches et
  compléter le journal dans `PLAN.md`, dans le même commit que le code.
- `cargo test` avant de pousser : tests unitaires et `tests/e2e.rs` (maître réel, enfant réel,
  faux serveur de modèles).
- La sécurité prime : tout est refusé par défaut, un enfant n'a jamais plus de droits que son
  parent, chaque appel d'outil est revérifié. Toute nouvelle surface (outil, message du lien,
  route de l'API, mode de déploiement) passe par une revue de sécurité.
- Langue : le dépôt public est en anglais (`README.md`, `docs/DESIGN.md`, `CONTRIBUTING.md`,
  `SECURITY.md`, nouveaux commentaires et nouveaux messages). L'interface web est écrite en anglais
  avec sa traduction française (`web/src/locales/fr/`, vérifiée par `npm run build`). Le code Rust
  existant garde ses commentaires et messages en français tant qu'il n'est pas traduit : traduire
  ce que l'on touche. `SPEC.md`, `PLAN.md`, `EXIGENCES.md` et `VERIFICATION.md` restent en français.
- Deux branches : le travail se fait sur la branche de travail ; `main` est la branche publique.
  `main` ne reçoit **jamais** `docs/SPEC.md`, `docs/PLAN.md`, `docs/EXIGENCES.md`,
  `docs/VERIFICATION.md` ni `CLAUDE.md` (propos du propriétaire, journal interne) : leur condensé
  public est `docs/DESIGN.md`, à tenir à jour avec le code. Publier : `scripts/publier-main.sh`
  (dans la branche de travail seulement), qui pose sur `main` un commit du même arbre moins ces
  fichiers.
- Toute question au propriétaire passe par un formulaire HTML (une page privée publiée, où il
  répond et dont les réponses sont relues), jamais par une question posée dans le texte d'une
  réponse : il y répond, et la réponse est relue avant
  d'agir. Les choix proposés disent ce que chacun entraîne ; la recommandation vient en premier.
  Le formulaire existe déjà : la page privée « Questions Talixio Agent Network ». Pour une nouvelle
  série de questions, la republier avec un nouvel identifiant de formulaire ; les réponses se lisent
  dans sa collection `answers/<identifiant>`.
- Ne pas ouvrir de pull request sans demande explicite.
