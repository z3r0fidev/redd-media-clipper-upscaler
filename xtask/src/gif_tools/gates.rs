//! The release and bundle gates: a tree or bundle with gifski needs the application licence decision (FR-025).

use super::*;

/// Application licences under which gifski (AGPL-3.0-or-later) may ship inside rmedia, or the
/// marker for a bought gifski licence. The owner edits this list only together with the
/// licence decision.
/// `commercial-gifski-imagequant`: bought licences for both gifski and imagequant (GPL-3.0-or-later,
/// compiled into gifski); a gifski licence alone does not cover imagequant.
pub const GATE_ALLOWLIST: [&str; 5] = [
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "commercial-gifski-imagequant",
];

/// Why a release may not ship from `tree` (`None`: it may). It may when no gifski is staged in
/// `tree/binaries/`, or when `tree` has a root `LICENSE` and `[workspace.metadata.rmedia]
/// app_licence` in its `Cargo.toml` names an allowlisted id.
pub fn release_gate(tree: &Path) -> Result<Option<String>> {
    let staged = std::fs::read_dir(tree.join("binaries"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("rmedia-gifski"));
    if !staged {
        return Ok(None);
    }
    licence_gate(tree, "is staged")
}

/// The gate over built output (security review L3): any file under `dirs` named after gifski,
/// or an NSIS/WiX installer script that mentions it, needs the same licence decision as a staged
/// gifski. Catches a bundle that has gifski although the staging step did not.
pub fn bundle_gate(tree: &Path, dirs: &[PathBuf]) -> Result<Option<String>> {
    let mut found = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            bail!("{} is not a folder", dir.display());
        }
        gifski_files(dir, &mut found)?;
    }
    match found.first() {
        None => Ok(None),
        Some(f) => licence_gate(tree, &format!("is in the build ({})", f.display())),
    }
}

/// Build folders that hold intermediate artifacts, not bundle contents.
pub(super) const NOT_BUNDLED: [&str; 5] = ["build", "deps", "incremental", ".fingerprint", "examples"];

pub(super) fn gifski_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let e = e?;
        let (path, name) = (e.path(), e.file_name().to_string_lossy().to_ascii_lowercase());
        // Symbolic links are not followed: they cannot add files to a bundle tree by themselves.
        let kind = e.file_type()?;
        if kind.is_dir() {
            if !NOT_BUNDLED.contains(&name.as_str()) {
                gifski_files(&path, found)?;
            }
        } else if name.contains("gifski")
            || ((name.ends_with(".nsi") || name.ends_with(".wxs")) && mentions_gifski(&std::fs::read(&path)?))
        {
            found.push(path);
        }
    }
    Ok(())
}

/// Whether an installer script names gifski, in UTF-8 or UTF-16 (zero bytes are skipped).
pub(super) fn mentions_gifski(bytes: &[u8]) -> bool {
    let text: Vec<u8> = bytes.iter().filter(|b| **b != 0).map(u8::to_ascii_lowercase).collect();
    text.windows(6).any(|w| w == b"gifski")
}

/// Why gifski, which `what` (e.g. "is staged"), may not ship from `tree` (`None`: it may).
pub(super) fn licence_gate(tree: &Path, what: &str) -> Result<Option<String>> {
    let manifest = std::fs::read_to_string(tree.join("Cargo.toml")).context("read the root Cargo.toml")?;
    let licence = app_licence(&manifest);
    let has_license_file = tree.join("LICENSE").is_file();
    match (licence, has_license_file) {
        (Some(l), true) if GATE_ALLOWLIST.contains(&l.as_str()) => Ok(None),
        (Some(l), true) => Ok(Some(format!(
            "gifski (AGPL-3.0) {what} but app_licence {l:?} is not one it may ship under ({})",
            GATE_ALLOWLIST.join(", ")
        ))),
        _ => Ok(Some(format!(
            "gifski (AGPL-3.0) {what} but the application licence is undecided (docs/roadmap.md): \
             add a root LICENSE and [workspace.metadata.rmedia] app_licence, or stage without gifski \
             (cargo xtask fetch-tools --no-gifski)"
        ))),
    }
}

/// `app_licence = "…"` in the `[workspace.metadata.rmedia]` table of a Cargo.toml (a minimal
/// reader: the table header, then `key = "value"` lines until the next table).
pub(super) fn app_licence(manifest: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_table = line == "[workspace.metadata.rmedia]";
            continue;
        }
        if in_table
            && let Some(value) = line.strip_prefix("app_licence").map(str::trim_start)
            && let Some(value) = value.strip_prefix('=')
        {
            let value = value.split('#').next().unwrap_or_default().trim();
            return value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).map(str::to_owned);
        }
    }
    None
}

/// `check-release-gate [--tree DIR] [--scan DIR]…`: the staged programs, and with `--scan` the
/// built bundles and archives too.
pub fn check_release_gate(tree: Option<&Path>, scan: &[PathBuf]) -> Result<()> {
    let tree = tree.map(Path::to_path_buf).unwrap_or_else(root);
    let reason = match release_gate(&tree)? {
        None if !scan.is_empty() => bundle_gate(&tree, scan)?,
        other => other,
    };
    match reason {
        None => {
            println!("release gate: ok");
            Ok(())
        }
        Some(reason) => bail!("release gate: {reason}"),
    }
}
