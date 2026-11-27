//! floor: Ronnie's server (its own repository, next to Ronnie's): it hosts Ronnie's releases (the
//! updates come from it) and its download page.

/// Where floor runs (its own VM: no domain name, a Let's Encrypt certificate for the IP).
const URL: &str = "https://185.245.143.130/floor";

/// floor's address: RONNIE_FLOOR_URL when given (at run time, or when built); a dev build, a local floor.
pub fn url() -> Option<String> {
    let url = std::env::var("RONNIE_FLOOR_URL").ok().or_else(|| option_env!("RONNIE_FLOOR_URL").map(str::to_owned));
    let url = url.unwrap_or_else(|| if crate::config::OFFICIAL { URL } else { "http://localhost:5002" }.to_owned());
    let url = url.trim().trim_end_matches('/');
    (!url.is_empty()).then(|| url.to_owned())
}

/// The download page on floor's server (its root, floor being served under /floor), or floor's own
/// page (a floor without the prefix, as in development).
pub fn site() -> Option<String> {
    let url = url()?;
    Some(match url.strip_suffix("/floor") {
        Some(root) => format!("{root}/"),
        None => format!("{url}/download"),
    })
}
