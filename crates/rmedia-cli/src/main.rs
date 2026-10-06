//! `rmedia` command-line interface (contracts/cli.md). All media logic lives in rmedia-core.

mod commands;
mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rmedia_core::request::{Mode, OnExists};

/// Exit codes (contracts/cli.md).
pub mod exit {
    pub const OK: u8 = 0;
    pub const JOB_FAILED: u8 = 1;
    pub const INVALID_INPUT: u8 = 3;
    pub const ENGINE: u8 = 4;
    pub const CANCELLED: u8 = 130;
}

#[derive(Debug, Parser)]
#[command(name = "rmedia", version, about = "Cut clips from videos — fast (no re-encode) or frame-accurate.")]
pub struct Cli {
    /// Path to the ffmpeg executable (default: bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub ffmpeg: Option<PathBuf>,
    /// Path to the ffprobe executable (default: bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub ffprobe: Option<PathBuf>,
    /// More detail (show ffmpeg commands with -v).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Only print errors and the final summary.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Cut clips from each input video: one with -s/-e/-d, or many with -l LIST.
    Clip(Box<ClipArgs>),
    /// Find the videos in folders (recursively) and list them.
    Scan(ScanArgs),
    /// Show the video engine in use and its capabilities.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
pub struct ClipArgs {
    /// Input video files and/or folders (folders are scanned for videos).
    #[arg(value_name = "INPUT", required_unless_present = "inputs_from")]
    pub inputs: Vec<PathBuf>,
    /// Read more inputs from a file (one path per line), or "-" for stdin.
    #[arg(long, value_name = "FILE")]
    pub inputs_from: Option<String>,
    #[command(flatten)]
    pub scan: ScanFlags,
    /// Clip start: 90, 1:30, 00:01:30.500, 1m30s …
    #[arg(
        short = 's',
        long,
        value_name = "TIME",
        allow_hyphen_values = true,
        required_unless_present = "list"
    )]
    pub start: Option<String>,
    /// Clip list file: one clip per line, "START END|+DURATION [NAME]" (applied to every input).
    #[arg(short = 'l', long, value_name = "FILE", conflicts_with_all = ["start", "end", "duration"])]
    pub list: Option<PathBuf>,
    /// Clip end (a time, or "end"). Defaults to the end of the video.
    #[arg(short = 'e', long, value_name = "TIME", conflicts_with = "duration", allow_hyphen_values = true)]
    pub end: Option<String>,
    /// Clip length instead of an end time: 15, +15, 1:00 …
    #[arg(short = 'd', long, value_name = "DURATION", allow_hyphen_values = true)]
    pub duration: Option<String>,
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Debug, Args)]
pub struct CommonArgs {
    /// fast = no re-encode, starts at the keyframe before START; accurate = frame-exact re-encode.
    #[arg(long, value_enum, default_value_t = ModeArg::Fast)]
    pub mode: ModeArg,
    /// Output folder (default: a "clips" folder next to each source).
    #[arg(short = 'o', long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
    /// Output name template: {source} {index} {index:03} {name} {name-or-range} {name-or-start} {start} {end} {range} {ext}
    #[arg(long, value_name = "TEMPLATE")]
    pub name: Option<String>,
    /// Output container/extension (default: same as the source).
    #[arg(long, value_name = "EXT")]
    pub container: Option<String>,
    /// What to do when an output file already exists.
    #[arg(long, value_enum, default_value_t = OnExistsArg::Skip)]
    pub on_exists: OnExistsArg,
    /// Drop all audio.
    #[arg(long, conflicts_with = "audio")]
    pub no_audio: bool,
    /// Keep only these audio tracks (1-based), e.g. 1,2.
    #[arg(long, value_name = "TRACKS", value_delimiter = ',')]
    pub audio: Option<Vec<u32>>,
    /// Drop subtitles.
    #[arg(long)]
    pub no_subs: bool,
    /// How many clips to process at once (default: half the CPU cores, max 4).
    #[arg(short = 'j', long, value_name = "N", value_parser = clap::value_parser!(u16).range(1..=64))]
    pub jobs: Option<u16>,
    /// Show what would be done without writing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Machine-readable output on stdout (NDJSON events, or one document for --dry-run).
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Folders (or files) to scan.
    #[arg(required = true, value_name = "PATH")]
    pub roots: Vec<PathBuf>,
    #[command(flatten)]
    pub flags: ScanFlags,
    /// Print only the paths of found videos, one per line (pipe into `clip --inputs-from -`).
    #[arg(long, conflicts_with = "json")]
    pub paths: bool,
    /// Print the result as one JSON document.
    #[arg(long)]
    pub json: bool,
}

/// Options controlling how folders are walked (shared by `scan` and folder inputs of `clip`).
#[derive(Debug, Args, Clone)]
pub struct ScanFlags {
    /// Only look at the top level of each folder.
    #[arg(long)]
    pub no_recursive: bool,
    /// Maximum folder depth (1 = top level only).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u16).range(1..))]
    pub max_depth: Option<u16>,
    /// Only include matching paths (glob, relative to the folder; repeatable), e.g. "*.mp4".
    #[arg(long, value_name = "GLOB")]
    pub include: Vec<String>,
    /// Exclude matching paths (glob; repeatable), e.g. "**/drafts/**".
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
    /// Include hidden files and folders.
    #[arg(long)]
    pub hidden: bool,
    /// Follow symbolic links (loops are detected).
    #[arg(long)]
    pub follow_symlinks: bool,
}

impl ScanFlags {
    pub fn options(&self) -> rmedia_core::discover::ScanOptions {
        rmedia_core::discover::ScanOptions {
            no_recursive: self.no_recursive,
            max_depth: self.max_depth.map(usize::from),
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            hidden: self.hidden,
            follow_symlinks: self.follow_symlinks,
            ..Default::default()
        }
    }
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ModeArg {
    Fast,
    Accurate,
}

impl From<ModeArg> for Mode {
    fn from(m: ModeArg) -> Mode {
        match m {
            ModeArg::Fast => Mode::Fast,
            ModeArg::Accurate => Mode::Accurate,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OnExistsArg {
    Skip,
    Overwrite,
    Rename,
}

impl From<OnExistsArg> for OnExists {
    fn from(o: OnExistsArg) -> OnExists {
        match o {
            OnExistsArg::Skip => OnExists::Skip,
            OnExistsArg::Overwrite => OnExists::Overwrite,
            OnExistsArg::Rename => OnExists::Rename,
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => e.exit(), // 2 for usage errors, 0 for --help/--version
    };
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let code = runtime.block_on(async {
        match &cli.command {
            Command::Clip(args) => commands::clip::run(&cli, args).await,
            Command::Scan(args) => commands::scan::run(&cli, args).await,
            Command::Doctor(args) => commands::doctor::run(&cli, args),
        }
    });
    ExitCode::from(code)
}
