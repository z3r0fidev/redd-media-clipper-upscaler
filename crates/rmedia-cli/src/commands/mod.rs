pub mod batch;
pub mod clip;
pub mod doctor;
pub mod scan;
pub mod upscale;

/// A cancel token that Ctrl-C triggers while it is alive (for steps before a run starts, such as
/// the GPU check).
pub struct CtrlC {
    pub token: tokio_util::sync::CancellationToken,
    watcher: tokio::task::JoinHandle<()>,
}

impl Drop for CtrlC {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

pub fn ctrl_c_token() -> CtrlC {
    let token = tokio_util::sync::CancellationToken::new();
    let on_ctrl_c = token.clone();
    let watcher = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_ctrl_c.cancel();
        }
    });
    CtrlC { token, watcher }
}

/// Runs the machine's GPU/tier check (stored, or measured once per hardware or app change)
/// when `req` upscales on a tier other than a forced CPU, so planning can choose the tier.
/// `Err` is the exit code (Ctrl-C).
pub async fn check_tiers(
    report: &mut rmedia_core::doctor::DoctorReport,
    engine: &mut (rmedia_core::tools::Tools, rmedia_core::plan::Caps),
    req: &rmedia_core::request::BatchRequest,
) -> Result<(), u8> {
    if !req.needs_capability_check() {
        return Ok(());
    }
    let ctrl_c = ctrl_c_token();
    rmedia_core::doctor::check_capability(
        report,
        Some(engine),
        rmedia_core::upscale::capability::CheckMode::Ensure,
        &ctrl_c.token,
    )
    .await;
    if ctrl_c.token.is_cancelled() { Err(crate::exit::CANCELLED) } else { Ok(()) }
}
