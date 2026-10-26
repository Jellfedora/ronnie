# Contribuer à Ronnie

Merci de vouloir donner un coup de main ! Quelques règles pour que ta pull request passe sans allers-retours.

## Proposer un changement

1. Forke le dépôt et crée une branche à partir de `main`.
2. Lance Ronnie en local avec `cargo run` : c'est un build **dev**, avec sa propre config (dossier `ronnie-dev`), qui ne touche pas à l'app installée. Pour partir d'une config vierge : `RONNIE_CONFIG_DIR=/tmp/ronnie cargo run`.
3. Vérifie que `cargo test --locked` passe.
4. Ouvre la pull request vers `main`. La CI la compile et la teste sur macOS, Linux et Windows. Pour une première contribution, elle démarre après validation du mainteneur.

Une PR = un sujet. Pour un gros changement, ouvre d'abord une issue pour en parler.

## Conventions

- **Commits** : en anglais, une phrase qui dit ce que l'utilisateur voit changer (« Processes: the page has a close button, back to the tab shown »), plutôt que ce que fait le code.
- **Textes de l'interface** : jamais en dur. Chaque texte passe par `src/i18n.rs`, en français **et** en anglais.
- **Raccourcis** dans les textes : avec des espaces entre les touches, « ⌘ F », jamais « ⌘F ».
- **Les trois systèmes** : Ronnie tourne sur macOS (Apple Silicon), Linux et Windows. Ce qui dépend du système (chemins, shell, raccourcis ⌘ / Ctrl) doit marcher partout, ou être prévu pour chacun.
- **Code** : suis le style des fichiers autour, commentaires en anglais.
- **CHANGELOG.md** et version : ne les modifie pas, ils sont mis à jour au moment de la release.

## Licence

En proposant une contribution, tu acceptes qu'elle soit publiée sous la [licence MIT](LICENSE) du projet.
