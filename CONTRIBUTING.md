# Contribuer

Merci de votre intérêt. Le projet est jeune : les retours d'usage comptent autant que le code.

## Signaler un problème, proposer une idée

Ouvrez un ticket sur GitHub, avec :

- la version (`talixio-agent --version`) et le système ;
- ce que vous avez fait, ce que vous attendiez, ce qui s'est passé ;
- le journal du maître autour du problème, **sans jeton ni clé**.

Une faille de sécurité ne se signale pas dans un ticket public : voir [SECURITY.md](SECURITY.md).

## Proposer un changement

1. Lisez [docs/SPEC.md](docs/SPEC.md) (exigences, décisions, questions ouvertes), puis
   [docs/PLAN.md](docs/PLAN.md). Pour un changement de fond, ouvrez d'abord un ticket : une
   décision déjà prise et motivée dans la spécification ne se défait pas en passant.
2. Écrivez le changement avec ses tests.
3. Mettez à jour la documentation **dans le même changement** : l'état des exigences dans
   `SPEC.md`, le journal de `PLAN.md`, `releases.toml` si le comportement change pour
   l'utilisateur, le `README.md` si la façon de s'en servir change.
4. Vérifiez :

   ```bash
   cargo test
   ```

   ```bash
   cargo clippy --all-targets
   ```

   L'intégration continue rejoue le tout sous Linux, macOS et Windows, et construit l'exécutable
   statique. Si vous touchez à l'interface (`web/`), reconstruisez-la (`npm ci && npm run build`
   dans `web/`) : `web/dist` fait partie du dépôt et doit correspondre aux sources.

## Règles du projet

- **La sécurité prime.** Tout est refusé par défaut ; un enfant n'a jamais plus de droits que son
  parent ; chaque appel d'outil est revérifié. Toute nouvelle surface (outil, message du lien,
  route de l'API, mode de déploiement, commande) passe par une revue de sécurité, dont le compte
  rendu va dans `SPEC.md`, §7.
- **Un seul exécutable**, sans dépendance obligatoire au système.
- **Dire ce qui n'a pas été essayé.** Une fonction écrite et testée avec des programmes factices
  n'est pas « essayée » : la documentation fait la différence.
- **Langue** : documentation, messages et commentaires en français. Les noms dans le code sont
  en anglais.
- **Jamais de secret** dans le dépôt, dans une configuration d'exemple ni dans un journal.

## Licence des contributions

Le projet est sous licence [AGPL-3.0-only](LICENSE). En proposant un changement, vous acceptez
qu'il soit publié sous cette licence, et vous affirmez avoir le droit de le proposer.
