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
#[command(
    name = "rmedia",
    version,
    about = "Cut clips from videos and make them phone-ready: upscaled and reframed for portrait screens."
)]
pub struct Cli {
    /// Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub ffmpeg: Option<PathBuf>,
    /// Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub ffprobe: Option<PathBuf>,
    /// Path to gifsicle, the optional GIF optimiser (default: the gifsicle_path setting,
    /// $RMEDIA_GIFSICLE, then bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub gifsicle: Option<PathBuf>,
    /// Path to gifski, the optional best-quality GIF engine (default: the gifski_path setting,
    /// $RMEDIA_GIFSKI, then bundled, then PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub gifski: Option<PathBuf>,
    /// More detail (show ffmpeg commands with -v).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Only print errors and the final summary.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Where to find the engines: the paths given on the command line, then the usual lookup.
    pub fn locate(&self) -> rmedia_core::tools::LocateOptions {
        rmedia_core::tools::LocateOptions {
            ffmpeg: self.ffmpeg.clone(),
            ffprobe: self.ffprobe.clone(),
            gifsicle: self.gifsicle.clone(),
            gifski: self.gifski.clone(),
            ..rmedia_core::tools::LocateOptions::default()
        }
    }
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
    /// Make GIFs for Reddit that fit a size target (default: a Reddit post, 720 px, 15 fps).
    Gif(Box<GifArgs>),
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
pub struct GifArgs {
    /// Input video files and/or folders (folders are scanned for videos).
    #[arg(value_name = "INPUT", required_unless_present_any = ["inputs_from", "list_targets"])]
    pub inputs: Vec<PathBuf>,
    /// Read more inputs from a file (one path per line), or "-" for stdin.
    #[arg(long, value_name = "FILE")]
    pub inputs_from: Option<String>,
    /// List the GIF targets and exit.
    #[arg(long)]
    pub list_targets: bool,
    #[command(flatten)]
    pub scan: ScanFlags,
    /// Range start, for every input: 90, 1:30, 00:01:30.500 … [default: the start]
    #[arg(short = 's', long, value_name = "TIME", allow_hyphen_values = true)]
    pub start: Option<String>,
    /// Range end (a time, or "end") [default: the end of the video].
    #[arg(short = 'e', long, value_name = "TIME", conflicts_with = "duration", allow_hyphen_values = true)]
    pub end: Option<String>,
    /// Range length instead of an end time: 15, +15, 1:00 …
    #[arg(short = 'd', long, value_name = "DURATION", allow_hyphen_values = true)]
    pub duration: Option<String>,
    #[command(flatten)]
    pub gif: GifFlags,
    #[command(flatten)]
    pub common: CommonArgs,
}

/// GIF options (`rmedia gif`). Unset flags come from the target.
#[derive(Debug, Args, Clone, Default)]
pub struct GifFlags {
    /// GIF target (see `gif --list-targets`) [default: reddit-post].
    #[arg(long, value_name = "ID")]
    pub target: Option<String>,
    /// Output format [default: gif].
    #[arg(long, value_enum)]
    pub format: Option<FormatArg>,
    /// Output width in pixels; never above the source's [default: the target's].
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(64..=1920))]
    pub width: Option<u32>,
    /// Frames per second, at most 50 for a GIF [default: the target's].
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=60))]
    pub fps: Option<u32>,
    /// Palette size, 2 to 256 (built-in engine) [default: the target's].
    #[arg(long, alias = "colours", value_name = "N", value_parser = clap::value_parser!(u16).range(2..=256))]
    pub colors: Option<u16>,
    /// Dithering: none, bayer, bayer:0..5, floyd-steinberg or sierra [default: the target's].
    #[arg(long, value_name = "MODE", value_parser = parse_dither)]
    pub dither: Option<rmedia_core::gif::target::Dither>,
    /// gifsicle lossy compression, 0 to 200 (needs gifsicle) [default: the target's].
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u16).range(0..=200))]
    pub lossy: Option<u16>,
    /// How many times it plays: forever, or 1 to 65535 [default: forever].
    #[arg(long = "loop", value_name = "N", value_parser = parse_loop)]
    pub loop_count: Option<rmedia_core::gif::LoopCount>,
    /// The size to aim for (3MB, 2.5MiB, 800k), or none [default: the target's].
    #[arg(long, value_name = "SIZE", value_parser = parse_budget)]
    pub budget: Option<BudgetArg>,
    /// Never write above this size [default: the budget, else the target's hard limit].
    #[arg(long, value_name = "SIZE", value_parser = parse_size)]
    pub hard_limit: Option<u64>,
    /// Allow shortening the range when no setting fits the budget.
    #[arg(long)]
    pub allow_trim: bool,
    /// GIF engine: the target's (auto), the built-in one or gifski [default: auto].
    #[arg(long, value_enum)]
    pub engine: Option<EngineArg>,
    /// Fail instead of falling back when the chosen engine is missing.
    #[arg(long)]
    pub strict_engine: bool,
    /// Never run the gifsicle optimiser.
    #[arg(long)]
    pub no_optimise: bool,
    /// Output shape [default: source].
    #[arg(long, value_enum)]
    pub shape: Option<ShapeArg>,
    /// How to reach another shape [default: smart].
    #[arg(long, value_enum)]
    pub framing: Option<ReframeArg>,
    /// With --dry-run, skip the sample encodes (the size estimate is shown as pending).
    /// Without it, a dry run encodes short samples of each input in a temporary folder.
    #[arg(long)]
    pub no_sample: bool,
}

impl GifFlags {
    /// The flags given, as one layer of overrides.
    pub fn partial(&self) -> rmedia_core::gif::PartialGif {
        use rmedia_core::gif::BudgetSpec;
        let budget = match (self.budget, self.hard_limit) {
            (Some(BudgetArg::Off), _) => Some(None),
            (Some(BudgetArg::Size(soft)), hard) => Some(Some(BudgetSpec { soft: Some(soft), hard })),
            (None, Some(hard)) => Some(Some(BudgetSpec { soft: None, hard: Some(hard) })),
            (None, None) => None,
        };
        rmedia_core::gif::PartialGif {
            enabled: None,
            target: self.target.clone(),
            format: self.format.map(Into::into),
            engine: self.engine.map(Into::into),
            strict_engine: self.strict_engine.then_some(true),
            optimise: self.no_optimise.then_some(false),
            loop_count: self.loop_count,
            shape: self.shape.map(Into::into),
            framing: self.framing.map(Into::into),
            width: self.width.map(Some),
            fps: self.fps.map(Some),
            colours: self.colors.map(Some),
            dither: self.dither.map(Some),
            lossy: self.lossy.map(Some),
            budget,
            allow_trim: self.allow_trim.then_some(true),
        }
    }

    /// The flags over the built-in defaults.
    pub fn options(&self) -> rmedia_core::gif::GifOptions {
        self.partial().apply(None).expect("a layer without `enabled: false` turns GIF export on")
    }

    /// A combination clap cannot check.
    pub fn usage_error(&self) -> Option<String> {
        if self.format != Some(FormatArg::Mp4)
            && let Some(fps) = self.fps.filter(|f| *f > 50)
        {
            return Some(format!("--fps {fps}: a GIF plays at most 50 fps (2 cs per frame)"));
        }
        if self.budget == Some(BudgetArg::Off) && self.hard_limit.is_some() {
            return Some("--hard-limit cannot be combined with --budget none".into());
        }
        None
    }
}

/// `--budget`: a size, or `none` (no budget search).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetArg {
    Off,
    Size(u64),
}

fn parse_size(s: &str) -> Result<u64, String> {
    rmedia_core::gif::parse_size(s)
}

fn parse_budget(s: &str) -> Result<BudgetArg, String> {
    if s.trim().eq_ignore_ascii_case("none") {
        Ok(BudgetArg::Off)
    } else {
        parse_size(s).map(BudgetArg::Size)
    }
}

fn parse_dither(s: &str) -> Result<rmedia_core::gif::target::Dither, String> {
    rmedia_core::gif::target::Dither::parse(s)
}

fn parse_loop(s: &str) -> Result<rmedia_core::gif::LoopCount, String> {
    use rmedia_core::gif::LoopCount;
    if s.trim().eq_ignore_ascii_case("forever") {
        return Ok(LoopCount::Forever);
    }
    match s.trim().parse::<u16>() {
        Ok(n @ 1..) => Ok(LoopCount::Plays(n)),
        _ => Err(format!("{s:?}: use forever, or a number of plays from 1 to 65535")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    Gif,
    Mp4,
}

impl From<FormatArg> for rmedia_core::gif::Format {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Gif => Self::Gif,
            FormatArg::Mp4 => Self::Mp4,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EngineArg {
    Auto,
    Builtin,
    Gifski,
}

impl From<EngineArg> for rmedia_core::gif::EngineChoice {
    fn from(e: EngineArg) -> Self {
        match e {
            EngineArg::Auto => Self::Auto,
            EngineArg::Builtin => Self::Builtin,
            EngineArg::Gifski => Self::Gifski,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShapeArg {
    Source,
    #[value(name = "9:16")]
    Tall,
    #[value(name = "4:5")]
    Feed,
    #[value(name = "1:1")]
    Square,
}

impl From<ShapeArg> for rmedia_core::gif::Shape {
    fn from(s: ShapeArg) -> Self {
        match s {
            ShapeArg::Source => Self::Source,
            ShapeArg::Tall => Self::Tall,
            ShapeArg::Feed => Self::Feed,
            ShapeArg::Square => Self::Square,
        }
    }
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
            Command::Gif(args) => commands::gif::run(&cli, args).await,
        }
    });
    ExitCode::from(code)
}
