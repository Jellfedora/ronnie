# Changelog

Les nouveautés de chaque version de Ronnie. La section d'une version sert aussi de notes à sa release GitHub.

## [0.13.0] - 2026-09-30

### Nouveautés

- **Autocomplétion SQL** dans la vue base de données (page SQL et requête au-dessus d'une table) : noms des tables après `FROM`, `JOIN`, `UPDATE`…, colonnes des tables de la requête avec leur type (alias compris : `u.` après `FROM users u`), mots-clés et fonctions MySQL / MariaDB. Tab complète, ↑ ↓ puis Entrée choisit, Échap ferme, ⌃ Espace ouvre la liste.
- **Windows : PowerShell** (7 s'il est installé, sinon celui du système) remplace cmd.exe, avec l'intégration de Ronnie : historique propre à chaque terminal, dossier courant suivi (split dans le même dossier, vue fichiers qui suit le terminal), suggestions de chemins, fin des longues commandes signalée.
- **Nouvelle page d'accueil**, affichée au lancement et quand on ferme l'onglet affiché : bonjour, nouveau terminal / hôte SSH / base de données en un clic, les onglets ouverts pour y revenir, et les profils, serveurs SSH et bases de données en cartes. Le jeu Speed Metal se cache désormais derrière trois clics sur le logo.
- **Onglets de requêtes** dans la page SQL d'une base : plusieurs requêtes côte à côte, chacune avec son résultat, pour comparer sans effacer la précédente. Le résultat d'une requête revient à son onglet même si on en a changé entre-temps, et la requête au-dessus d'une table garde son propre résultat.
- **Champ SQL au-dessus d'une table** : il montre la requête qui a lu les lignes affichées (tri, recherche et page compris), à modifier puis relancer. Il est plus haut, sur plusieurs lignes : ⌘ ↩ exécute, et l'autocomplétion est la même que dans la page SQL.
- **Réglages de l'accueil** (Réglages › Général) : écran de démarrage, astuces, cartes des serveurs SSH et des bases de données, chacun désactivable.
- **« Ne plus me le demander »** dans l'avertissement avant de fermer un onglet ou un pane où un programme tourne encore. Le réglage « Confirmer la fermeture d'un onglet actif » le réactive. Quitter Ronnie demande toujours.

### Corrections

- Linux (AppImage) : les fenêtres de Ronnie prenaient l'icône d'une autre application dans le dock. Elles portent maintenant l'identifiant « ronnie », et l'AppImage installe son entrée de menu et son icône dans `~/.local/share` quand rien ne l'a intégré (AppImageLauncher, Gear Lever…).
- Au lancement, les éléments de la barre latérale qui n'étaient pas ouverts apparaissaient surlignés comme l'onglet actif.
- Windows : Ronnie sait quel programme tourne dans un pane (avertissement avant de le fermer, badge).
- Windows : les questions de ssh sans terminal (vue fichiers SFTP, tunnels de la vue base de données) — mot de passe non enregistré, nouvelle clé d'hôte — s'affichent dans la fenêtre au lieu de faire échouer la connexion, et dans un terminal SSH elles se posent dans le terminal.
- Windows : le mot de passe enregistré n'est donné qu'à un ssh lancé par Ronnie, vérifié par la fenêtre comme sur macOS et Linux.

## [0.12.0] - 2026-09-30

### Nouveautés

- **Page d'accueil** : fermer un onglet (profil, connexion SSH…) ne bascule plus sur un autre onglet pris au hasard. Ronnie affiche sa page d'accueil, qu'on retrouve aussi en cliquant sur le logo, avec des astuces qui défilent. Et une petite surprise pour patienter.
- **Glisser-déposer depuis le Finder** (ou l'Explorateur) dans la vue fichiers : sur un dossier ou dans le panneau, les fichiers sont copiés en local ou envoyés sur le serveur ; un nom déjà pris propose Remplacer ou Ignorer.
- **Écrans de chargement** : connexion SSH d'un terminal, SFTP, base de données, lecture d'un dépôt git, d'un diff ou des issues ont chacun leur écran d'attente animé.

### Changements

- La barre latérale se replie et se déplie en glissant.
- Écran de démarrage : « Fait avec ❤ par Jellfedora ».

### Corrections

- Gestionnaire de fichiers : « Connexion au serveur en cours… » s'affiche pendant la connexion SFTP, au lieu de « Pas connecté au serveur ».

## [0.11.0] - 2026-09-29

### Nouveautés

- **Vue git plus complète** :
  - onglet **Historique** : les commits de la branche (branches et tags en pastilles, auteur, date), avec une recherche ; un commit ouvert montre son message et ses fichiers, chacun comparé côte à côte avec la version d'avant ;
  - **changer de branche** depuis le nom de la branche en haut de la vue : branches locales et distantes filtrables, création d'une nouvelle branche ;
  - commits à pousser et à récupérer (↑ ↓) et bouton **fetch** ;
  - onglet **Issues** GitHub (via le CLI `gh`) : liste ouvertes / fermées / toutes avec recherche, une issue avec ses commentaires, **créer une issue**, commenter, fermer ou rouvrir.
- **Ouvrir dans une nouvelle fenêtre** (clic droit sur le bouton git ou 📁 d'un panneau) : la vue git ou le gestionnaire de fichiers dans une fenêtre à part, sans restreindre la taille du panneau.
- Paramètres > **Fonctionnalités** : tout ce que Ronnie sait faire, regroupé par thème.

### Changements

- Menu du clic droit d'un terminal réorganisé : copier-coller, divisions, dossier, commandes, puis le panneau lui-même.

### Corrections

- Fermer un panneau termine tout ce qui y tournait (programme au premier plan comme tâches en arrière-plan), comme la fermeture d'une fenêtre de terminal ; en quittant Ronnie, ce qui n'est pas encore arrêté l'est aussi.

## [0.10.0] - 2026-09-29

### Nouveautés

- **Vue git d'un pane** : dans un dossier suivi par git, un bouton de branche dans la barre du pane affiche, à la place du terminal, les **fichiers modifiés** (M, A, D, R, U) et, pour celui choisi, sa version du dernier commit et sa version actuelle **côte à côte**, colorées, les changements en rouge et vert. Défilement horizontal, séparateurs à glisser pour régler les largeurs, **mini-carte** du fichier façon VS Code (cliquer ou glisser pour s'y rendre), ↑ ↓ d'une modification à l'autre. En lecture seule.
- **Commandes au démarrage d'un pane** (clic droit ou ⚙ > Commandes au démarrage…) : une commande par ligne (`cd ~/projet`, `nvm use 20`, `npm run dev`…), tapées quand le terminal s'ouvre, une fois son invite affichée. Le bouton ▶ **relance** le terminal et les tape à nouveau. Enregistrées avec la session et les profils.
- **Bouton ⚙ sur chaque pane** : le menu du clic droit, sans avoir à y penser.
- **Rouvrir un terminal fermé** : un pane renommé (ou avec des commandes au démarrage) qu'on ferme reste disponible dans le menu du pane > Rouvrir un terminal fermé, avec son dossier, son historique, ce qu'il affichait et ses commandes.
- Bases de données :
  - Chaque modification faite via l'interface (cellule, ligne, colonne, table, compte…) **montre la requête SQL** qu'elle va exécuter, à valider ; Annuler revient au formulaire. Désactivable dans Réglages > Général.
  - **Coloration syntaxique** du SQL (éditeur, champ au-dessus des tables, confirmations) et bouton **Formater** (⌘ ⇧ F) : une clause par ligne, mots-clés en majuscules.
  - **Historique** complet : les requêtes tapées et les modifications faites via l'interface, avec la date, la base, la durée et le résultat (ou l'erreur) ; recherche, rechargement d'un clic, « Vider l'historique ». Les mots de passe des comptes n'y sont pas écrits.
- Réglages > **Journal** : ce que Ronnie a noté (démarrages, relances, erreurs), avec les dates, à copier pour un rapport de bug ou à vider.

### Corrections

- Linux (Debian…) avec bash : le terminal local charge `~/.bashrc` comme le terminal du système (invite colorée, `ls` en couleur, alias), l'**autocomplétion des chemins** fonctionne, chaque terminal garde son historique et les fins de longues commandes sont notifiées.
- SSH avec un fichier de clé : le mot de passe enregistré sert aussi de **passphrase** de la clé, dans le terminal comme dans la vue fichiers (elle était redemandée à chaque connexion).
- Redémarrage après une mise à jour : la nouvelle version n'est lancée qu'une fois l'ancienne fermée, et le bouton ferme bien toute l'application même cliqué depuis une autre fenêtre. Chaque étape est notée dans le journal.

## [0.9.1] - 2026-09-28

### Changements

- **Mises à jour plus rapides** : sur Mac, un seul téléchargement pour Apple Silicon, deux fois plus léger que l'ancienne app universelle ; la progression s'affiche (pourcentage et Mo reçus) pendant le téléchargement.
- Fichiers publiés : **macOS Apple Silicon**, **Linux en AppImage** et **Windows**. Les Mac Intel et l'archive Linux `.tar.gz` ne sont plus fournis (une installation Linux faite avec l'archive ne se met plus à jour d'elle-même : passer à l'AppImage).

- Gestionnaire de fichiers : une ligne **« .. »** en haut de la liste mène au dossier parent (double-clic) ; on peut y **déposer** des fichiers et dossiers pour les remonter d'un niveau (ou les y transférer depuis l'autre panneau).

### Corrections

- Barre latérale : le texte « Base locale détectée » ne passe plus sous le bouton Ajouter.

## [0.9.0] - 2026-09-28

### Nouveautés

- **Bases de données MariaDB / MySQL**, façon phpMyAdmin : une section « Bases de données » dans la barre latérale (la base locale est détectée et proposée), des connexions comme dans un client lourd (adresse, port, utilisateur, mot de passe chiffré), ou **à travers un hôte SSH** (tunnel ouvert par Ronnie, pour les serveurs qui n'écoutent que sur leur machine).
  - Bases et tables à gauche ; pour une table : **Contenu** paginé et triable, **recherche** dans toutes les colonnes, **champ SQL** au-dessus du tableau, **double-clic pour modifier une cellule** (fenêtre pour les valeurs longues), mettre à NULL, **cases à cocher** pour supprimer des lignes, **insérer une ligne**.
  - **Structure** : modifier le type, le nom, la valeur par défaut, le commentaire d'une colonne (sans perdre sa collation), en ajouter, en supprimer ; index et CREATE TABLE.
  - **Nouvelle table** (formulaire de colonnes), renommer, vider, supprimer une table ; créer, supprimer une base.
  - **Export** d'une base ou d'une table en .sql (ou .sql.gz) cohérent, avec vues, procédures, fonctions, triggers et événements ; **export CSV** d'une table ou d'un résultat ; **import** de .sql / .sql.gz lu au fil de l'eau, même très gros. Option « Vérifier les clés étrangères », cochée par défaut.
  - Page **SQL** avec historique des requêtes ; **bouton Arrêter** pour une requête trop longue, un export ou un import ; reconnexion automatique.
  - **Utilisateurs** : liste des comptes et de leurs droits, créer un compte, changer son mot de passe, accorder ou retirer des droits, supprimer.
  - Le garde-fou demande confirmation avant un DROP, un TRUNCATE, un DELETE ou un UPDATE sans WHERE, un ALTER TABLE … DROP (aussi dans le terminal).
- **Barre latérale repliable** (⌘ B, ou le bouton « en haut) : une colonne de badges avec le R de Ronnie, les icônes des sections, tous les terminaux, profils, serveurs et bases, leurs menus et leurs « + ». Les **sections** Local, SSH et Bases se replient aussi, dans les deux modes.
- **Menu Fenêtre > Nouvelle fenêtre** sur macOS ; au relancement, **toutes les fenêtres** ouvertes sont restaurées.
- Gestionnaire de fichiers :
  - **Déplacer par glisser-déposer** des fichiers et dossiers dans un dossier du même panneau, en local comme en SSH ; un élément du même nom : **Remplacer** (les dossiers sont fusionnés) ou **Ignorer**, avec « Toujours faire ce choix pendant cette session » (aussi pour les transferts).
  - **Sélection au lasso** : glisser depuis la zone vide à côté des noms.
  - **Taille d'un dossier** : clic droit > Calculer la taille.
- Écran de démarrage : un rappel que Ronnie est en **version alpha**, à ne pas utiliser pour des opérations de production.

### Corrections

- Seule la dernière adresse locale ouverte par un terminal est proposée en raccourci (plus toute la liste des ports).

## [0.8.0] - 2026-09-28

### Nouveautés

- **Paramètres entièrement redessinés** : navigation à gauche (Général, Apparence, Raccourcis, Profils, SSH, Configuration, Informations), un titre et une explication par page, les réglages groupés en cartes avec un nom court, une description et des interrupteurs. Nouvelle page **Apparence** (taille de l'interface et du texte, thèmes). Pages SSH et Profils revues : badges, une carte par groupe, méthode de connexion affichée, nom d'un profil modifiable sur place.
- **Barre latérale plus soignée** : badges à l'initiale colorée (ronds pour les serveurs SSH, avec un point vert quand ils sont connectés), élément ouvert mis en valeur, en-têtes de section et nombre d'éléments par groupe.
- **Notifications dans la fenêtre** : quand une longue commande se termine dans un autre onglet alors que Ronnie est au premier plan, un avis aux couleurs du thème s'affiche (un clic mène à l'onglet). Réglages : dans Ronnie, notification du système ou les deux ; emplacement (six positions) ; bouton pour tester.
- **Renommer un panneau** : double-clic sur sa barre, ou clic droit > Renommer le panneau. Le nom s'affiche devant le dossier et se garde dans la session et les profils.
- **Croix pour fermer un panneau**, dans sa barre (avec avertissement si un programme tourne).
- Suggestions de chemins dans le terminal : **↑ ↓** pour choisir, **Entrée** pour prendre celle choisie, **Échap** pour fermer la liste.

### Corrections

- Le ✓ / ✗ de fin de commande s'affiche aussi sur les profils et les hôtes SSH de la barre latérale (seuls les terminaux simples l'avaient).
- La page Profils vide n'affiche plus le message de la section SSH.

## [0.7.1] - 2026-09-28

### Nouveautés

- **Glisser-déposer des panneaux** : dans un onglet divisé, attrape la barre en haut d'un terminal et lâche-la sur un autre : ils échangent leur place, programmes en cours compris. Le panneau survolé s'illumine (⇄). La barre s'affiche désormais sur tous les panneaux d'un onglet divisé, même quand l'affichage du dossier est désactivé.

### Corrections

- « Gérer les commandes… » (clic droit > ⚡ Commandes) ouvre bien la liste des commandes.
- « contenu restauré » ne s'affiche plus au-dessus d'un terminal où il n'y avait rien, et ne s'empile plus au fil des redémarrages.

## [0.7.0] - 2026-09-28

### Nouveautés

- **Plusieurs fenêtres** : clic droit sur un profil ou un hôte SSH > Ouvrir dans une nouvelle fenêtre, clic droit sur un onglet > Déplacer dans une nouvelle fenêtre (sans couper ce qui tourne), ou **⌘ N**. Chaque fenêtre a ses onglets ; un profil déjà ouvert ailleurs y ramène au lieu d'être dupliqué. Les fenêtres se rouvrent à leur place au lancement suivant.
- **Le contenu des terminaux revient** à la réouverture de Ronnie, couleurs comprises (les 5000 dernières lignes, en local comme en SSH), marqué « contenu restauré ». Désactivable dans Paramètres > Affichage.
- **Garde-fou métal** : avant qu'Entrée lance une commande destructrice, Ronnie demande « Tu es sûr, guerrier ? » en expliquant ce qu'elle va faire. `rm -rf` sur `/`, `~` ou un dossier système, `chmod -R` / `chown -R` au même endroit, formatage ou écrasement de disque, fork bomb, `DROP DATABASE`, `DELETE` sans `WHERE`, `TRUNCATE`, volumes Docker supprimés, `git push --force` sur main / master, et sur un serveur `reboot` / `shutdown`. Désactivable dans Paramètres > Affichage.
- **Autocomplétion des chemins** :
  - dans le terminal, en local (zsh) comme sur un serveur SSH : les fichiers et dossiers qui complètent le chemin tapé s'affichent sous le curseur, **→** complète, **⌥ ↑ ↓** choisit. Sur un serveur, la ligne est lue après l'invite habituelle et les fichiers viennent par SFTP (hôtes qui se connectent sans rien demander) ;
  - dans la barre de chemin du gestionnaire de fichiers (local et serveur) : Tab complète, ↑ ↓ choisit.
- **Permissions en local** aussi, dans la même fenêtre qu'en SFTP, avec les bits spéciaux (setuid, setgid, sticky), une valeur octale ou textuelle (`u+x`, `go-w`, `rwxr-xr-x`), des cases mixtes quand plusieurs éléments diffèrent (chacun garde son bit), et en récursif : tout, fichiers seulement ou dossiers seulement.
- **SSH : option Couleurs** par hôte, pour les serveurs qui ne colorent rien : invite colorée (rouge pour root), `ls`, `grep` et `diff` en couleur, la configuration du serveur étant chargée d'abord (bash).

### Corrections

- Cliquer un onglet, un profil ou un hôte SSH donne aussitôt le clavier au terminal.
- Le lien « Nouveautés » des mises à jour s'ouvre bien.
- Icônes plus grandes en haut à droite des terminaux.

## [0.6.0] - 2026-09-28

### Nouveautés

- **Éditeur de fichiers intégré** au gestionnaire de fichiers : clic droit > Modifier, Entrée, ou double-clic sur un fichier local. Le fichier s'ouvre à la place des panneaux, sans logiciel externe.
  - Numéros de ligne, coloration pour une trentaine de formats (YAML, JSON, `.env`, nginx, Dockerfile, shell, PHP, JS/TS, Python, HTML, CSS, SQL…).
  - **⌘ S** enregistre directement sur le serveur : le fichier garde son propriétaire et ses permissions. S'il a été modifié ailleurs depuis son ouverture, Ronnie propose d'écraser ou de recharger.
  - Rechercher (⌘ F, ⌘ G / ⇧ ⌘ G, respect de la casse), remplacer et tout remplacer, aller à la ligne (⌘ L).
  - Indentation détectée (tabulations, 2 ou 4 espaces), Tab / ⇧ Tab sur la sélection, Entrée garde l'indentation, ⌘ / commente les lignes.
  - Fins de ligne CRLF et BOM conservés ; « Enregistrer les modifications ? » avant de fermer.
- **Gros fichiers** :
  - de 1 à 200 Mo, un moteur d'édition qui ne dessine que les lignes visibles : frappe, défilement et recherche restent fluides, même sur des millions de lignes ;
  - au-delà, une **visionneuse en lecture seule** de taille illimitée qui lit le fichier par morceaux, cherche dans tout le fichier et peut **suivre** ce qui s'ajoute à la fin, comme `tail -f` (aussi par clic droit > Afficher en lecture seule).
- **Fichiers des terminaux locaux** : ⌘ E, 📁 dans l'en-tête ou clic droit > Fichiers de ce dossier ouvre le dossier courant du terminal, avec les mêmes outils (éditeur, visionneuse, zip…).
- **Icônes selon le format** des fichiers : images, vidéos, audio, archives, code, scripts, configuration, documents, bases de données, clés et certificats.
- **Nouveau fichier** (bouton ＋ ou clic droit), ouvert aussitôt dans l'éditeur, et **Dupliquer** des fichiers et dossiers (« nom copie.txt »).
- **Compresser en zip** (bouton 📦 ou clic droit) un ou plusieurs fichiers et dossiers. Sur un serveur, l'archive est faite sur place (`zip`, ou Python), sans rien faire transiter par cet ordinateur.
- **SSH : dossier de départ** par hôte, où s'ouvrent ses sessions.

### Corrections

- La fenêtre qui demande une passphrase ou un mot de passe pour le gestionnaire de fichiers se valide avec Entrée.

## [0.5.0] - 2026-09-28

### Nouveautés

- **Fin des commandes longues signalée** : quand une commande d'au moins 10 secondes se termine alors que Ronnie est au second plan, une notification du système indique la commande, sa durée et si elle a échoué (avec son code). Si elle tournait dans un autre onglet, un ✓ vert ou un ✗ rouge s'affiche sur l'onglet jusqu'à ce que tu y reviennes (détail au survol). Réglable dans Paramètres > Général (activer, durée minimale).
- **Un nouvel onglet s'ouvre dans le dossier du terminal affiché**, comme le faisaient déjà les splits.
- **SSH : Ronnie suit le dossier courant sur le serveur**, quand le shell distant l'indique (OSC 7, ou le titre `utilisateur@machine: dossier` du bash par défaut de Debian et Ubuntu) :
  - l'en-tête du panneau l'affiche à côté de l'hôte ;
  - un split ouvre la nouvelle session dans le même dossier ;
  - le gestionnaire de fichiers (📁) s'ouvre sur ce dossier, et y retourne quand le terminal a changé de dossier depuis.
- **AppImage pour Linux** (Ubuntu, Debian, Fedora…) : `Ronnie-x86_64.AppImage`, un seul fichier à rendre exécutable puis lancer, sans FUSE 2 à installer. Il se met à jour lui-même, comme les autres versions.

### Corrections

- Le curseur « Taille de l'interface » ne s'emballe plus quand on le fait glisser : la nouvelle taille s'applique au relâchement.

## [0.4.0] - 2026-09-26

### Nouveautés

- **Choix de la méthode de connexion** par hôte, comme dans FileZilla : automatique (agent SSH), mot de passe enregistré, demander le mot de passe, interactif (code, double authentification) ou fichier de clé.
- Bouton **Parcourir…** pour choisir la clé privée dans le Finder ou l'Explorateur, et exemple de chemin adapté au système.
- Avertissement pour les clés PuTTY (.ppk), que ssh ne sait pas lire, avec la façon de les convertir.
- **Dupliquer** un profil ou un hôte SSH (clic droit). Un hôte copié s'ouvre dans l'éditeur, avec le même mot de passe enregistré, pour changer son adresse avant de l'enregistrer.
- En-tête des panneaux SSH : icônes seules (↻ reconnecter, ⚡ commandes, 📁 fichiers), le détail au survol.

### Changements

- **Mots de passe enregistrés sans le trousseau du système** : plus de demande d'accès à chaque connexion ou mise à jour. Ils sont chiffrés dans `passwords.json`, avec une clé rangée dans un autre fichier. Ceux enregistrés dans le trousseau par les versions précédentes sont à retaper une fois.

## [0.3.0] - 2026-09-26

### Nouveautés

- **Gestionnaire de fichiers façon FileZilla** pour chaque serveur SSH (bouton 📁 Fichiers dans l'en-tête, ⌘ E, ou clic droit sur l'hôte > Fichiers (SFTP)) : cet ordinateur à gauche, le serveur à droite. Transferts par double-clic ou glisser-déposer (y compris sur un dossier), dossiers entiers, file d'attente avec progression, vitesse et annulation, remplacer / ignorer les fichiers existants. Renommer, supprimer, nouveau dossier, **permissions** (grille rwx, valeur octale, récursif). Utilise la configuration ssh habituelle (clés, agent, rebond, mots de passe enregistrés) ; un mot de passe ou une nouvelle empreinte d'hôte sont demandés dans une fenêtre. Les dossiers sont mémorisés par serveur.
- Une ligne de transfert par fichier ou dossier sélectionné, chacune avec sa progression et son annulation.
- **⌘ + / ⌘ − / ⌘ 0 agrandissent toute l'interface** (barre latérale, paramètres, fichiers, terminal), de 60 à 200 %, mémorisé ; la taille du texte du terminal reste réglable à part.

### Sécurité et fiabilité du gestionnaire de fichiers

- Un serveur malveillant ne peut pas écrire en dehors du dossier de téléchargement (noms suspects comme `../../.zshrc` refusés, comme le fait OpenSSH).
- Les opérations récursives ne suivent jamais les liens symboliques ; les téléchargements passent par un fichier temporaire (jamais de fichier tronqué) et s'arrêtent si le serveur envoie plus que prévu.
- Une coupure réseau ou une session fermée est détectée (« Déconnecté », avec la raison, et bouton Reconnecter).
- Le temps de taper un mot de passe ou de lire une empreinte ne fait plus échouer la connexion ; la fenêtre indique quel serveur demande.
- Annulation immédiate, y compris d'un transfert en attente ; les fichiers ignorés sont signalés.
- Listes de milliers de fichiers fluides (seules les lignes visibles sont dessinées) ; envois jusqu'à 4× plus rapides vers un serveur lointain.

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
