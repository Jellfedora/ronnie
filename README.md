# Ronnie

Terminal rapide écrit en Rust : onglets, splits, profils sauvegardés et hôtes SSH, pour macOS, Linux et Windows.

## Installation

Télécharge l'archive de ton système dans la [dernière release](https://github.com/Jellfedora/ronnie/releases/latest). Ensuite, Ronnie se met à jour tout seul : quand une nouvelle version sort, il propose de l'installer puis de redémarrer (réglable dans Paramètres > Général).

### macOS (Apple Silicon et Intel)

1. Télécharge `ronnie-universal-apple-darwin.tar.gz`, ouvre-le et glisse **Ronnie.app** dans **Applications**.
2. L'app n'est pas signée par Apple : au premier lancement macOS la bloque. Ouvre **Réglages Système > Confidentialité et sécurité** et clique **Ouvrir quand même**, ou lance une fois dans un terminal :

   ```sh
   xattr -dr com.apple.quarantine /Applications/Ronnie.app
   ```

### Linux (x86_64)

```sh
tar xzf ronnie-x86_64-unknown-linux-gnu.tar.gz
./ronnie/install.sh
```

Ronnie est installé dans `~/.local/bin` avec une entrée dans le menu des applications.

### Windows (x86_64)

Décompresse `ronnie-x86_64-pc-windows-msvc.zip` où tu veux (par exemple `%LOCALAPPDATA%\Programs\Ronnie`) et lance `ronnie.exe`. SmartScreen peut afficher un avertissement au premier lancement : **Informations complémentaires > Exécuter quand même**.

## Développement

```sh
cargo run
```

- `RONNIE_CONFIG_DIR=/tmp/ronnie cargo run` lance une instance avec sa propre config (pratique pour tester sans toucher à ses profils).
- Les builds de debug ne cherchent pas de mise à jour ; `RONNIE_UPDATE_CHECK=1` force la vérification.
- `scripts/make-icon.py` régénère l'icône (`assets/icon/`) à partir de la police du logo.

## Publier une version

```sh
scripts/release.sh 0.2.0
```

Le script met à jour la version dans `Cargo.toml`, commit, crée le tag `v0.2.0` et pousse. GitHub Actions compile alors pour macOS (app universelle), Linux et Windows, puis publie la release avec les archives. Les Ronnie installés la proposent à leurs utilisateurs à leur prochaine vérification (au lancement puis toutes les 6 h).

`scripts/package.sh` produit localement l'archive du système courant dans `dist/`.
