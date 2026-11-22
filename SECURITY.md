# Sécurité

## Signaler une faille

**Ne l'écrivez pas dans un ticket public.** Utilisez le signalement privé de GitHub : onglet
« Security » du dépôt, puis « Report a vulnerability »
(<https://github.com/fredolier/talixio-ai-agent-network/security/advisories/new>).

Dites ce que vous avez observé, sur quelle version (`talixio-agent --version`), et comment le
reproduire. Vous recevrez une réponse, puis un correctif ou une explication ; votre nom figurera
dans les notes de version si vous le souhaitez.

## Versions suivies

Seule la dernière version publiée reçoit des correctifs. Le projet est jeune et avance vite :
mettez à jour avant de signaler.

## Ce qui compte comme une faille

Le projet promet ceci, et tout ce qui le contredit nous intéresse :

- un agent qui lit, écrit ou lance quelque chose hors de ses droits ;
- un enfant qui obtient plus de droits que son parent ;
- une clé de modèle ou le jeton de l'API qui sort de chez le maître (journal, interface, enfant) ;
- le maître qui installe ou modifie lui-même ses commandes, ses rôles ou sa configuration ;
- un paquet ou un dépôt de paquets accepté sans la vérification annoncée ;
- l'API ou l'interface utilisable sans le jeton.

## Limites connues

Elles sont écrites, et ne sont donc pas des découvertes : voir [docs/SPEC.md](docs/SPEC.md), §7,
« Limites connues ». La principale : entre agents d'un même compte, le cloisonnement est fait par
le programme et pas encore par le système d'exploitation. Une commande que vous autorisez tourne
avec tous les droits du compte ; n'autorisez que des commandes écrites au plus serré, et faites
tourner le maître sous un compte dédié (`sudo talixio-agent service install`) ou dans un conteneur.

Le même paragraphe de la spécification rend compte de chaque revue de sécurité : constats,
gravité, traitement.
