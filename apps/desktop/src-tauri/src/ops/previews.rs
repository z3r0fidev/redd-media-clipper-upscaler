//! Source previews: playable proxies, frames and thumbnails, and the stills registered with the media server.

use super::*;

impl AppState {
    pub async fn preview_source(&self, path: &Path) -> OpResult<PreviewInfo> {
        let info = self.probe(path).await?;
        let direct = direct_playable(&info);
        let direct_url =
            if direct { Some(self.media.register(path).map_err(|e| e.to_string())?) } else { None };
        Ok(PreviewInfo { time_offset: preview::time_offset(&info).as_secs_f64(), direct_url, info })
    }

    pub async fn ensure_proxy(
        &self,
        path: &Path,
        codec: ProxyCodec,
        on_progress: &(dyn Fn(f64) + Send + Sync),
    ) -> OpResult<String> {
        let info = self.probe(path).await?;
        let (tools, _) = self.engine().await?;
        let proxy = preview::ensure_proxy(
            &tools,
            &info,
            &self.cache(),
            codec,
            on_progress,
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())?;
        self.media.register(&proxy).map_err(|e| e.to_string())
    }

    pub async fn frame_at(&self, path: &Path, t: f64, height: u32) -> OpResult<Vec<u8>> {
        let info = self.probe(path).await?;
        let (tools, _) = self.engine().await?;
        preview::frame_at(
            &tools,
            &info,
            Ts::from_secs_f64(t.max(0.0)),
            height.clamp(32, 1080),
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// Thumbnails as `data:` URLs (no file access from the webview needed).
    pub async fn thumbnails(&self, path: &Path, n: usize) -> OpResult<Vec<String>> {
        use base64::Engine as _;
        let info = self.probe(path).await?;
        let (tools, _) = self.engine().await?;
        let files = preview::thumbnails(&tools, &info, n, &self.cache(), &CancellationToken::new())
            .await
            .map_err(|e| e.to_string())?;
        files
            .iter()
            .map(|f| {
                let bytes = std::fs::read(f).map_err(|e| e.to_string())?;
                Ok(format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ))
            })
            .collect()
    }

    /// Serves a preview still, forgetting the oldest ones (their files are evicted too).
    pub(super) fn register_preview(&self, file: &Path) -> OpResult<String> {
        let url = self.media.register(file).map_err(|e| e.to_string())?;
        // Kept canonical: that is the key the media server registered it under.
        let file = std::fs::canonicalize(file).map_err(|e| e.to_string())?;
        let mut files = lock(&self.preview_files);
        if !files.contains(&file) {
            files.push_back(file);
        }
        while files.len() > MAX_PREVIEW_URLS {
            if let Some(old) = files.pop_front() {
                self.media.forget(&old);
            }
        }
        Ok(url)
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(Clone))]
pub struct PreviewInfo {
    pub info: MediaInfo,
    /// Present when the webview can likely play the source as is.
    pub direct_url: Option<String>,
    /// Seconds to subtract from the player's `currentTime` to get clip time.
    pub time_offset: f64,
}

/// Preview stills kept servable (the core keeps 48 files: 24 pairs).
pub(super) const MAX_PREVIEW_URLS: usize = 48;

/// Containers/codecs both WebView2 and WebKitGTK commonly play; anything else uses a proxy. The
/// frontend still falls back to a proxy if direct playback errors.
pub(super) fn direct_playable(m: &MediaInfo) -> bool {
    let v = m.first_video().map(|s| s.codec.as_str());
    let a: Vec<&str> = m
        .streams
        .iter()
        .filter(|s| s.kind == rmedia_core::probe::StreamKind::Audio)
        .map(|s| s.codec.as_str())
        .collect();
    match m.container.as_str() {
        "mp4" | "m4v" | "mov" => v == Some("h264") && a.iter().all(|c| *c == "aac"),
        "webm" => {
            matches!(v, Some("vp8" | "vp9" | "av1")) && a.iter().all(|c| matches!(*c, "opus" | "vorbis"))
        }
        _ => false,
    }
}
