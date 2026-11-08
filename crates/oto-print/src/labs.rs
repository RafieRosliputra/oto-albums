//! Lab profiles: example profiles shipped as data files in `labs/`, plus user profiles from disk.

use std::path::Path;

use oto_core::lab::LabProfile;

const BUNDLED: [(&str, &str); 4] = [
    ("generic-layflat-10x10.toml", include_str!("../../../labs/generic-layflat-10x10.toml")),
    ("generic-softcover-8x8-single.toml", include_str!("../../../labs/generic-softcover-8x8-single.toml")),
    ("generic-hardcover-landscape-11x8.5.toml", include_str!("../../../labs/generic-hardcover-landscape-11x8.5.toml")),
    ("generic-portrait-a4-layflat.toml", include_str!("../../../labs/generic-portrait-a4-layflat.toml")),
];

/// Bundled example profiles (invalid files are skipped and logged).
pub fn bundled() -> Vec<LabProfile> {
    BUNDLED
        .iter()
        .filter_map(|(name, s)| match LabProfile::from_toml(s) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!("bundled lab profile {name} is invalid: {e}");
                None
            }
        })
        .collect()
}

/// Load user profiles from a folder; returns (profiles, errors per file).
pub fn load_dir(dir: &Path) -> (Vec<LabProfile>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errs = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (ok, errs);
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| LabProfile::from_toml(&s).map_err(|e| e.to_string())) {
            Ok(l) => ok.push(l),
            Err(e) => errs.push(format!("{}: {e}", p.display())),
        }
    }
    (ok, errs)
}

/// All profiles: bundled first, then user ones (a user profile with the same id wins).
pub fn all(user_dir: Option<&Path>) -> (Vec<LabProfile>, Vec<String>) {
    let mut v = bundled();
    let mut errs = Vec::new();
    if let Some(d) = user_dir {
        let (u, e) = load_dir(d);
        errs = e;
        for p in u {
            v.retain(|x| x.id != p.id);
            v.push(p);
        }
    }
    (v, errs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_profiles_parse() {
        let v = super::bundled();
        assert_eq!(v.len(), 4);
        assert!(v.iter().any(|p| p.id == "generic-layflat-10x10"));
        assert!(v.iter().any(|p| p.binding.as_deref() == Some("single_pages")));
        assert!(v.iter().all(|p| p.is_example()));
    }
}
