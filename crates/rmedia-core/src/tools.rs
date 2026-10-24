//! Locating ffmpeg/ffprobe: explicit path → env → bundled beside the executable → PATH.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};

pub const MIN_VERSION: (u32, u32) = (6, 0);
pub const BUNDLED_FFMPEG: &str = "rmedia-ffmpeg";
pub const BUNDLED_FFPROBE: &str = "rmedia-ffprobe";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ToolSource {
    /// `--ffmpeg`/settings or `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE`.
    Config,
    Bundled,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Tool {
    #[serde(serialize_with = "crate::pathser::path")]
    pub path: PathBuf,
    pub source: ToolSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Version {
    Release {
        major: u32,
        minor: u32,
        text: String,
    },
    /// Git/nightly builds report no release number; treated as recent.
    Git {
        text: String,
    },
}

impl Version {
    pub fn text(&self) -> &str {
        match self {
            Version::Release { text, .. } | Version::Git { text } => text,
        }
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Version::Release { major, minor, .. } => (*major, *minor) >= MIN_VERSION,
            Version::Git { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tools {
    pub ffmpeg: Tool,
    pub ffprobe: Tool,
    pub version: Version,
}

#[derive(Debug, Clone, Default)]
pub struct LocateOptions {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    /// The optional GIF engines (feature 003; `None`: the settings' path, then the usual
    /// lookup).
    pub gifsicle: Option<PathBuf>,
    pub gifski: Option<PathBuf>,
    /// Directory searched for bundled binaries; defaults to the current executable's directory.
    pub bundled_dir: Option<PathBuf>,
}

/// Finds both tools and checks the ffmpeg version.
pub fn locate(opts: &LocateOptions) -> Result<Tools> {
    let bundled_dir = opts
        .bundled_dir
        .clone()
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)));
    let ffmpeg =
        find_tool(opts.ffmpeg.as_deref(), "RMEDIA_FFMPEG", BUNDLED_FFMPEG, "ffmpeg", bundled_dir.as_deref())?;
    let ffprobe = find_tool(
        opts.ffprobe.as_deref(),
        "RMEDIA_FFPROBE",
        BUNDLED_FFPROBE,
        "ffprobe",
        bundled_dir.as_deref(),
    )?;
    let output = quiet_command(&ffmpeg.path)
        .arg("-hide_banner")
        .arg("-version")
        .output()
        .map_err(|e| Error::EngineMissing(format!("cannot run {}: {e}", ffmpeg.path.display())))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = parse_version(&text).ok_or_else(|| {
        Error::EngineMissing(format!("{} did not report an ffmpeg version", ffmpeg.path.display()))
    })?;
    if !version.is_supported() {
        return Err(Error::EngineMissing(format!(
            "ffmpeg {} is too old; version {}.{} or newer is required",
            version.text(),
            MIN_VERSION.0,
            MIN_VERSION.1
        )));
    }
    Ok(Tools { ffmpeg, ffprobe, version })
}

fn find_tool(
    explicit: Option<&Path>,
    env_var: &str,
    bundled_name: &str,
    path_name: &str,
    bundled_dir: Option<&Path>,
) -> Result<Tool> {
    if let Some(p) = explicit {
        return existing(p, ToolSource::Config);
    }
    if let Some(p) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        return existing(Path::new(&p), ToolSource::Config);
    }
    bundled_or_path(bundled_name, path_name, bundled_dir).ok_or_else(|| {
        Error::EngineMissing(format!(
            "{path_name} not found: not bundled with rmedia and not on PATH \
                 (use --{path_name} PATH)"
        ))
    })
}

/// The bundled copy beside rmedia, else `path_name` on PATH (only when nothing is bundled).
fn bundled_or_path(bundled_name: &str, path_name: &str, bundled_dir: Option<&Path>) -> Option<Tool> {
    if let Some(dir) = bundled_dir {
        let candidate = dir.join(format!("{bundled_name}{}", std::env::consts::EXE_SUFFIX));
        if candidate.is_file() {
            return Some(Tool { path: candidate, source: ToolSource::Bundled });
        }
    }
    which::which(path_name).ok().map(|path| Tool { path, source: ToolSource::Path })
}

/// Where an optional tool was found (feature 003's gifsicle and gifski), in [`find_tool`]'s
/// order: explicit path → the environment variable's value (`env`) → bundled → PATH, with PATH
/// used only when no bundled copy exists. `None`: not configured and not found. `Some(Err)`: an
/// explicit or environment path that does not exist (the tool is then unavailable; the next
/// places are not tried).
pub(crate) fn find_optional(
    explicit: Option<&Path>,
    env: Option<&Path>,
    bundled_name: &str,
    path_name: &str,
    bundled_dir: Option<&Path>,
) -> Option<std::result::Result<Tool, String>> {
    if let Some(p) = explicit.or(env) {
        return Some(existing(p, ToolSource::Config).map_err(|e| e.to_string()));
    }
    bundled_or_path(bundled_name, path_name, bundled_dir).map(Ok)
}

fn existing(p: &Path, source: ToolSource) -> Result<Tool> {
    if p.is_file() {
        // Absolute, resolved path: never depends on the working directory at spawn time.
        let path = crate::paths::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        Ok(Tool { path, source })
    } else {
        Err(Error::EngineMissing(format!("{} does not exist", p.display())))
    }
}

/// Parses the first line of `ffmpeg -version` / `ffprobe -version`.
pub fn parse_version(output: &str) -> Option<Version> {
    let line = output.lines().next()?;
    let rest = line.strip_prefix("ffmpeg version ").or_else(|| line.strip_prefix("ffprobe version "))?;
    let text = rest.split_whitespace().next()?.to_owned();
    let numeric = text.strip_prefix('n').unwrap_or(&text);
    let mut parts = numeric.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().and_then(|p| p.parse::<u32>().ok());
    // Release strings look like "6.1.1-3ubuntu5" or "n7.1"; git builds like "N-118000-g…"
    // or "2025-01-01-git-…" (a date, not a version).
    let looks_like_release = numeric
        .split_once('.')
        .is_some_and(|(maj, _)| !maj.is_empty() && maj.len() <= 2 && maj.bytes().all(|b| b.is_ascii_digit()));
    match (major, minor) {
        (Some(major), Some(minor)) if looks_like_release => Some(Version::Release { major, minor, text }),
        _ => Some(Version::Git { text }),
    }
}

/// A `std::process::Command` that never opens a console window on Windows.
pub(crate) fn quiet_command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        type Case<'a> = (&'a str, Option<(u32, u32)>, bool);
        let cases: &[Case<'_>] = &[
            ("ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023", Some((6, 1)), true),
            ("ffmpeg version n7.1 Copyright", Some((7, 1)), true),
            ("ffmpeg version n9.0.2-22-g46d8f462ee-20261005 Copyright", Some((9, 0)), true),
            ("ffmpeg version 4.4.2-0ubuntu0.22.04.1 Copyright", Some((4, 4)), false),
            ("ffmpeg version 5.1.6 Copyright", Some((5, 1)), false),
            ("ffmpeg version N-118000-gabcdef0 Copyright", None, true),
            ("ffmpeg version 2025-01-01-git-abc-full_build-www.gyan.dev", None, true),
            ("ffprobe version 6.0 Copyright", Some((6, 0)), true),
        ];
        for (line, release, supported) in cases {
            let v = parse_version(line).unwrap_or_else(|| panic!("no version for {line}"));
            match (release, &v) {
                (Some((ma, mi)), Version::Release { major, minor, .. }) => {
                    assert_eq!((ma, mi), (major, minor), "{line}")
                }
                (None, Version::Git { .. }) => {}
                _ => panic!("unexpected {v:?} for {line}"),
            }
            assert_eq!(v.is_supported(), *supported, "{line}");
        }
        assert_eq!(parse_version("not ffmpeg"), None);
    }

    #[test]
    fn explicit_path_must_exist() {
        let err = find_tool(Some(Path::new("/definitely/missing/ffmpeg")), "X", "x", "x", None);
        assert!(matches!(err, Err(Error::EngineMissing(_))));
    }

    #[test]
    fn bundled_beats_path() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join(format!("{BUNDLED_FFMPEG}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&bundled, b"").unwrap();
        let tool =
            find_tool(None, "RMEDIA_TEST_UNSET_VAR", BUNDLED_FFMPEG, "ffmpeg", Some(dir.path())).unwrap();
        assert_eq!(tool.source, ToolSource::Bundled);
        assert_eq!(tool.path, bundled);
    }
}
