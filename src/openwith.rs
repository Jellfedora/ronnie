//! Files of this computer opened with another program: the system's default one, one picked among those
//! the system says can open the file (as the Finder's, Explorer's or the desktop's "Open With"), or any
//! other program chosen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A program that can open a file.
#[derive(Clone, PartialEq, Debug)]
pub struct App {
    pub name: String,
    /// The system's default for this kind of file.
    pub default: bool,
    /// macOS: the .app bundle; Windows: the handler's name (to find it again); Linux: the .desktop file.
    id: PathBuf,
}

/// Opens `path` with the system's default program for it.
pub fn open(path: &Path) {
    #[cfg(target_os = "macos")]
    spawn(std::process::Command::new("open").arg(path));
    #[cfg(all(unix, not(target_os = "macos")))]
    spawn(std::process::Command::new("xdg-open").arg(path));
    #[cfg(windows)]
    windows::open(path);
}

/// The programs that can open `path`, the default one first.
pub fn apps_for(path: &Path) -> Vec<App> {
    cached(path, || list(path))
}

/// The programs for a file that is not on this computer (on a server): asked about an empty one of the
/// same name.
pub fn apps_for_name(name: &str) -> Vec<App> {
    cached(Path::new(name), || {
        let dir = std::env::temp_dir().join("ronnie-open-probe");
        let probe = dir.join(name);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&probe, b"");
        let apps = list(&probe);
        let _ = std::fs::remove_file(&probe);
        apps
    })
}

/// Programs listed for a kind of file, and when.
type Listed = (Instant, Vec<App>);

/// Asked once in a while by kind of file (every .xlsx gets the same programs): menus are drawn every frame.
fn cached(path: &Path, list: impl FnOnce() -> Vec<App>) -> Vec<App> {
    static CACHE: Mutex<Option<HashMap<String, Listed>>> = Mutex::new(None);
    let key = match path.extension() {
        Some(ext) => ext.to_string_lossy().to_lowercase(),
        None => path.display().to_string(),
    };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some((at, apps)) = cache.get(&key)
        && at.elapsed() < Duration::from_secs(30)
    {
        return apps.clone();
    }
    let mut apps = list();
    apps.sort_by_key(|a| (!a.default, a.name.to_lowercase()));
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.name.to_lowercase()));
    cache.insert(key, (Instant::now(), apps.clone()));
    apps
}

/// Opens `path` with `app`.
pub fn open_with(path: &Path, app: &App) {
    #[cfg(target_os = "macos")]
    spawn(std::process::Command::new("open").arg("-a").arg(&app.id).arg(path));
    #[cfg(all(unix, not(target_os = "macos")))]
    linux::launch(&app.id, path);
    #[cfg(windows)]
    windows::open_with(path, &app.id);
}

/// Asks which other program to open `path` with: the system's own window on Windows, a program to pick
/// elsewhere.
pub fn choose(path: &Path) {
    #[cfg(windows)]
    windows::choose(path);
    #[cfg(not(windows))]
    {
        let path = path.to_path_buf();
        // The picker blocks: not on the window's thread.
        std::thread::spawn(move || {
            #[cfg(target_os = "macos")]
            let picked = rfd::FileDialog::new().set_directory("/Applications").add_filter("Application", &["app"]).pick_file();
            #[cfg(not(target_os = "macos"))]
            let picked = rfd::FileDialog::new().set_directory("/usr/bin").pick_file();
            let Some(program) = picked else { return };
            #[cfg(target_os = "macos")]
            spawn(std::process::Command::new("open").arg("-a").arg(&program).arg(&path));
            #[cfg(not(target_os = "macos"))]
            spawn(std::process::Command::new(&program).arg(&path));
        });
    }
}

/// Whether `path` looks like a document for another program rather than text: something with a NUL
/// byte or not UTF-8 in its first kilobytes (Office files, PDFs, images...).
pub fn is_binary(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(file) = std::fs::File::open(path) else { return false };
    let mut head = Vec::with_capacity(8192);
    if file.take(8192).read_to_end(&mut head).is_err() {
        return false;
    }
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(&head) {
        Ok(_) => false,
        // Cut in the middle of a character at the end: still text.
        Err(e) => e.error_len().is_some(),
    }
}

/// Starts a program without waiting for it, but reaps it once it ends.
#[cfg(unix)]
fn spawn(command: &mut std::process::Command) {
    if let Ok(mut child) = command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(target_os = "macos")]
fn list(path: &Path) -> Vec<App> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let workspace = NSWorkspace::sharedWorkspace();
    let to_path = |u: &NSURL| u.path().map(|p| PathBuf::from(p.to_string()));
    let default = workspace.URLForApplicationToOpenURL(&url).and_then(|u| to_path(&u));
    workspace
        .URLsForApplicationsToOpenURL(&url)
        .iter()
        .filter_map(|u| to_path(&u))
        // The name the Finder shows, in the system's language ("Aperçu", not "Preview").
        .map(|id| {
            let shown = NSFileManager::defaultManager().displayNameAtPath(&NSString::from_str(&id.to_string_lossy())).to_string();
            let name = shown.strip_suffix(".app").unwrap_or(&shown).to_owned();
            App { name, default: default.as_ref() == Some(&id), id }
        })
        .filter(|a| !a.name.is_empty())
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn list(path: &Path) -> Vec<App> {
    linux::list(path)
}

#[cfg(windows)]
fn list(path: &Path) -> Vec<App> {
    windows::list(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    //! The desktop's programs: .desktop files of the XDG data folders, by the types they open
    //! (mimeinfo.cache, and mimeapps.list's added ones).

    use super::{spawn, App};
    use std::path::{Path, PathBuf};

    fn data_dirs() -> Vec<PathBuf> {
        let home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".local/share")));
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        home.into_iter().chain(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from)).map(|d| d.join("applications")).collect()
    }

    fn output(program: &str, args: &[&std::ffi::OsStr]) -> Option<String> {
        let out = std::process::Command::new(program).args(args).stderr(std::process::Stdio::null()).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (out.status.success() && !text.is_empty()).then_some(text)
    }

    fn mime_type(path: &Path) -> Option<String> {
        output("xdg-mime", &["query".as_ref(), "filetype".as_ref(), path.as_os_str()]).or_else(|| output("file", &["--mime-type".as_ref(), "-b".as_ref(), path.as_os_str()]))
    }

    /// The .desktop names listed for `mime` in the `[section]`s of an ini-like file.
    fn listed(text: &str, sections: &[&str], mime: &str) -> Vec<String> {
        let mut section = "";
        let mut found = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name;
            } else if let Some((key, value)) = line.split_once('=')
                && key.trim() == mime
                && sections.contains(&section)
            {
                found.extend(value.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned));
            }
        }
        found
    }

    fn find_desktop(name: &str) -> Option<PathBuf> {
        data_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file())
    }

    /// The `Name` and `Exec` of a .desktop file's main entry; None for hidden or terminal programs.
    fn read_desktop(path: &Path) -> Option<(String, String)> {
        let text = std::fs::read_to_string(path).ok()?;
        let (mut name, mut exec, mut in_entry) = (None, None, false);
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else { continue };
            match key.trim() {
                "Name" => name = Some(value.trim().to_owned()),
                "Exec" => exec = Some(value.trim().to_owned()),
                "NoDisplay" | "Hidden" | "Terminal" if value.trim() == "true" => return None,
                _ => {}
            }
        }
        Some((name?, exec?))
    }

    pub fn list(path: &Path) -> Vec<App> {
        let Some(mime) = mime_type(path) else { return Vec::new() };
        let default = output("xdg-mime", &["query".as_ref(), "default".as_ref(), mime.as_ref()]);
        let mut names: Vec<String> = default.iter().cloned().collect();
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")));
        for file in config.iter().map(|c| c.join("mimeapps.list")).chain(data_dirs().iter().map(|d| d.join("mimeapps.list"))) {
            if let Ok(text) = std::fs::read_to_string(file) {
                names.extend(listed(&text, &["Added Associations", "Default Applications"], &mime));
            }
        }
        for dir in data_dirs() {
            if let Ok(text) = std::fs::read_to_string(dir.join("mimeinfo.cache")) {
                names.extend(listed(&text, &["MIME Cache"], &mime));
            }
        }
        names
            .into_iter()
            .filter_map(|n| {
                let id = find_desktop(&n)?;
                let (name, _) = read_desktop(&id)?;
                Some(App { name, default: default.as_deref() == Some(n.as_str()), id })
            })
            .collect()
    }

    /// Runs a .desktop file's command on `path` (its %f / %u... field codes replaced).
    pub fn launch(desktop: &Path, path: &Path) {
        let Some((_, exec)) = read_desktop(desktop) else { return };
        let mut args = Vec::new();
        let mut given = false;
        for word in split(&exec) {
            match word.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    args.push(path.as_os_str().to_owned());
                    given = true;
                }
                w if w.starts_with('%') && w.len() == 2 => {}
                w => args.push(w.replace("%%", "%").into()),
            }
        }
        if !given {
            args.push(path.as_os_str().to_owned());
        }
        let Some((program, rest)) = args.split_first() else { return };
        spawn(std::process::Command::new(program).args(rest));
    }

    /// The words of an Exec line (double quotes, backslash escapes).
    fn split(exec: &str) -> Vec<String> {
        let (mut words, mut word, mut quoted, mut escaped, mut any) = (Vec::new(), String::new(), false, false, false);
        for c in exec.chars() {
            match c {
                _ if escaped => {
                    word.push(c);
                    escaped = false;
                }
                '\\' => escaped = true,
                '"' => {
                    quoted = !quoted;
                    any = true;
                }
                c if c.is_whitespace() && !quoted => {
                    if any || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                c => word.push(c),
            }
        }
        if any || !word.is_empty() {
            words.push(word);
        }
        words
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn exec_words() {
            assert_eq!(super::split(r#"libreoffice --calc "%U""#), ["libreoffice", "--calc", "%U"]);
            assert_eq!(super::split(r#""/opt/My App/app" %f"#), ["/opt/My App/app", "%f"]);
        }

        #[test]
        fn listed_types() {
            let text = "[MIME Cache]\ntext/plain=gedit.desktop;vim.desktop;\n[Other]\ntext/plain=no.desktop;";
            assert_eq!(super::listed(text, &["MIME Cache"], "text/plain"), ["gedit.desktop", "vim.desktop"]);
        }
    }
}

#[cfg(windows)]
mod windows {
    //! Explorer's handlers for the file's extension (as its "Open with" menu), and its "Choose another
    //! app" window.

    use super::App;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, IDataObject, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        BHID_DataObject, IAssocHandler, IShellItem, SHAssocEnumHandlers, SHCreateItemFromParsingName, SHOpenWithDialog, ShellExecuteW, ASSOC_FILTER_RECOMMENDED, OAIF_ALLOW_REGISTRATION,
        OAIF_EXEC, OAIF_REGISTER_EXT, OPENASINFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    pub fn open(path: &Path) {
        // SAFETY: the strings live through the call.
        unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    }

    /// The handlers recommended for `path`'s extension.
    fn handlers(path: &Path) -> Vec<IAssocHandler> {
        let Some(ext) = path.extension() else { return Vec::new() };
        let ext = HSTRING::from(format!(".{}", ext.to_string_lossy()));
        // SAFETY: plain COM calls (COM is set up on the window's thread by winit).
        unsafe {
            let Ok(list) = SHAssocEnumHandlers(&ext, ASSOC_FILTER_RECOMMENDED) else { return Vec::new() };
            let mut found = Vec::new();
            loop {
                let mut one = [None];
                let mut fetched = 0;
                if list.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                found.extend(one[0].take());
            }
            found
        }
    }

    fn take(text: windows::core::PWSTR) -> String {
        // SAFETY: a string the shell allocated for us, freed once read.
        unsafe {
            let s = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as _));
            s
        }
    }

    pub fn list(path: &Path) -> Vec<App> {
        handlers(path)
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                // SAFETY: COM calls on a live handler.
                let (id, name) = unsafe { (take(h.GetName().ok()?), take(h.GetUIName().ok()?)) };
                // The first one is the default.
                Some(App { name, default: i == 0, id: PathBuf::from(id) })
            })
            .collect()
    }

    pub fn open_with(path: &Path, id: &Path) {
        // SAFETY: COM calls; the handler is the one listed, found again by its name.
        unsafe {
            let Some(handler) = handlers(path).into_iter().find(|h| h.GetName().ok().map(|n| take(n)).is_some_and(|n| Path::new(&n) == id)) else { return };
            let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path.as_os_str()), None) else { return };
            let Ok(data) = item.BindToHandler::<_, IDataObject>(None, &BHID_DataObject) else { return };
            let _ = handler.Invoke(&data);
        }
    }

    /// Explorer's own window, with its "Always use this app" box. On its own thread: it waits for an answer.
    pub fn choose(path: &Path) {
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        std::thread::spawn(move || {
            let info = OPENASINFO { pcszFile: PCWSTR(file.as_ptr()), pcszClass: PCWSTR::null(), oaifInFlags: OAIF_EXEC | OAIF_ALLOW_REGISTRATION | OAIF_REGISTER_EXT };
            // SAFETY: COM set up for this thread; `file` outlives the call.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let _ = SHOpenWithDialog(None, &info);
            }
        });
    }
}

