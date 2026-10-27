pub mod batch;
pub mod clip;
pub mod doctor;
pub mod gif;
pub mod scan;
pub mod upscale;

static INTERRUPT: std::sync::OnceLock<tokio_util::sync::CancellationToken> = std::sync::OnceLock::new();
/// The run's own partial outputs, removed when a second Ctrl-C quits at once.
static PARTIALS: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Catches Ctrl-C for a command that runs jobs, from its start: scanning, the GPU check,
/// planning, samples and the run all watch [`interrupted`]. Once tokio listens for Ctrl-C the
/// default action is gone for the whole process, so one listener covers every step. The first
/// press cancels; the second removes the run's partial outputs and exits 130 at once.
pub fn catch_ctrl_c() {
    #[cfg(unix)]
    let presses = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());
    #[cfg(windows)]
    let presses = tokio::signal::windows::ctrl_c();
    let Ok(mut presses) = presses else {
        return; // Ctrl-C keeps its default action: the process ends, children with it
    };
    let token = INTERRUPT.get_or_init(tokio_util::sync::CancellationToken::new).clone();
    tokio::spawn(async move {
        presses.recv().await;
        eprintln!("\ncancelling… (press Ctrl-C again to quit immediately)");
        token.cancel();
        presses.recv().await;
        // Children die with us: process group + PDEATHSIG (Linux) / Job Object (Windows).
        // Best-effort removal of partial outputs first; anything still locked (Windows) is
        // purged by the next run via the crash journal.
        for t in PARTIALS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
            // Only our own temp outputs (`.<stem>.rmedia-<run>-<job>.part|.d`).
            if t.file_name().is_some_and(|n| n.to_string_lossy().contains(".rmedia-")) {
                let _ = rmedia_core::exec::commit::remove_now(t);
            }
        }
        std::process::exit(i32::from(crate::exit::CANCELLED));
    });
}

/// The command's Ctrl-C token (see [`catch_ctrl_c`]); never cancelled when it is not caught.
pub fn interrupted() -> tokio_util::sync::CancellationToken {
    INTERRUPT.get().cloned().unwrap_or_default()
}

/// Registers the partial outputs a second Ctrl-C removes.
pub fn set_partials(temps: Vec<std::path::PathBuf>) {
    *PARTIALS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = temps;
}

/// A cancel token that Ctrl-C triggers while it is alive (for steps before a run starts, such as
/// the GPU check): the command's own when it catches Ctrl-C, otherwise a watcher of its own.
pub struct CtrlC {
    pub token: tokio_util::sync::CancellationToken,
    watcher: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for CtrlC {
    fn drop(&mut self) {
        if let Some(w) = &self.watcher {
            w.abort();
        }
    }
}

pub fn ctrl_c_token() -> CtrlC {
    if let Some(token) = INTERRUPT.get() {
        return CtrlC { token: token.clone(), watcher: None };
    }
    let token = tokio_util::sync::CancellationToken::new();
    let on_ctrl_c = token.clone();
    let watcher = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_ctrl_c.cancel();
        }
    });
    CtrlC { token, watcher: Some(watcher) }
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
