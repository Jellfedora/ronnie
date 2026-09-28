//! The metal guard: before Enter runs a command that destroys a lot (rm -rf on / or ~, disks formatted,
//! databases dropped, a server rebooted...), Ronnie asks "Are you sure, warrior?".

/// Why a command line deserves a second thought, if it does.
#[derive(Debug, PartialEq)]
pub(super) enum Danger {
    /// Deletes this and everything in it.
    Delete(String),
    /// Changes the permissions or the owner of this, recursively.
    Permissions(String),
    Disk,
    ForkBomb,
    DropDatabase,
    /// DELETE without WHERE, or TRUNCATE: a whole table emptied.
    EmptyTable,
    /// UPDATE without WHERE: every row of a table changed.
    UpdateAll,
    DropTable,
    /// ALTER TABLE ... DROP (a column and its values).
    DropColumn,
    DockerVolumes,
    ForcePush(String),
    /// Reboots or shuts down the server (SSH panes only).
    Reboot,
}

/// Places whose recursive deletion (or chmod) is a disaster.
fn vital(target: &str) -> bool {
    let t = target.trim_end_matches('/');
    let t = if t.is_empty() && target.starts_with('/') { "/" } else { t };
    matches!(t, "/" | "/*" | "~" | "~/*" | "$HOME" | "${HOME}" | "$HOME/*" | "*" | ".." | "../*" | "." | "./*")
        || ["/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/opt", "/proc", "/root", "/sbin", "/srv", "/sys", "/usr", "/var", "/Applications", "/System", "/Library", "/Users"]
            .iter()
            .any(|d| t == *d || t == format!("{d}/*"))
}

/// Shell words of `line` (quotes removed), split into commands at ; && || | &.
fn commands(line: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    let push = |out: &mut Vec<Vec<String>>, word: &mut String, in_word: &mut bool| {
        if *in_word {
            out.last_mut().unwrap().push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                    in_word = true;
                }
            }
            (None, ' ' | '\t') => push(&mut out, &mut word, &mut in_word),
            (None, ';' | '|' | '&') => {
                push(&mut out, &mut word, &mut in_word);
                if out.last().is_some_and(|c| !c.is_empty()) {
                    out.push(Vec::new());
                }
            }
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    push(&mut out, &mut word, &mut in_word);
    out.retain(|c| !c.is_empty());
    out
}

/// SQL in `upper` (uppercased) that changes or drops a lot, statement by statement.
fn sql_danger(upper: &str) -> Option<Danger> {
    for statement in upper.split(';') {
        let words: Vec<&str> = statement.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
        let has = |w: &str| words.contains(&w);
        for (i, w) in words.iter().enumerate() {
            let next = words.get(i + 1).copied();
            match *w {
                // Not "ON UPDATE CURRENT_TIMESTAMP" in a table's definition.
                "UPDATE" if i.checked_sub(1).and_then(|p| words.get(p)) != Some(&"ON") && has("SET") && !has("WHERE") => return Some(Danger::UpdateAll),
                "DROP" if next == Some("TABLE") => return Some(Danger::DropTable),
                "DROP" if has("ALTER") && has("TABLE") && !matches!(next, Some("INDEX" | "KEY" | "FOREIGN" | "PRIMARY" | "CONSTRAINT" | "CHECK" | "PARTITION" | "TABLE" | "DEFAULT")) => return Some(Danger::DropColumn),
                _ => {}
            }
        }
    }
    None
}

/// What `line` would destroy, if it's one of the well-known disasters. `ssh`: typed on a server.
pub(super) fn check(line: &str, ssh: bool) -> Option<Danger> {
    let upper = line.to_uppercase();
    if upper.contains("DROP DATABASE") || upper.contains("DROP SCHEMA") {
        return Some(Danger::DropDatabase);
    }
    if upper.contains("TRUNCATE ") || (upper.contains("DELETE FROM") && !upper.contains("WHERE")) {
        return Some(Danger::EmptyTable);
    }
    if let Some(danger) = sql_danger(&upper) {
        return Some(danger);
    }
    if line.replace(' ', "").contains(":(){:|:&};:") {
        return Some(Danger::ForkBomb);
    }
    let raw = line.replace(' ', "");
    if [">/dev/sd", ">/dev/nvme", ">/dev/disk", ">/dev/hd", ">/dev/vd", ">/dev/mmcblk"].iter().any(|d| raw.contains(d)) {
        return Some(Danger::Disk);
    }
    for words in commands(line) {
        // The command itself, after sudo, env settings and the like.
        let mut i = 0;
        while i < words.len() && (matches!(words[i].as_str(), "sudo" | "doas" | "command" | "nohup" | "time" | "exec" | "env" | "-E") || words[i].contains('=') && !words[i].starts_with('-')) {
            i += 1;
        }
        let Some(cmd) = words.get(i) else { continue };
        let cmd = cmd.rsplit('/').next().unwrap_or(cmd);
        let args = &words[i + 1..];
        let flags: Vec<&str> = args.iter().filter(|a| a.starts_with('-')).map(String::as_str).collect();
        let targets: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(String::as_str).collect();
        let short = |c: char| flags.iter().any(|f| !f.starts_with("--") && f.contains(c));
        let long = |name: &str| flags.iter().any(|f| *f == name);
        match cmd {
            "rm" if short('r') || short('R') || long("--recursive") => {
                if let Some(t) = targets.iter().find(|t| vital(t)) {
                    return Some(Danger::Delete((*t).to_owned()));
                }
            }
            "chmod" | "chown" | "chgrp" if short('R') || long("--recursive") => {
                if let Some(t) = targets.iter().skip(1).find(|t| vital(t)) {
                    return Some(Danger::Permissions((*t).to_owned()));
                }
            }
            c if c.starts_with("mkfs") => return Some(Danger::Disk),
            "dd" if args.iter().any(|a| a.starts_with("of=/dev/") && !a.starts_with("of=/dev/null")) => return Some(Danger::Disk),
            "docker" | "docker-compose" | "podman" => {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                let has = |w: &str| a.contains(&w);
                if (has("down") && (has("-v") || has("--volumes"))) || (has("volume") && has("prune")) || (has("system") && has("prune") && (has("-a") || has("--all") || has("--volumes"))) {
                    return Some(Danger::DockerVolumes);
                }
            }
            "git" if args.first().is_some_and(|a| a == "push") && (short('f') || long("--force")) => {
                if let Some(b) = targets.iter().find(|t| matches!(t.rsplit(':').next(), Some("main" | "master"))) {
                    return Some(Danger::ForcePush((*b).to_owned()));
                }
            }
            "reboot" | "shutdown" | "poweroff" | "halt" if ssh => return Some(Danger::Reboot),
            "init" if ssh && targets.first().is_some_and(|t| *t == "0" || *t == "6") => return Some(Danger::Reboot),
            "systemctl" if ssh && targets.first().is_some_and(|t| matches!(*t, "reboot" | "poweroff" | "halt" | "kexec")) => return Some(Danger::Reboot),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_the_classics() {
        assert_eq!(check("rm -rf /", false), Some(Danger::Delete("/".into())));
        assert_eq!(check("sudo rm -fr /*", false), Some(Danger::Delete("/*".into())));
        assert_eq!(check("rm -r -f ~/", false), Some(Danger::Delete("~/".into())));
        assert_eq!(check("rm --recursive --force $HOME", false), Some(Danger::Delete("$HOME".into())));
        assert_eq!(check("cd /tmp && rm -Rf /etc", false), Some(Danger::Delete("/etc".into())));
        assert_eq!(check("chmod -R 777 /", false), Some(Danger::Permissions("/".into())));
        assert_eq!(check("sudo mkfs.ext4 /dev/sdb1", false), Some(Danger::Disk));
        assert_eq!(check("dd if=image.iso of=/dev/disk4 bs=4m", false), Some(Danger::Disk));
        assert_eq!(check("cat x > /dev/sda", false), Some(Danger::Disk));
        assert_eq!(check(":(){ :|:& };:", false), Some(Danger::ForkBomb));
        assert_eq!(check("mysql -e 'drop database prod'", false), Some(Danger::DropDatabase));
        assert_eq!(check("DELETE FROM users;", false), Some(Danger::EmptyTable));
        assert_eq!(check("UPDATE users SET admin = 1", false), Some(Danger::UpdateAll));
        assert_eq!(check("mysql -e 'update users set admin=1'", false), Some(Danger::UpdateAll));
        assert_eq!(check("DROP TABLE users", false), Some(Danger::DropTable));
        assert_eq!(check("ALTER TABLE users DROP COLUMN email", false), Some(Danger::DropColumn));
        assert_eq!(check("alter table users drop email", false), Some(Danger::DropColumn));
        assert_eq!(check("docker compose down -v", false), Some(Danger::DockerVolumes));
        assert_eq!(check("git push --force origin main", false), Some(Danger::ForcePush("main".into())));
        assert_eq!(check("sudo reboot", true), Some(Danger::Reboot));
        assert_eq!(check("systemctl poweroff", true), Some(Danger::Reboot));
    }

    #[test]
    fn lets_everyday_commands_through() {
        for line in [
            "rm -rf node_modules",
            "rm -rf ./build/",
            "rm -rf ~/Downloads/old",
            "rm /tmp/x",
            "chmod -R 755 storage",
            "dd if=/dev/zero of=/dev/null count=1",
            "DELETE FROM users WHERE id = 3;",
            "UPDATE users SET a = 1 WHERE id = 2",
            "ALTER TABLE t ADD COLUMN at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP",
            "ALTER TABLE t DROP INDEX idx_a",
            "CREATE TABLE t (a INT)",
            "docker compose down",
            "git push --force origin feature/x",
            "git push origin main",
            "echo rm -rf / is bad",
            "grep -r reboot .",
        ] {
            assert_eq!(check(line, true), None, "{line}");
        }
        // Rebooting this computer on purpose is its user's business.
        assert_eq!(check("sudo reboot", false), None);
    }
}
