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
    /// Command-line usage errors (clap uses the same code).
    pub const USAGE: u8 = 2;
    pub const INVALID_INPUT: u8 = 3;
    pub const ENGINE: u8 = 4;
    pub const CANCELLED: u8 = 130;
}

#[derive(Debug, Parser)]
#[command(name = "rmedia", version, about = "Cut clips from videos — fast (no re-encode) or frame-accurate.")]
pub struct Cli {
    /// Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub ffmpeg: Option<PathBuf>,
    /// Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH).
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
    /// Make videos phone-ready: upscale and reframe to a device preset (default 1080x2340).
    Upscale(Box<UpscaleArgs>),
    /// Run a batch manifest (.json or .csv) listing many videos and their clips.
    Batch(BatchArgs),
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
    /// fast = no re-encode, starts at the keyframe before START; accurate = frame-exact re-encode.
    #[arg(long, value_enum, default_value_t = ModeArg::Fast)]
    pub mode: ModeArg,
    /// Also upscale every clip for phones, in the same pass (feature 002; see `rmedia upscale`).
    #[arg(long)]
    pub upscale: bool,
    #[command(flatten)]
    pub up: UpscaleFlags,
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Debug, Args)]
pub struct UpscaleArgs {
    /// Input video files and/or folders (folders are scanned for videos).
    #[arg(value_name = "INPUT", required_unless_present_any = ["inputs_from", "list_presets"])]
    pub inputs: Vec<PathBuf>,
    /// Read more inputs from a file (one path per line), or "-" for stdin.
    #[arg(long, value_name = "FILE")]
    pub inputs_from: Option<String>,
    /// List the device presets and exit.
    #[arg(long)]
    pub list_presets: bool,
    #[command(flatten)]
    pub scan: ScanFlags,
    #[command(flatten)]
    pub upscale: UpscaleFlags,
    /// With smart reframe, write the full crop path as JSON to FILE.
    #[arg(long, value_name = "FILE")]
    pub crop_path: Option<PathBuf>,
    /// With --dry-run, skip the smart-reframe analysis (the crop path is shown as pending).
    #[arg(long)]
    pub no_analyze: bool,
    #[command(flatten)]
    pub common: CommonArgs,
}

/// Upscale options (`rmedia upscale`, `clip --upscale`, `batch`). Unset flags are inherited:
/// from the built-in defaults, or under `batch` from the manifest.
#[derive(Debug, Args, Clone, Default)]
pub struct UpscaleFlags {
    /// Device preset (see `upscale --list-presets`) [default: phone-fill].
    #[arg(long, value_name = "ID")]
    pub preset: Option<String>,
    /// How to fill a screen of a different shape: follow the subject, crop the centre, or show
    /// the whole frame over a blurred background [default: smart].
    #[arg(long, value_enum)]
    pub reframe: Option<ReframeArg>,
    /// Quality tier: chosen for this machine (auto) or forced [default: auto].
    #[arg(long, value_enum)]
    pub tier: Option<TierArg>,
    /// HDR sources: convert to standard range (sdr) or keep HDR (HDR-capable presets only)
    /// [default: sdr].
    #[arg(long, value_enum)]
    pub hdr: Option<HdrArg>,
    /// Smart-reframe camera speed [default: normal].
    #[arg(long, value_enum)]
    pub motion: Option<MotionArg>,
    /// Override the preset's quality (CRF, lower = better and bigger).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=51))]
    pub crf: Option<u8>,
}

impl UpscaleFlags {
    /// The flags given, as one layer of overrides.
    pub fn partial(&self) -> rmedia_core::upscale::PartialUpscale {
        rmedia_core::upscale::PartialUpscale {
            enabled: None,
            preset: self.preset.clone(),
            reframe: self.reframe.map(Into::into),
            tier: self.tier.map(Into::into),
            hdr: self.hdr.map(Into::into),
            motion: self.motion.map(Into::into),
            crf: self.crf.map(Some),
        }
    }

    /// The flags over the built-in defaults.
    pub fn options(&self) -> rmedia_core::upscale::UpscaleOptions {
        self.partial().apply(None).expect("a layer without `enabled: false` turns upscaling on")
    }

    /// Whether any upscale flag was given.
    pub fn any(&self) -> bool {
        self.partial() != rmedia_core::upscale::PartialUpscale::default()
    }

    /// The flag names given, for usage errors.
    pub fn given(&self) -> Vec<&'static str> {
        [
            ("--preset", self.preset.is_some()),
            ("--reframe", self.reframe.is_some()),
            ("--tier", self.tier.is_some()),
            ("--hdr", self.hdr.is_some()),
            ("--motion", self.motion.is_some()),
            ("--crf", self.crf.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, set)| set.then_some(n))
        .collect()
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReframeArg {
    Smart,
    Center,
    Blur,
}

impl From<ReframeArg> for rmedia_core::upscale::Reframe {
    fn from(r: ReframeArg) -> Self {
        match r {
            ReframeArg::Smart => Self::Smart,
            ReframeArg::Center => Self::Center,
            ReframeArg::Blur => Self::Blur,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TierArg {
    Auto,
    Cpu,
    Gpu,
    Ai,
}

impl From<TierArg> for rmedia_core::upscale::TierChoice {
    fn from(t: TierArg) -> Self {
        match t {
            TierArg::Auto => Self::Auto,
            TierArg::Cpu => Self::Cpu,
            TierArg::Gpu => Self::Gpu,
            TierArg::Ai => Self::Ai,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HdrArg {
    Sdr,
    Keep,
}

impl From<HdrArg> for rmedia_core::upscale::HdrMode {
    fn from(h: HdrArg) -> Self {
        match h {
            HdrArg::Sdr => Self::Sdr,
            HdrArg::Keep => Self::Keep,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum MotionArg {
    Calm,
    Normal,
    Fast,
}

impl From<MotionArg> for rmedia_core::upscale::Motion {
    fn from(m: MotionArg) -> Self {
        match m {
            MotionArg::Calm => Self::Calm,
            MotionArg::Normal => Self::Normal,
            MotionArg::Fast => Self::Fast,
        }
    }
}

#[derive(Debug, Args)]
pub struct CommonArgs {
    /// Output folder (default: a "clips" folder next to each source; "upscaled" for upscale).
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
pub struct BatchArgs {
    /// Manifest file: .json (structured) or .csv (one row per clip).
    #[arg(value_name = "MANIFEST")]
    pub manifest: PathBuf,
    /// Skip videos that do not exist instead of stopping before anything runs.
    #[arg(long)]
    pub continue_on_missing: bool,
    /// Default mode for clips the manifest does not specify.
    #[arg(long, value_enum)]
    pub mode: Option<ModeArg>,
    /// Default output folder for clips the manifest does not specify.
    #[arg(short = 'o', long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
    /// Default name template.
    #[arg(long, value_name = "TEMPLATE")]
    pub name: Option<String>,
    /// Default output container/extension.
    #[arg(long, value_name = "EXT")]
    pub container: Option<String>,
    /// What to do when an output file already exists (default: skip).
    #[arg(long, value_enum)]
    pub on_exists: Option<OnExistsArg>,
    /// How many clips to process at once.
    #[arg(short = 'j', long, value_name = "N", value_parser = clap::value_parser!(u16).range(1..=64))]
    pub jobs: Option<u16>,
    /// Upscale every video the manifest does not turn upscaling off for (feature 002).
    #[arg(long)]
    pub upscale: bool,
    /// Upscale settings for the videos that are upscaled, under the manifest's own values.
    #[command(flatten)]
    pub up: UpscaleFlags,
    /// Show what would be done without writing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Machine-readable output on stdout.
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
    /// Run the GPU self-tests again now (normally they run only when the hardware, driver or
    /// app version changes).
    #[arg(long)]
    pub recheck: bool,
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
            Command::Upscale(args) => commands::upscale::run(&cli, args).await,
            Command::Batch(args) => commands::batch::run(&cli, args).await,
            Command::Scan(args) => commands::scan::run(&cli, args).await,
            Command::Doctor(args) => commands::doctor::run(&cli, args).await,
        }
    });
    ExitCode::from(code)
}
