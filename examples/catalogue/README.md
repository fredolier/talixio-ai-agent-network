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
- `cible-navigateur.toml` : la cible Docker où tourne le collecteur quand il a le navigateur.
- `consigne.md` : la consigne à remettre au maître, à coller dans une tâche planifiée.

## Le lancer dans Docker

```bash
TALIXIO_CONFIG=examples/catalogue/talixio.toml TALIXIO_TOOLS=examples/catalogue/outils ./tools.sh up claude
```

(Ainsi, sans navigateur : le collecteur lit le web avec `lire-web` seul. Avec navigateur : voir plus bas.)

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

## Avec un navigateur : le collecteur dans son conteneur

Les grilles de prix officielles ne se lisent qu'avec un navigateur. Il n'est pas ajouté à l'image
du maître : c'est le collecteur qui est lancé dans un conteneur à part, fait pour ça, que le
maître crée et retire lui-même.

```bash
./tools.sh build navigateur
```

```bash
TALIXIO_DOCKER=1 TALIXIO_CONFIG=examples/catalogue/talixio.toml TALIXIO_TOOLS=examples/catalogue/outils ./tools.sh up claude
```

`TALIXIO_DOCKER=1` donne au maître le socket Docker de la machine, et le place avec ses agents dans
un réseau à part (`talixio-net`). **Qui prend le maître commande alors Docker, donc la machine** :
c'est le prix de cette commodité, à ne payer que sur une machine d'essai ou dédiée. Cela vaut
pour tout ce que le maître lance sous son compte : ne lui donnez aucune commande à arguments
libres. Dans la configuration, `network` et `volume` portent le nom du maître (`talixio-net`,
`talixio-data` ; avec `TALIXIO_NAME=x` : `x-net`, `x-data`). Aucun agent
ne reçoit le socket, et il est interdit d'office à leurs outils.

L'image `talixio-navigateur` porte l'agent, Chromium et les scripts de `outils/` tels qu'ils
étaient à la construction : reconstruisez-la après avoir changé `sites.txt` ou mis le maître à jour.

Un maître installé sur la machine (`talixio-agent service install`) fait la même chose sans
socket partagé : ajoutez `cible-navigateur.toml` à sa configuration, avec vos chemins.

## Ce qu'il faut savoir

- Une page lue sur le web peut chercher à manipuler l'agent qui la lit. Le collecteur n'a aucun
  droit sur le catalogue, et rien n'entre dans le dépôt sans votre copie.
- Un navigateur est une grosse surface : il exécute les scripts des pages qu'il lit. Il ne
  tourne donc jamais à côté du maître : seulement dans le conteneur du collecteur, jetable, en
  lecture seule, sans capacité, qui ne voit que ses scripts et ses relevés, et où Chromium garde
  son bac à sable. Ce conteneur garde le réseau : n'ajoutez à la liste que des sites publics.
- Certains sites refusent les navigateurs pilotés (réponse 403) : le relevé le dit, et le
  modèle reste « non confirmé ».
- `lire-web` borne les sites, pas ce qu'ils répondent : ne mettez dans la liste que des sites
  publics, jamais une adresse de votre réseau.
- Une passe complète appelle les trois modèles plusieurs dizaines de fois : comptez-la dans les
  limites de votre abonnement. Une fois par semaine suffit largement.
