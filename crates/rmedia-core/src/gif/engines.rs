//! Locating the optional engines gifsicle and gifski (research R7, FR-021, FR-024).
//!
//! Both are optional: when one is missing, the job uses the built-in engine (gifski) or skips
//! the post-pass (gifsicle), and `doctor` says so. Lookup order: explicit setting or flag →
//! `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI` → bundled `rmedia-gifsicle`/`rmedia-gifski` beside rmedia →
//! PATH, only when no bundled copy exists. A configured path that is missing, or a program that
//! does not identify itself, makes the engine unavailable with a reason (never a silent fall
//! through to PATH). A program found on PATH inside the current directory, the job's scratch
//! folder or an input folder is refused (a planted binary). Every check runs the program with a
//! cleared environment, a 5 s timeout and bounded output.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::tools::{Tool, ToolSource};

/// How long `--version` may take.
const BANNER_TIMEOUT: Duration = Duration::from_secs(5);
/// The most of a `--version` banner that is read.
const BANNER_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifEngineKind"))]
pub enum EngineKind {
    Gifsicle,
    Gifski,
}

impl EngineKind {
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::Gifsicle => "gifsicle",
            EngineKind::Gifski => "gifski",
        }
    }

    pub fn env_var(self) -> &'static str {
        match self {
            EngineKind::Gifsicle => "RMEDIA_GIFSICLE",
            EngineKind::Gifski => "RMEDIA_GIFSKI",
        }
    }

    pub fn bundled_name(self) -> &'static str {
        match self {
            EngineKind::Gifsicle => "rmedia-gifsicle",
            EngineKind::Gifski => "rmedia-gifski",
        }
    }

    /// The version in a `--version` banner: `LCDF Gifsicle 1.98` → `1.98`, `gifski 1.34.0` →
    /// `1.34.0`. Only major version 1 is accepted.
    pub fn parse_banner(self, banner: &str) -> Option<String> {
        let first = banner.lines().next()?.trim();
        let rest = match self {
            EngineKind::Gifsicle => first.strip_prefix("LCDF Gifsicle ")?,
            EngineKind::Gifski => first.strip_prefix("gifski ")?,
        };
        let version = rest.split_whitespace().next()?;
        let parts: Vec<&str> = version.split('.').collect();
        let numeric = parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        let shape = match self {
            EngineKind::Gifsicle => parts.len() == 2,
            EngineKind::Gifski => parts.len() == 3,
        };
        (numeric && shape && parts[0] == "1").then(|| version.to_owned())
    }
}

/// What `doctor` and the planner know about one engine (data-model.md › GifEngine status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "GifEngineStatus"))]
pub struct EngineStatus {
    pub name: EngineKind,
    #[serde(serialize_with = "crate::pathser::opt_path")]
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub path: Option<PathBuf>,
    pub source: Option<ToolSource>,
    pub version: Option<String>,
    pub ok: bool,
    pub reason: Option<String>,
}

impl EngineStatus {
    fn unavailable(
        name: EngineKind,
        path: Option<PathBuf>,
        source: Option<ToolSource>,
        reason: String,
    ) -> Self {
        EngineStatus { name, path, source, version: None, ok: false, reason: Some(reason) }
    }

    /// The program to run, when the engine is available.
    pub fn program(&self) -> Option<&Path> {
        self.path.as_deref().filter(|_| self.ok)
    }
}

/// Both optional engines, as found for this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engines {
    pub gifsicle: EngineStatus,
    pub gifski: EngineStatus,
}

impl Default for Engines {
    /// Neither engine: what the planner assumes when nothing was looked up.
    fn default() -> Self {
        let none = |name| EngineStatus::unavailable(name, None, None, "not looked up".into());
        Engines { gifsicle: none(EngineKind::Gifsicle), gifski: none(EngineKind::Gifski) }
    }
}

/// Where to look.
#[derive(Debug, Clone, Default)]
pub struct EngineLookup {
    pub gifsicle: Option<PathBuf>,
    pub gifski: Option<PathBuf>,
    /// Directory searched for the bundled copies; defaults to the current executable's.
    pub bundled_dir: Option<PathBuf>,
}

impl Engines {
    /// Finds both engines (environment variables included).
    pub fn locate(opts: &EngineLookup) -> Engines {
        let bundled_dir = opts
            .bundled_dir
            .clone()
            .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)));
        let env = |k: EngineKind| std::env::var_os(k.env_var()).filter(|v| !v.is_empty()).map(PathBuf::from);
        let refused: Vec<PathBuf> = std::env::current_dir().into_iter().collect();
        Engines {
            gifsicle: locate(
                EngineKind::Gifsicle,
                opts.gifsicle.as_deref(),
                env(EngineKind::Gifsicle).as_deref(),
                bundled_dir.as_deref(),
                &refused,
            ),
            gifski: locate(
                EngineKind::Gifski,
                opts.gifski.as_deref(),
                env(EngineKind::Gifski).as_deref(),
                bundled_dir.as_deref(),
                &refused,
            ),
        }
    }

    /// Refuses PATH-found engines inside any of `dirs` (a job's input folders and scratch).
    pub fn refusing(mut self, dirs: &[PathBuf]) -> Engines {
        for s in [&mut self.gifsicle, &mut self.gifski] {
            if s.ok
                && s.source == Some(ToolSource::Path)
                && let Some(p) = s.path.clone()
                && let Some(dir) = refused_by(&p, dirs)
            {
                *s = EngineStatus::unavailable(
                    s.name,
                    Some(p),
                    s.source,
                    format!("found on PATH inside {}, which is not trusted", dir.display()),
                );
            }
        }
        self
    }
}

/// Finds one engine and checks that it runs and identifies itself.
pub fn locate(
    kind: EngineKind,
    explicit: Option<&Path>,
    env: Option<&Path>,
    bundled_dir: Option<&Path>,
    refused: &[PathBuf],
) -> EngineStatus {
    let found = crate::tools::find_optional(explicit, env, kind.bundled_name(), kind.name(), bundled_dir);
    let Tool { path, source } = match found {
        None => {
            return EngineStatus::unavailable(
                kind,
                None,
                None,
                "not found (not bundled, not on PATH)".into(),
            );
        }
        Some(Err(reason)) => return EngineStatus::unavailable(kind, None, Some(ToolSource::Config), reason),
        Some(Ok(t)) => t,
    };
    // Absolute and resolved: a later change of working directory never changes what runs.
    let path = crate::paths::canonicalize(&path).unwrap_or(path);
    if source == ToolSource::Path
        && let Some(dir) = refused_by(&path, refused)
    {
        return EngineStatus::unavailable(
            kind,
            Some(path),
            Some(source),
            format!("found on PATH inside {}, which is not trusted", dir.display()),
        );
    }
    match banner(&path, BANNER_TIMEOUT).map(|b| kind.parse_banner(&b)) {
        Ok(Some(version)) => EngineStatus {
            name: kind,
            path: Some(path),
            source: Some(source),
            version: Some(version),
            ok: true,
            reason: None,
        },
        Ok(None) => EngineStatus::unavailable(
            kind,
            Some(path),
            Some(source),
            format!("did not identify itself as {} 1.x", kind.name()),
        ),
        Err(reason) => EngineStatus::unavailable(kind, Some(path), Some(source), reason),
    }
}

/// The first of `dirs` that contains `path` (both canonicalised where possible).
fn refused_by(path: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    let path = crate::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    dirs.iter()
        .map(|d| crate::paths::canonicalize(d).unwrap_or_else(|_| d.clone()))
        .find(|d| path.starts_with(d))
}

/// The environment the engines run with: nothing but what the OS needs (research R7).
pub(crate) fn engine_env() -> Vec<(OsString, OsString)> {
    let keep: &[&str] = if cfg!(windows) { &["SystemRoot", "TEMP", "TMP", "LANG"] } else { &["LANG"] };
    keep.iter().filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v))).collect()
}

/// Runs `path --version` with a cleared environment, returning at most 4 KiB of its stdout.
fn banner(path: &Path, timeout: Duration) -> Result<String, String> {
    let mut cmd = crate::tools::quiet_command(path);
    cmd.arg("--version").env_clear().envs(engine_env());
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    // A program written moments ago can be briefly busy (another thread forked while it was
    // open for writing): retry that case a few times.
    let mut tries = 0;
    let mut child = loop {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 5 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            r => break r.map_err(|e| format!("cannot run {}: {e}", path.display()))?,
        }
    };
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.take(BANNER_BYTES).read_to_end(&mut text);
        let _ = tx.send(text);
    });
    let text = match rx.recv_timeout(timeout) {
        Ok(t) => t,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} --version did not finish within {} s",
                path.display(),
                timeout.as_secs()
            ));
        }
    };
    // The banner is read; the program may still be writing (bounded read): stop it.
    let _ = child.kill();
    let _ = child.wait();
    Ok(String::from_utf8_lossy(&text).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banners_are_recognised() {
        let g = EngineKind::Gifsicle;
        assert_eq!(
            g.parse_banner("LCDF Gifsicle 1.98\nCopyright (C) 1997-2025 Eddie Kohler\n").as_deref(),
            Some("1.98")
        );
        assert_eq!(g.parse_banner("LCDF Gifsicle 2.0"), None, "only major version 1");
        assert_eq!(g.parse_banner("gifsicle 1.98"), None);
        let s = EngineKind::Gifski;
        assert_eq!(s.parse_banner("gifski 1.34.0").as_deref(), Some("1.34.0"));
        assert_eq!(s.parse_banner("gifski 1.34"), None);
        assert_eq!(s.parse_banner("gifski 1.x.0"), None);
        assert_eq!(s.parse_banner(""), None);
    }

    #[test]
    fn a_missing_configured_path_is_unavailable_with_a_reason() {
        let st =
            locate(EngineKind::Gifsicle, Some(Path::new("/definitely/missing/gifsicle")), None, None, &[]);
        assert!(!st.ok);
        assert!(st.reason.as_deref().unwrap().contains("does not exist"), "{st:?}");
        assert_eq!(st.source, Some(ToolSource::Config));
        let env = locate(EngineKind::Gifski, None, Some(Path::new("/definitely/missing/gifski")), None, &[]);
        assert!(!env.ok && env.reason.as_deref().unwrap().contains("does not exist"), "{env:?}");
        assert_eq!(env.program(), None);
    }

    #[test]
    fn path_results_inside_untrusted_folders_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("in/gifsicle");
        std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
        std::fs::write(&inner, b"").unwrap();
        assert_eq!(refused_by(&inner, &[dir.path().join("in")]).map(|d| d.ends_with("in")), Some(true));
        assert_eq!(refused_by(&inner, &[dir.path().join("other")]), None);
        let status = EngineStatus {
            name: EngineKind::Gifsicle,
            path: Some(inner.clone()),
            source: Some(ToolSource::Path),
            version: Some("1.98".into()),
            ok: true,
            reason: None,
        };
        let engines = Engines { gifsicle: status.clone(), gifski: Engines::default().gifski };
        let refused = engines.clone().refusing(&[dir.path().to_path_buf()]);
        assert!(!refused.gifsicle.ok && refused.gifsicle.reason.unwrap().contains("not trusted"));
        let bundled =
            Engines { gifsicle: EngineStatus { source: Some(ToolSource::Bundled), ..status }, ..engines };
        assert!(bundled.refusing(&[dir.path().to_path_buf()]).gifsicle.ok, "only PATH results are refused");
    }

    #[cfg(unix)]
    mod fake {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;

        /// A small test program (a shell script) at `dir/name`.
        fn program(dir: &Path, name: &str, body: &str) -> PathBuf {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn explicit_beats_env_beats_bundled() {
            let dir = tempfile::tempdir().unwrap();
            let explicit = program(dir.path(), "explicit", "echo 'LCDF Gifsicle 1.91'");
            let env = program(dir.path(), "env", "echo 'LCDF Gifsicle 1.92'");
            program(dir.path(), "rmedia-gifsicle", "echo 'LCDF Gifsicle 1.98'");
            let k = EngineKind::Gifsicle;
            let v = |s: EngineStatus| (s.version, s.source);
            assert_eq!(
                v(locate(k, Some(&explicit), Some(&env), Some(dir.path()), &[])),
                (Some("1.91".into()), Some(ToolSource::Config))
            );
            assert_eq!(
                v(locate(k, None, Some(&env), Some(dir.path()), &[])),
                (Some("1.92".into()), Some(ToolSource::Config))
            );
            assert_eq!(
                v(locate(k, None, None, Some(dir.path()), &[])),
                (Some("1.98".into()), Some(ToolSource::Bundled))
            );
            let st = locate(k, None, None, Some(dir.path()), &[]);
            assert!(st.ok && st.path.unwrap().is_absolute());
        }

        #[test]
        fn a_program_that_does_not_identify_itself_is_unavailable() {
            let dir = tempfile::tempdir().unwrap();
            let liar = program(dir.path(), "rmedia-gifski", "echo 'definitely not gifski'");
            let st = locate(EngineKind::Gifski, None, None, Some(dir.path()), &[]);
            assert!(!st.ok);
            assert!(st.reason.as_deref().unwrap().contains("did not identify itself as gifski"), "{st:?}");
            assert_eq!(st.path.as_deref(), crate::paths::canonicalize(&liar).ok().as_deref());
            let failing = program(dir.path(), "fails", "exit 3");
            assert!(!locate(EngineKind::Gifski, Some(&failing), None, None, &[]).ok);
        }

        #[test]
        fn the_banner_check_is_bounded_in_time_and_size() {
            let dir = tempfile::tempdir().unwrap();
            let slow = program(dir.path(), "slow", "sleep 10; echo 'gifski 1.34.0'");
            let started = std::time::Instant::now();
            let err = banner(&slow, Duration::from_millis(300)).unwrap_err();
            assert!(err.contains("did not finish"), "{err}");
            assert!(started.elapsed() < Duration::from_secs(5));
            let chatty = program(dir.path(), "chatty", "echo 'gifski 1.34.0'; yes rmedia | head -c 10000000");
            let text = banner(&chatty, Duration::from_secs(5)).unwrap();
            assert!(text.len() as u64 <= BANNER_BYTES);
            assert_eq!(EngineKind::Gifski.parse_banner(&text).as_deref(), Some("1.34.0"));
        }

        #[test]
        fn engines_run_with_a_cleared_environment() {
            let dir = tempfile::tempdir().unwrap();
            let envdump = program(dir.path(), "envdump", "env");
            let text = banner(&envdump, Duration::from_secs(5)).unwrap();
            for line in text.lines().filter(|l| l.contains('=')) {
                let key = line.split('=').next().unwrap();
                assert!(["LANG", "PWD", "SHLVL", "_"].contains(&key), "leaked {key}: {text}");
            }
            assert!(!text.contains("HOME="), "{text}");
        }
    }
}
