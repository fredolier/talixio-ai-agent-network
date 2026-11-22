# Exemple : une équipe qui tient à jour le catalogue des modèles

Trois agents, aux droits opposés, mettent à jour la liste des fournisseurs et des modèles
(`catalog.toml`). Le résultat est une proposition, dans le dossier de travail : c'est vous qui la
copiez dans le dépôt.

| Agent | Modèle | Ce qu'il peut | Ce qu'il ne peut pas |
|---|---|---|---|
| collecteur | haiku | lire le web, par `lire-web` et `lire-page`, sur les seuls sites de `outils/sites.txt` ; écrire ses relevés | toucher au catalogue |
| rédacteur | sonnet | lire les relevés, écrire le catalogue, lancer `catalog check` | lire le web |
| vérificateur | opus | lire, lancer `catalog check` | écrire, lire le web |

Le maître coordonne. Il relève lui-même ce que les fournisseurs connectés annoncent
(`list_provider_models`, avec sa clé, qui ne sort pas) et ne conclut que si le vérificateur n'a
plus d'objection.

Rien n'est programmé pour cet exemple : tout est dans ce dossier.

- `talixio.toml` : les droits du maître, dont les commandes permises.
- `outils/lire-web` : le seul accès au web. Une adresse https, un site de la liste, pas de
  redirection suivie, taille et durée bornées. `outils/sites.txt` : la liste, à compléter.
- `outils/lire-page` : la même lecture, avec un vrai navigateur (Chromium piloté par Playwright),
  pour les pages qui se construisent par leurs scripts : les grilles de prix des fournisseurs.
  La page et ses cadres restent sur les sites de la liste ; ce qu'elle charge pour se construire
  doit venir d'un nom public, en https ; ni image, ni téléchargement ; profil jeté à chaque
  lecture ; 60 s au plus. Les lignes de tableau sont rendues sur une ligne.
- `consigne.md` : la consigne à remettre au maître, à coller dans une tâche planifiée.

## Le lancer dans Docker

```bash
TALIXIO_CONFIG=examples/catalogue/talixio.toml TALIXIO_TOOLS=examples/catalogue/outils ./tools.sh up navigateur
```

```bash
./tools.sh login
```

Dans l'interface : « Fournisseurs » (connectez Claude par abonnement, et ceux dont vous avez une
clé : leurs listes de modèles font foi), « Modèles » (activez `opus`, `sonnet` et `haiku`), puis
« Tâches » : créez une tâche hebdomadaire avec le texte de `consigne.md`, et « Lancer » pour une
première passe. La page « Grappe » montre chaque agent, son modèle et ses droits.

Le résultat est dans le volume du conteneur :

```bash
docker cp talixio:/data/work/catalogue ./catalogue-propose
```

```bash
talixio-agent catalog check ./catalogue-propose/catalog.toml
```

Relisez `CHANGEMENTS.md`, en particulier « À valider par une personne » (la note de qualité des
nouveaux modèles), puis copiez `catalog.toml` à la racine du dépôt.

## Ce qu'il faut savoir

- Une page lue sur le web peut chercher à manipuler l'agent qui la lit. Le collecteur n'a aucun
  droit sur le catalogue, et rien n'entre dans le dépôt sans votre copie.
- Un navigateur est une grosse surface : il exécute les scripts des pages qu'il lit. Il garde
  son bac à sable (l'image se lance avec `docker/chromium-seccomp.json`, le filtre ordinaire de
  Docker plus ce que ce bac à sable demande), il tourne sous le compte sans droit du conteneur,
  et seul le collecteur peut le lancer. Mais il tourne dans le même conteneur que le maître :
  n'ajoutez à la liste que des sites que vous tenez pour sérieux.
- Certains sites refusent les navigateurs pilotés (réponse 403) : le relevé le dit, et le
  modèle reste « non confirmé ».
- Hors de Docker : `lire-page` demande Node, `playwright-core` et un Chromium ; s'ils ne sont pas
  aux endroits ordinaires, donnez-les dans `outils/navigateur.conf` (`modules = …`,
  `chromium = …`).
- `lire-web` borne les sites, pas ce qu'ils répondent : ne mettez dans la liste que des sites
  publics, jamais une adresse de votre réseau.
- Une passe complète appelle les trois modèles plusieurs dizaines de fois : comptez-la dans les
  limites de votre abonnement. Une fois par semaine suffit largement.
