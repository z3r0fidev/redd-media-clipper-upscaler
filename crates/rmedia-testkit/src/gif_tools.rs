//! The optional GIF engines (gifsicle, gifski) for the tests that need them.

use super::*;

/// The optional GIF engines (feature 003), found through `RMEDIA_GIFSICLE` / `RMEDIA_GIFSKI`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifTool {
    Gifsicle,
    Gifski,
}

impl GifTool {
    pub(super) fn env(self) -> &'static str {
        match self {
            GifTool::Gifsicle => "RMEDIA_GIFSICLE",
            GifTool::Gifski => "RMEDIA_GIFSKI",
        }
    }
}

/// Path to an optional GIF engine, when its environment variable names a program that runs.
pub fn gif_tool(tool: GifTool) -> Option<PathBuf> {
    let path = std::env::var_os(tool.env()).filter(|v| !v.is_empty()).map(PathBuf::from)?;
    Command::new(&path).arg("--version").output().ok().filter(|o| o.status.success()).map(|_| path)
}

/// Whether a test needing `tool` can run. Panics when `RMEDIA_REQUIRE_GIF_TOOLS=1` and the tool
/// is missing, so the bundled-engine CI jobs never skip silently.
pub fn require_gif_tool(tool: GifTool) -> bool {
    if gif_tool(tool).is_some() {
        return true;
    }
    if std::env::var_os("RMEDIA_REQUIRE_GIF_TOOLS").is_some_and(|v| v == "1") {
        panic!(
            "{tool:?} is required (RMEDIA_REQUIRE_GIF_TOOLS=1) but {} is not set to a working program",
            tool.env()
        );
    }
    eprintln!("skipping: {tool:?} not available ({} unset)", tool.env());
    false
}
