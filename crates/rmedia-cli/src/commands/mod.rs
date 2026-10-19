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
