# Changelog

Les nouveautés de chaque version de Ronnie. La section d'une version sert aussi de notes à sa release GitHub.

## [0.3.0] - 2026-09-26

### Nouveautés

- **Gestionnaire de fichiers façon FileZilla** pour chaque serveur SSH (bouton 📁 Fichiers dans l'en-tête, ⌘ E, ou clic droit sur l'hôte > Fichiers (SFTP)) : cet ordinateur à gauche, le serveur à droite. Transferts par double-clic ou glisser-déposer (y compris sur un dossier), dossiers entiers, file d'attente avec progression, vitesse et annulation, remplacer / ignorer les fichiers existants. Renommer, supprimer, nouveau dossier, **permissions** (grille rwx, valeur octale, récursif). Utilise la configuration ssh habituelle (clés, agent, rebond, mots de passe enregistrés) ; un mot de passe ou une nouvelle empreinte d'hôte sont demandés dans une fenêtre. Les dossiers sont mémorisés par serveur.
- Une ligne de transfert par fichier ou dossier sélectionné, chacune avec sa progression et son annulation.
- **⌘ + / ⌘ − / ⌘ 0 agrandissent toute l'interface** (barre latérale, paramètres, fichiers, terminal), de 60 à 200 %, mémorisé ; la taille du texte du terminal reste réglable à part.

## [0.2.1] - 2026-09-25

Version de fiabilité et de sécurité, issue d'un audit complet.

### Sécurité

- **Liens** : sous Windows, un lien piégé (`…&commande`) pouvait lancer une commande au clic. Les liens s'ouvrent désormais sans passer par un interpréteur de commandes. Seuls les liens web s'ouvrent directement ; les autres (`file://`, schémas d'applications, chemins) demandent confirmation et montrent leur vraie cible.
- **Mots de passe SSH** : ils ne sont plus lisibles par un autre programme via Ronnie. Seule la fenêtre de Ronnie les donne, et seulement aux connexions ssh qu'elle a lancées, une fois par connexion. Le mot de passe d'un hôte n'est plus envoyé à son hôte de rebond.
- Mises à jour : téléchargement refusé sans empreinte SHA-256, délais maximums.
- Fichiers de configuration et historiques lisibles par toi seul ; hôtes SSH commençant par « - » refusés.
- Coller plusieurs lignes dans un programme qui les exécuterait immédiatement demande confirmation. Réglage pour interdire aux programmes de modifier le presse-papiers.
- CI : actions épinglées, permissions minimales.

### Fiabilité

- Une config illisible ne peut plus faire perdre d'historiques ni la session ; une session illisible est mise de côté au lieu d'être écrasée.
- Les éléments qu'une version ne sait pas lire sont mis de côté au lieu de rendre toute la config inutilisable.
- Écritures sûres (synchronisées sur le disque), copie de secours quotidienne `config.json.bak`, et « Réinitialiser » garde une copie de secours.
- Après une mise à jour, la nouvelle fenêtre ne démarre plus en lecture seule.
- Linux : les mots de passe du trousseau survivent au redémarrage.
- Windows : le mot de passe SSH enregistré fonctionne à chaque connexion (plus seulement la première).
- Fermer un onglet ne peut plus faire agir une fenêtre ouverte sur l'onglet voisin.
- Les commandes tapées dans Ronnie rejoignent aussi l'historique habituel du shell.
- Journal `ronnie.log`, rapports de plantage, `ronnie --version`.

### Performances

- **0 % de CPU au repos**, même avec un `npm run dev` en cours : l'app ne se réveille plus que quand il se passe quelque chose.
- La recherche dans le texte et la détection des serveurs locaux ne ralentissent plus pendant qu'un programme écrit beaucoup.

### Nouveautés

- **Taille du texte** : ⌘ + / ⌘ − / ⌘ 0, mémorisée, aussi réglable dans les paramètres.
- **Longueur du défilement** réglable.

## [0.2.0] - 2026-09-25

### Nouveautés

- **Historique propre à chaque terminal** : chaque panneau garde ses commandes (↑), même après fermeture. En rouvrant un profil, chaque panneau retrouve les siennes. Un nouveau panneau démarre avec l'historique habituel du shell.
- **Recherche dans un terminal** (⌘ F, ou la loupe de l'en-tête) : dans tout le texte affiché, défilement compris, avec les occurrences surlignées et un compteur. Un second mode (⌘ R) cherche parmi les commandes tapées dans le panneau : Entrée l'écrit au prompt, ⌘ ↩ la lance.
- **Commandes enregistrées** (⚡ dans l'en-tête, ou clic droit > Commandes) : générales ou propres à un profil ou un hôte SSH. Un clic écrit la commande au prompt sans la lancer.
- **Badge « live »** : un halo vert autour de l'onglet tant qu'un programme tourne dedans.
- **Serveurs locaux cliquables** : les adresses `localhost`, `127.0.0.1` et `0.0.0.0` affichées par un programme (Vite, API…) deviennent des boutons dans l'en-tête du panneau.
- **Groupes de profils locaux**, comme pour les hôtes SSH (glisser-déposer, « Déplacer vers »).
- **Modifier un profil** (clic droit) : nom, couleur, dossier de chaque panneau et commandes enregistrées.
- **Raccourcis personnalisables** dans Paramètres > Raccourcis, avec détection des doublons.
- **⌘ K** vide le terminal ; **⌘ + flèches** passe au panneau voisin (sinon début / fin de ligne) ; **⌘ ⌫** efface la ligne.
- **Barre de menus macOS** : Paramètres, Recharger Ronnie, Quitter, menu Fenêtre.
- **Paramètres** : nouvel onglet Informations (version, mises à jour, liens GitHub) en premier, taille fixe quel que soit l'onglet, profils et hôtes SSH présentés par groupe, bouton **Réinitialiser** (avec confirmation).
- **Thème « Ronnie »** : noir profond, rouge sang et or. Le logo suit la couleur du thème.

### Améliorations

- Une seule instance de Ronnie écrit la configuration : une seconde fenêtre passe en lecture seule et le signale.
- La configuration garde les réglages qu'une autre version ne connaît pas, au lieu de les effacer.
- Les builds de développement utilisent leur propre configuration et affichent un badge DEV.
- Les symboles des raccourcis (⌘ ⇧ ⌥ ⌫ ↩) s'affichent correctement et sont espacés.
- Splash screen un peu plus lent, fenêtre « Fermer quand même ? » plus claire.

### Corrections

- Les commandes enregistrées pouvaient disparaître quand une ancienne version tournait en même temps.
- Des profils rangés dans d'anciens groupes n'apparaissaient plus dans la barre latérale.

## [0.1.0] - 2026-09-25

Première version publique : terminal rapide en Rust avec onglets, splits, profils sauvegardés, hôtes SSH (import de `~/.ssh/config`, mots de passe dans le trousseau), 12 thèmes, confirmation avant de fermer un programme en cours, et mises à jour automatiques depuis GitHub.
