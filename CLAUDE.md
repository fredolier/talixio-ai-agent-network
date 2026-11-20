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
- Langue : documentation, messages et commentaires en français.
- Ne pas ouvrir de pull request sans demande explicite.
