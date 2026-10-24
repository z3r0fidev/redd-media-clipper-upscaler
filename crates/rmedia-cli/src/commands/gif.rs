//! `rmedia gif` (feature 003, contracts/cli.md): make GIFs (or Reddit MP4s) that fit a target.

use crate::{Cli, GifArgs, exit};

pub async fn run(_cli: &Cli, _args: &GifArgs) -> u8 {
    eprintln!("error: rmedia gif is not available yet");
    exit::USAGE
}
