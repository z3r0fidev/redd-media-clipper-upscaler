//! Which engine the tests use, and skipping (or failing, in the bundled-engine jobs) when it lacks a feature.

use super::*;

/// Path to ffmpeg (`RMEDIA_FFMPEG` or PATH).
pub fn ffmpeg() -> PathBuf {
    std::env::var_os("RMEDIA_FFMPEG").map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into())
}

/// Whether the engine is a pinned release build (the bundled engine reports `version nX.Y`;
/// distro builds report e.g. `6.1.1-3ubuntu5`). Exact-timing and byte-identity checks are
/// measured on it only.
pub fn bundled_engine() -> bool {
    Command::new(ffmpeg())
        .args(["-hide_banner", "-version"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).starts_with("ffmpeg version n"))
}

/// Path to ffprobe (`RMEDIA_FFPROBE` or PATH).
pub fn ffprobe() -> PathBuf {
    std::env::var_os("RMEDIA_FFPROBE").map(PathBuf::from).unwrap_or_else(|| "ffprobe".into())
}

/// Whether media tests can run. Panics when `RMEDIA_REQUIRE_FFMPEG=1` and ffmpeg is missing.
pub fn ffmpeg_available() -> bool {
    let ok = Command::new(ffmpeg()).arg("-version").output().is_ok_and(|o| o.status.success());
    if !ok && std::env::var_os("RMEDIA_REQUIRE_FFMPEG").is_some_and(|v| v == "1") {
        panic!("ffmpeg is required (RMEDIA_REQUIRE_FFMPEG=1) but was not found");
    }
    ok
}

/// Filters and encoders the engine at [`ffmpeg()`] provides (from `-filters` and `-encoders`),
/// read once per test process.
pub(super) fn engine_features() -> &'static std::collections::BTreeSet<String> {
    static FEATURES: std::sync::OnceLock<std::collections::BTreeSet<String>> = std::sync::OnceLock::new();
    FEATURES.get_or_init(|| {
        let mut set = std::collections::BTreeSet::new();
        for flag in ["-filters", "-encoders"] {
            let Ok(out) = Command::new(ffmpeg()).args(["-hide_banner", flag]).output() else { continue };
            // Both listings print "<flags> <name> ..." rows after a header; take the name column.
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let mut cols = line.split_whitespace();
                if let (Some(flags), Some(name)) = (cols.next(), cols.next())
                    && flags.chars().all(|c| c.is_ascii_uppercase() || c == '.' || c == '|')
                    && flags.len() <= 6
                {
                    set.insert(name.to_owned());
                }
            }
        }
        set
    })
}

/// Whether the engine has ffmpeg plus every filter/encoder in `needs`. Tests for 002 call this
/// (through [`require_engine!`]) so they skip on older engines, such as the distro ffmpeg 6.1 in
/// the CI minimum-engine job. With `RMEDIA_REQUIRE_FFMPEG=1` a missing feature panics instead,
/// so the bundled-engine jobs can never skip silently.
pub fn require_engine(needs: &[&str]) -> bool {
    let strict = std::env::var_os("RMEDIA_REQUIRE_FFMPEG").is_some_and(|v| v == "1");
    if !ffmpeg_available() {
        return false;
    }
    let missing: Vec<&str> = needs.iter().copied().filter(|n| !engine_features().contains(*n)).collect();
    if missing.is_empty() {
        return true;
    }
    if strict && std::env::var_os("RMEDIA_ALLOW_MISSING_FEATURES").is_none_or(|v| v != "1") {
        panic!("engine lacks {missing:?} (RMEDIA_REQUIRE_FFMPEG=1)");
    }
    eprintln!("skipping: engine lacks {missing:?}");
    false
}
