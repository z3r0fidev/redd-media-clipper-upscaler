//! The planner: one batch request becomes jobs (sources probed once, clips resolved, names and collisions settled, every feature's job spec made).

use super::*;

pub(super) struct Planner<'a> {
    pub(super) tools: &'a Tools,
    pub(super) caps: &'a Caps,
    pub(super) cancel: &'a CancellationToken,
    pub(super) run_id: String,
    pub(super) jobs: Vec<PlannedJob>,
    pub(super) issues: Vec<Issue>,
    /// Outputs already assigned in this plan (normalized keys).
    pub(super) claimed: HashSet<std::ffi::OsString>,
    /// Canonical source paths (normalized keys); outputs may never equal one.
    pub(super) sources: HashSet<std::ffi::OsString>,
    pub(super) probes: HashMap<PathBuf, Result<MediaInfo, String>>,
    pub(super) presets: PresetRegistry,
    pub(super) targets: crate::gif::target::GifTargetRegistry,
}

impl Planner<'_> {
    pub(super) fn error(&mut self, origin: &Origin, code: IssueCode, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            code,
            origin: origin.clone(),
            message: message.into(),
            detail: None,
        });
    }

    pub(super) async fn probe(&mut self, path: &Path) -> Result<MediaInfo, String> {
        if let Some(r) = self.probes.get(path) {
            return r.clone();
        }
        let r = match probe::probe(self.tools, path, self.cancel).await {
            Ok(m) if m.is_video() => Ok(m),
            Ok(_) => Err(format!("{} has no video stream", path.display())),
            Err(Error::SourceMissing(p)) => Err(format!("source not found: {}", p.display())),
            Err(Error::Cancelled) => Err("cancelled".into()),
            Err(e) => Err(e.to_string()),
        };
        self.probes.insert(path.to_path_buf(), r.clone());
        r
    }

    pub(super) async fn plan_source(&mut self, req: &BatchRequest, source: &SourceRequest) {
        if probe::is_playlist(&source.path) {
            return self.error(
                &source.origin,
                IssueCode::InvalidInput,
                format!("{} is a playlist/stream description, not a video file", source.path.display()),
            );
        }
        let media = match self.probe(&source.path).await {
            Ok(m) => m,
            Err(msg) => {
                let code =
                    if source.path.exists() { IssueCode::ProbeFailed } else { IssueCode::SourceMissing };
                self.error(&source.origin, code, msg);
                return;
            }
        };
        let base = source.overrides.apply(&req.defaults);
        for (i, clip) in source.clips.iter().enumerate() {
            if self.cancel.is_cancelled() {
                return;
            }
            let opts = clip.overrides.apply(&base);
            self.plan_clip(source, &media, i + 1, clip, &opts).await;
        }
    }

    pub(super) async fn plan_clip(
        &mut self,
        source: &SourceRequest,
        media: &MediaInfo,
        index: usize,
        clip: &ClipRequest,
        opts: &crate::request::Options,
    ) {
        let origin = &clip.origin;
        let mut warnings = Vec::new();

        // --- upscaling (feature 002) ---------------------------------------------------------
        let up = match &opts.upscale {
            Some(u) => match self.check_upscale(origin, u, media) {
                Some((preset, tier)) => Some((u, preset, tier)),
                None => return,
            },
            None => None,
        };
        // --- GIF export (feature 003): reframes and downscales only, so no upscale tier runs
        // (research R16); the chained `--gif --upscale` job arrives with US5.
        let gif = opts.gif.as_ref();
        let up = match (gif, up) {
            (Some(_), Some(_)) => {
                warnings.push("upscale skipped: a GIF is never larger than its source".into());
                None
            }
            (_, up) => up,
        };
        // --- fit (feature 005): an MP4 with audio within a video target's caps -----------------
        let fit = match &opts.fit {
            Some(_) if gif.is_some() => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    "a clip cannot be both fitted to a video target and made a GIF (use --fit or --gif)",
                );
            }
            Some(_) if up.as_ref().is_some_and(|(u, _, _)| u.hdr == crate::upscale::HdrMode::Keep) => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    "a fitted video is SDR H.264: --hdr keep cannot be used with --fit",
                );
            }
            Some(f) => match self.caps.video_targets.get(&f.target) {
                Some(t) => Some((f, t.clone())),
                None => {
                    let known = self.caps.video_targets.ids().join(", ");
                    return self.error(
                        origin,
                        IssueCode::TargetUnknown,
                        format!("unknown video target {:?} (known: {known})", f.target),
                    );
                }
            },
            None => None,
        };
        let whole = clip.start == Ts::ZERO && clip.end == EndSpec::EndOfMedia;
        // Named like a whole-video upscale unless the user named the clip (names stay distinct).
        let whole_video = whole && clip.name.is_none();
        // Upscaling and GIF export always re-encode, so they cut like accurate mode (and `mode`
        // is ignored).
        let mode = if up.is_some() || gif.is_some() || fit.is_some() { Mode::Accurate } else { opts.mode };
        let mut notices = Vec::new();
        let mut notice = |warnings: &mut Vec<String>, code, message: String, detail| {
            warnings.push(message.clone());
            notices.push(Notice { code, message, detail });
        };
        if opts.mode == Mode::Smart && mode == Mode::Accurate {
            let message = if fit.is_some() {
                "smart cut does not apply: the clip is re-encoded to fit its target".to_owned()
            } else {
                "smart cut does not apply: the clip is re-encoded for upscaling/GIF".to_owned()
            };
            notice(&mut warnings, IssueCode::SmartNotApplied, message, None);
        }
        // A smart job is planned as an accurate one first (its other streams, and the fallback,
        // are cut exactly as accurate mode cuts them); the smart plan is added below (004).
        let smart_requested = mode == Mode::Smart;
        let mode = if smart_requested { Mode::Accurate } else { mode };

        // --- times ---------------------------------------------------------------------------
        let duration = media.duration;
        if let Some(d) = duration
            && clip.start >= d
        {
            return self.error(
                origin,
                IssueCode::InvalidInput,
                format!("start ({}) is at or beyond the end of the video ({})", clip.start, d),
            );
        }
        let mut end = match clip.end {
            EndSpec::At(t) => Some(t),
            EndSpec::Duration(d) if d == Ts::ZERO => {
                return self.error(origin, IssueCode::InvalidInput, "duration must be greater than zero");
            }
            EndSpec::Duration(d) => Some(clip.start + d),
            EndSpec::EndOfMedia => None,
        };
        if let Some(e) = end
            && e <= clip.start
        {
            return self.error(
                origin,
                IssueCode::InvalidInput,
                format!("end ({e}) is not after start ({})", clip.start),
            );
        }
        if let (Some(e), Some(d)) = (end, duration)
            && e > d
        {
            warnings.push(format!("end {e} is past the end of the video; clip ends at {d}"));
            end = None;
        }
        let requested_end = match (end, duration) {
            (Some(e), _) => e,
            (None, Some(d)) => d,
            (None, None) => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    "the video's duration is unknown; give an explicit end time",
                );
            }
        };

        // --- container & encoding ------------------------------------------------------------
        let ext = opts
            .container
            .clone()
            .map(|c| c.trim_start_matches('.').to_ascii_lowercase())
            .unwrap_or_else(|| media.container.clone());
        if fit.is_some()
            && let Some(c) = opts.container.as_deref().map(|c| c.trim_start_matches('.'))
            && !c.eq_ignore_ascii_case("mp4")
        {
            return self.error(
                origin,
                IssueCode::InvalidInput,
                format!("a fitted clip is always .mp4 (container .{c} was asked for)"),
            );
        }
        let ext = match (&up, &opts.container) {
            (None, _) if fit.is_some() => "mp4".to_owned(),
            (None, _) => ext,
            (Some(_), Some(c)) if !c.trim_start_matches('.').eq_ignore_ascii_case("mp4") => {
                warnings.push(format!("upscaled outputs are always .mp4 (ignoring container .{ext})"));
                "mp4".to_owned()
            }
            (Some(_), _) => "mp4".to_owned(),
        };
        let (mut container, mut out_ext) = match (gif, Container::from_ext(&ext)) {
            // GIF jobs: `format` decides the file (the container key is ignored); the streams
            // are selected as for a video-only MP4.
            (Some(g), _) => (Container::Mp4, g.format.ext().to_owned()),
            (None, Some(c)) => (c, ext.clone()),
            (None, None) if opts.container.is_some() => {
                return self.error(
                    origin,
                    IssueCode::InvalidInput,
                    format!("unsupported output container .{ext}"),
                );
            }
            (None, None) => {
                warnings.push(format!("unknown container .{ext}; writing .mkv"));
                (Container::Mkv, "mkv".to_owned())
            }
        };
        let mut encode = None;
        if mode == Mode::Accurate && up.is_none() && gif.is_none() {
            if container.encode_profile().is_none() {
                warnings.push(format!(
                    "accurate mode cannot write .{out_ext}; writing .mp4 instead (choose --container to override)"
                ));
                container = Container::Mp4;
                out_ext = "mp4".to_owned();
            }
            let profile = container.encode_profile().expect("checked above");
            for enc in [profile.video_encoder, profile.audio_encoder] {
                if !self.caps.encoders.contains(enc) {
                    return self.error(
                        origin,
                        IssueCode::EngineMissing,
                        format!("the ffmpeg build lacks the {enc} encoder needed for accurate mode"),
                    );
                }
            }
            encode = Some(profile);
            if let Some(fmt) = media.first_video().and_then(|v| v.pix_fmt.as_deref())
                && fit.is_none()
                && ["10", "12", "16"].iter().any(|d| fmt.contains(d))
            {
                warnings.push(format!(
                    "source is high bit depth ({fmt}); accurate mode writes 8-bit 4:2:0 (HDR is not preserved — use fast mode to keep it)"
                ));
            }
        }

        // A GIF or Reddit MP4 is silent and has no subtitles.
        let silent = crate::request::StreamSelection {
            audio: crate::request::AudioSel::None,
            subtitles: crate::request::SubSel::None,
        };
        let selection = if gif.is_some() { &silent } else { &opts.streams };
        let mut stream_plan = match streams::select_streams(media, selection, container, mode) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, IssueCode::InvalidInput, msg),
        };
        warnings.extend(stream_plan.warnings.iter().cloned());
        if fit.is_some() {
            // An MP4 for Reddit: the first video and the first audio track, nothing else (R6).
            let audio = stream_plan.maps.iter().filter(|m| m.kind == crate::probe::StreamKind::Audio).count();
            let mut kept = (false, false);
            stream_plan.maps.retain(|m| match m.kind {
                crate::probe::StreamKind::Video if !kept.0 => {
                    kept.0 = true;
                    true
                }
                crate::probe::StreamKind::Audio if !kept.1 => {
                    kept.1 = true;
                    true
                }
                _ => false,
            });
            stream_plan.subtitle_codec = None;
            if audio > 1 {
                warnings
                    .push(format!("only the first of {audio} audio tracks is kept (a Reddit video has one)"));
            }
        }

        // --- output path ---------------------------------------------------------------------
        // Absolute paths: upscale jobs run ffmpeg inside their scratch directory.
        let input = std::path::absolute(&source.path).unwrap_or_else(|_| source.path.clone());
        let source_dir = input.parent().map(Path::to_path_buf).unwrap_or_default();
        let out_dir = match &opts.output_dir {
            Some(d) if d.is_absolute() => d.clone(),
            Some(d) => std::path::absolute(d).unwrap_or_else(|_| d.clone()),
            None if gif.is_some() => source_dir.join("gifs"),
            None => source_dir.join(if up.is_some() && whole_video { "upscaled" } else { "clips" }),
        };
        let template = match (&up, gif) {
            (_, Some(_)) if opts.name_template == DEFAULT_NAME_TEMPLATE && whole_video => GIF_NAME_TEMPLATE,
            (_, Some(_)) if opts.name_template == DEFAULT_NAME_TEMPLATE => GIF_CLIP_NAME_TEMPLATE,
            (_, Some(_)) if opts.name_template == MULTI_SOURCE_NAME_TEMPLATE && whole_video => {
                GIF_NAME_TEMPLATE
            }
            (_, Some(_)) if opts.name_template == MULTI_SOURCE_NAME_TEMPLATE => GIF_MULTI_CLIP_NAME_TEMPLATE,
            (_, Some(_)) => opts.name_template.as_str(),
            (up, None) => match up {
                Some(_) if opts.name_template == DEFAULT_NAME_TEMPLATE && whole_video => {
                    UPSCALE_NAME_TEMPLATE
                }
                Some(_) if opts.name_template == DEFAULT_NAME_TEMPLATE => UPSCALE_CLIP_NAME_TEMPLATE,
                // Whole-video names are unique per folder already; clips keep their sub-folder.
                Some(_) if opts.name_template == MULTI_SOURCE_NAME_TEMPLATE && whole_video => {
                    UPSCALE_NAME_TEMPLATE
                }
                Some(_) if opts.name_template == MULTI_SOURCE_NAME_TEMPLATE => {
                    UPSCALE_MULTI_CLIP_NAME_TEMPLATE
                }
                _ => opts.name_template.as_str(),
            },
        };
        let stem = source.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let rel = match naming::render(
            template,
            &naming::NameVars {
                source_stem: &stem,
                index,
                name: clip.name.as_deref(),
                start: clip.start,
                end: requested_end,
                ext: &out_ext,
                preset: up.as_ref().map(|(u, ..)| u.preset.as_str()),
                target: gif.map(|g| g.target.as_str()).or(fit.as_ref().map(|(_, t)| t.id.as_str())),
                format: gif.map(|g| g.format.ext()),
            },
        ) {
            Ok(p) => p,
            Err(msg) => return self.error(origin, IssueCode::InvalidInput, msg),
        };
        let Some((output, disposition)) = self.claim_output(
            origin,
            out_dir.join(rel),
            opts.on_exists,
            if up.is_some() || gif.is_some() { "jobs" } else { "clips" },
            &mut warnings,
        ) else {
            return;
        };

        // --- seek plan (research.md R3) -----------------------------------------------------
        let requested = Span { start: clip.start, end: requested_end };
        // Decide from the demuxer ffprobe actually used, never the extension (a mislabelled
        // MPEG-TS named .mp4 must still get the two-stage seek).
        let source_precise = media.has_indexed_seeking();
        let frame = media.frame_duration().unwrap_or(Ts::from_micros(40_000));
        // Only argument combinations whose timing does not depend on how ffmpeg relates input
        // `-ss` to the container start are used (research.md R3).
        let mut trim = None;
        let (input_seek, output_seek, cut, predicted) = if clip.start == Ts::ZERO {
            (None, None, Ts::ZERO, None)
        } else if mode == Mode::Accurate && source_precise {
            // Index-based demuxer seek + decode is frame exact.
            (Some(clip.start), None, clip.start, None)
        } else if mode == Mode::Accurate {
            // Approximate seek: land well before the start, keep absolute timestamps and cut with
            // trim/atrim filters at absolute times — exact wherever the seek landed.
            let zero = media.zero_point();
            trim = Some(ffargs::Trim { start: zero + clip.start, end: end.map(|e| zero + e) });
            if stream_plan.maps.iter().any(|m| m.kind == crate::probe::StreamKind::Subtitle) {
                stream_plan.maps.retain(|m| m.kind != crate::probe::StreamKind::Subtitle);
                stream_plan.subtitle_codec = None;
                warnings.push(
                    "subtitles cannot be cut frame-accurately from this kind of source; dropped".into(),
                );
            }
            (Some(clip.start.saturating_sub(COARSE_SEEK_MARGIN)), None, clip.start, None)
        } else {
            // Stream copy must start on a keyframe: find it exactly, then cut just before it
            // (ffmpeg drops leading non-keyframes, so the output starts precisely at it).
            let key = match probe::keyframe_at_or_before(self.tools, media, clip.start, self.cancel).await {
                Ok(Some(k)) => k,
                Ok(None) => {
                    return self.error(
                        origin,
                        IssueCode::InvalidInput,
                        format!("no keyframe found before {}; use --mode accurate for this clip", clip.start),
                    );
                }
                Err(Error::Cancelled) => return,
                Err(e) => return self.error(origin, e.issue_code(), e.to_string()),
            };
            let gap = key.previous.map_or(Ts::from_micros(i64::MAX / 4), |p| key.at - p);
            let lead = Ts::from_micros((frame.as_micros() * 4).min(gap.as_micros() / 2));
            let predicted = Some(Span { start: key.at, end: requested_end });
            if source_precise {
                // Demuxer seek to S lands exactly on K (≤ S) for indexed containers.
                (Some(clip.start), None, clip.start, predicted)
            } else {
                // No demuxer seek: copy packets from the start (cheap) and cut just before K;
                // leading non-keyframes are dropped, so the output starts exactly on K.
                let cut = key.at.saturating_sub(lead);
                (None, Some(cut), cut, predicted)
            }
        };

        // ffmpeg's `-ss` timeline starts at the container start; user time starts at the first
        // video frame. Shift every seek by the difference (non-zero e.g. when audio leads).
        let shift = media.zero_point().saturating_sub(media.start_time);
        let (input_seek, output_seek) = if trim.is_some() {
            (input_seek, output_seek)
        } else {
            (input_seek.map(|t| t + shift), output_seek.map(|t| t + shift))
        };

        let id = self.jobs.len() as JobId + 1;
        let temp = temp_path(&output, &self.run_id, id);
        let mut cut = ClipSpec {
            input,
            input_seek,
            output_seek,
            duration: if trim.is_some() { None } else { end.map(|e| e - cut) },
            trim,
            verify_video: self.expected_video(media, mode, clip.start, end, frame),
            mode,
            requested,
            predicted,
            container,
            encode,
            streams: stream_plan,
            smart: None,
            smart_fallback: None,
            fit: None,
        };
        // An upscale's fit is planned from its output size and rate, below.
        if let Some((f, target)) = &fit
            && up.is_none()
        {
            let Some(found) = self.plan_fit(origin, media, &mut cut, (f, target), &mut warnings, None) else {
                return;
            };
            for (code, message, detail) in found {
                notice(&mut warnings, code, message, detail);
            }
        }
        let mut requested = cut.requested;
        let mut smart = None;
        if smart_requested {
            let strict = !opts.smart_fallback;
            let reason = match self.smart_plan(media, &cut, end.is_none()).await {
                Ok(mut p) => {
                    p.strict = strict;
                    cut.mode = Mode::Smart;
                    cut.smart = Some(p);
                    None
                }
                Err(None) if self.cancel.is_cancelled() => return,
                Err(reason) => Some(reason),
            };
            if let Some(reason) = reason {
                // `None`: the keyframes could not be read.
                let why = reason.map_or("the keyframes could not be read", |r| r.message());
                let detail =
                    reason.map(|r| crate::error::ErrorDetail { reason: Some(r), ..Default::default() });
                if strict {
                    self.issues.push(Issue {
                        severity: Severity::Error,
                        code: IssueCode::SmartUnavailable,
                        origin: origin.clone(),
                        message: format!("smart cut not possible: {why}"),
                        detail,
                    });
                    return;
                }
                let message = format!("smart cut not possible: {why}; cutting in accurate mode");
                notice(&mut warnings, IssueCode::SmartFallback, message, detail);
                cut.smart_fallback = reason;
            }
            smart = Some(SmartSummary::of(&cut, strict));
        }
        let (spec, scratch) = match (gif, up) {
            (Some(g), _) => {
                let scratch = scratch_path(&output, &self.run_id, id);
                // An engine found on PATH may not come from the input folder or the scratch.
                let refused = [source_dir.clone(), scratch.clone()];
                // Whole videos keep their timestamps (as for upscaling): smart-reframe commands
                // are written on that timeline.
                let cmd_offset =
                    if whole { (media.zero_point() - media.start_time).as_secs_f64() } else { 0.0 };
                match crate::gif::plan::plan_gif(
                    g,
                    &self.targets,
                    self.caps,
                    media,
                    &cut,
                    &refused,
                    cmd_offset,
                ) {
                    Ok(p) => {
                        warnings.extend(p.warnings);
                        notices.extend(p.notices);
                        (
                            JobSpec::Gif(Box::new(crate::gif::GifSpec { cut, frame: p.frame, gif: p.gif })),
                            Some(scratch),
                        )
                    }
                    Err((code, msg)) => return self.error(origin, code, msg),
                }
            }
            (None, None) => {
                let scratch = (cut.smart.is_some() || cut.fit.is_some())
                    .then(|| scratch_path(&output, &self.run_id, id));
                (JobSpec::Clip(Box::new(cut)), scratch)
            }
            (None, Some((u, preset, tier))) => {
                let again = fit.as_ref().map(|_| (preset.clone(), tier.clone()));
                let mut plan = self.upscale_plan(origin, u, preset, tier, media, &cut, whole, &mut warnings);
                if let (Some((f, target)), Some((preset, tier))) = (&fit, again) {
                    // Feature 005 (research R7): the fit takes the preset's shape and rate; the
                    // budget may lower the size further or trim the range.
                    let out = Some((plan.target, plan.frame_rate));
                    let Some(found) = self.plan_fit(origin, media, &mut cut, (f, target), &mut warnings, out)
                    else {
                        return;
                    };
                    if cut.fit.as_ref().is_some_and(|p| p.trimmed_from.is_some()) {
                        // Trimmed: the upscale is planned again for the shorter range.
                        plan =
                            self.upscale_plan(origin, u, preset, tier, media, &cut, false, &mut Vec::new());
                        requested = cut.requested;
                    }
                    for (code, message, detail) in found {
                        warnings.push(message.clone());
                        notices.push(Notice { code, message, detail });
                    }
                    if let Some(p) = &cut.fit {
                        // Two passes take about twice as long (R7); the size is the fit's.
                        plan.estimate.secs *= f64::from(p.passes());
                        plan.estimate.bytes = p.predicted_bytes;
                    }
                }
                let scratch = scratch_path(&output, &self.run_id, id);
                (JobSpec::Upscale(Box::new(UpscaleSpec { cut, up: plan })), Some(scratch))
            }
        };
        self.jobs.push(PlannedJob {
            id,
            source: source.path.clone(),
            output,
            temp,
            scratch,
            on_exists: opts.on_exists,
            disposition,
            requested,
            warnings,
            notices,
            smart,
            fit: match &spec {
                JobSpec::Clip(c) => c.fit.as_ref().map(crate::fit::plan::FitPlan::summary),
                JobSpec::Upscale(u) => u.cut.fit.as_ref().map(crate::fit::plan::FitPlan::summary),
                JobSpec::Gif(_) => None,
            },
            spec,
        });
    }

    /// Plans `cut`'s fit to `target` (feature 005, research R2–R5): refusals become issues; a
    /// trim shortens `cut`. Returns the notices (lowered settings, trims); `None`: refused.
    pub(super) fn plan_fit(
        &mut self,
        origin: &Origin,
        media: &MediaInfo,
        cut: &mut ClipSpec,
        (opts, target): (&crate::fit::FitOptions, &crate::fit::target::VideoTarget),
        warnings: &mut Vec<String>,
        upscaled: Option<((u32, u32), (u32, u32))>,
    ) -> Option<Vec<(IssueCode, String, Option<crate::error::ErrorDetail>)>> {
        use crate::fit::plan::{Refusal, Source, plan_fit};
        let Some(v) = media.first_video() else {
            self.error(origin, IssueCode::InvalidInput, "a fit needs a video stream");
            return None;
        };
        let Some(source_size) = v.display_size() else {
            self.error(origin, IssueCode::InvalidInput, "the source's picture size is unknown");
            return None;
        };
        let source_fps =
            if v.is_vfr() { v.r_frame_rate.or(v.frame_rate) } else { v.frame_rate.or(v.r_frame_rate) };
        // An upscale's output (its preset's shape and rate), or the source.
        let (size, fps) = match upscaled {
            Some((size, fps)) => (size, Some(fps)),
            None => (source_size, source_fps),
        };
        let audio = cut
            .streams
            .maps
            .iter()
            .find(|m| m.kind == crate::probe::StreamKind::Audio)
            .and_then(|m| media.streams.iter().find(|s| s.index == m.index));
        // Principle VI: without the encoders a fit cannot run (ENGINE_MISSING, exit 4).
        let lacking: Vec<&str> = ["libx264", "aac"]
            .into_iter()
            .filter(|e| (*e != "aac" || audio.is_some()) && !self.caps.encoders.contains(*e))
            .collect();
        if !lacking.is_empty() {
            self.error(
                origin,
                IssueCode::EngineMissing,
                format!("the ffmpeg build lacks {} needed to fit a clip", lacking.join(", ")),
            );
            return None;
        }
        let src = Source {
            len: cut.requested.end.saturating_sub(cut.requested.start).as_secs_f64(),
            size,
            fps: fps.unwrap_or((30, 1)),
            has_audio: audio.is_some(),
        };
        let requested_len = src.len;
        let mut plan = match plan_fit(target, opts, src) {
            Ok(p) => p,
            Err(r) => {
                let (code, detail) = match &r {
                    Refusal::TooLong { len, max_length } => (
                        IssueCode::RangeTooLong,
                        Some(crate::error::ErrorDetail {
                            len: Some(*len),
                            max_length: Some(*max_length),
                            ..Default::default()
                        }),
                    ),
                    Refusal::Unreachable { min_cap, .. } => (
                        IssueCode::BudgetUnreachable,
                        Some(crate::error::ErrorDetail { min_cap: Some(*min_cap), ..Default::default() }),
                    ),
                    Refusal::AboveTarget(_) => (IssueCode::InvalidInput, None),
                };
                self.issues.push(Issue {
                    severity: Severity::Error,
                    code,
                    origin: origin.clone(),
                    message: r.message(&target.id),
                    detail,
                });
                return None;
            }
        };
        if let Some(c) = audio.and_then(|a| a.channels).filter(|c| *c > 2) {
            plan.downmix = true;
            warnings.push(format!("the {c}-channel audio is mixed down to stereo"));
        }
        // An upscale's graph already converts HDR to SDR.
        if v.is_hdr() && upscaled.is_none() {
            // 002's tone-map to SDR, so every phone plays it (spec › Edge cases).
            let lacking: Vec<&str> =
                ["zscale", "tonemap"].into_iter().filter(|f| !self.caps.filters.contains(*f)).collect();
            if !lacking.is_empty() {
                self.error(
                    origin,
                    IssueCode::EngineMissing,
                    format!(
                        "the ffmpeg build lacks {} needed to tone-map this HDR source",
                        lacking.join(", ")
                    ),
                );
                return None;
            }
            let tags = crate::upscale::encode::ColourTags {
                space: v.color_space.clone(),
                primaries: v.color_primaries.clone(),
                transfer: v.color_transfer.clone(),
                range: v.color_range.clone(),
            };
            plan.tonemap =
                Some(crate::upscale::graph::zscale_input(&tags, v.height.unwrap_or(source_size.1)));
        }
        let mut found = Vec::new();
        let mut notice = |code, message: String, detail| found.push((code, message, detail));
        for l in &plan.lowered {
            let what = if l.setting == "size" { "size" } else { "audio bitrate (kb/s)" };
            notice(
                IssueCode::FitLowered,
                format!(
                    "{what} lowered {} → {} to fit {}",
                    setting_text(&l.from),
                    setting_text(&l.to),
                    target.id
                ),
                None,
            );
        }
        if plan.len < requested_len - 1e-9 {
            // A shorter range: trimmed, or the length limit in whole frames. The cut ends 1 ms
            // early so a frame that starts exactly at the end is not kept (rounding).
            let d = Ts::from_secs_f64(plan.len);
            let cut_d = Ts::from_secs_f64(plan.len - 0.001);
            cut.requested.end = cut.requested.start + d;
            match &mut cut.trim {
                Some(t) => t.end = Some(t.start + cut_d),
                None => cut.duration = Some(cut_d),
            }
            if let Some((_, tolerance)) = cut.verify_video {
                cut.verify_video = Some((d, tolerance));
            }
        }
        if let Some(from) = plan.trimmed_from {
            notice(
                IssueCode::FitTrimmed,
                format!(
                    "the range was shortened from {:.1} s to {:.1} s to fit {}",
                    from, plan.len, target.id
                ),
                Some(crate::error::ErrorDetail {
                    from: Some(from),
                    to: Some(plan.len),
                    ..Default::default()
                }),
            );
        }
        cut.fit = Some(plan);
        Some(found)
    }

    /// A smart job's plan (feature 004, research R2): the source checks, then the keyframe index
    /// of the range and the range checks. `Err(None)`: the index could not be read.
    pub(super) async fn smart_plan(
        &self,
        media: &MediaInfo,
        cut: &ClipSpec,
        to_end: bool,
    ) -> Result<crate::smart::SmartPlan, Option<crate::smart::FallbackReason>> {
        let x264 = self.caps.encoders.contains("libx264");
        crate::smart::check_source(media, x264, cut.container)?;
        // Past the end, so a keyframe on the end and the packets after it (the open-GOP rule) are
        // read too.
        let window = crate::event::Span { start: cut.requested.start, end: cut.requested.end + INDEX_RUNOUT };
        let index =
            crate::smart::keyframes(self.tools, media, Some(window), self.cancel).await.map_err(|e| {
                tracing::warn!("smart cut: keyframe index of {}: {e}", media.path.display());
                None
            })?;
        let range = crate::smart::Range { span: cut.requested, to_end };
        Ok(crate::smart::check(media, &index, range, x264, cut.container)?)
    }

    /// Upscale checks that need no output path: the preset exists, HDR can be kept, the tier can
    /// run here, and the engine has what the job needs (FR-008, research R4). Reports an issue
    /// and returns `None` when the job cannot be planned.
    pub(super) fn check_upscale(
        &mut self,
        origin: &Origin,
        u: &UpscaleOptions,
        media: &MediaInfo,
    ) -> Option<(Preset, TierDecision)> {
        let Some(preset) = self.presets.get(&u.preset).cloned() else {
            let known = self.presets.ids().join(", ");
            self.error(
                origin,
                IssueCode::PresetUnknown,
                format!("unknown preset {:?} (known: {known})", u.preset),
            );
            return None;
        };
        if let Some(crf) = u.crf.filter(|&c| c > crate::upscale::MAX_CRF) {
            self.error(
                origin,
                IssueCode::InvalidInput,
                format!("crf {crf} is out of range (0 to {})", crate::upscale::MAX_CRF),
            );
            return None;
        }
        if u.hdr == HdrMode::Keep && !preset.hdr_capable {
            self.error(
                origin,
                IssueCode::HdrUnsupported,
                format!("preset {} cannot keep HDR; use --hdr sdr or an HDR-capable preset", preset.id),
            );
            return None;
        }
        let display =
            media.first_video().and_then(|v| v.display_size()).unwrap_or((preset.width, preset.height));
        let tier = match crate::upscale::tier::select(u.tier, self.caps.capability.as_ref(), display) {
            Ok(t) => t,
            Err(msg) => {
                self.error(origin, IssueCode::TierUnavailable, msg);
                return None;
            }
        };
        let v = media.first_video()?;
        if tier.tier == Tier::Ai {
            if self.caps.ai.is_none() {
                self.error(
                    origin,
                    IssueCode::TierUnavailable,
                    "the ai tier is not available on this machine: the AI worker (rmedia-sr) or its models are not installed"
                        .to_owned(),
                );
                return None;
            }
            if u.hdr == HdrMode::Keep && v.is_hdr() {
                self.error(
                    origin,
                    IssueCode::HdrUnsupported,
                    "the AI tier makes SDR output; use --hdr sdr or another tier to keep HDR".to_owned(),
                );
                return None;
            }
        }
        let encoder = match preset.video.codec {
            VideoCodec::Hevc => "libx265",
            VideoCodec::H264 => "libx264",
        };
        let mut filters = vec!["zscale", "cas", "hqdn3d", "aac"];
        if v.is_interlaced() {
            filters.push("bwdif");
        }
        if v.is_hdr() && u.hdr == HdrMode::Sdr {
            filters.push("tonemap");
        }
        if u.reframe != Reframe::Center {
            filters.push("gblur");
        }

        let missing: Vec<&str> = std::iter::once(encoder)
            .chain(filters)
            .filter(|f| !self.caps.encoders.contains(*f) && !self.caps.filters.contains(*f))
            .collect();
        if !missing.is_empty() {
            self.error(
                origin,
                IssueCode::EngineMissing,
                format!("the ffmpeg build lacks {} needed for upscaling", missing.join(", ")),
            );
            return None;
        }
        Some((preset, tier))
    }

    /// Resolves geometry, colour, frame rate, encoder, estimate and warnings for an upscale job
    /// (research R2–R5, R12). `cut` is the job's source range.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn upscale_plan(
        &mut self,
        origin: &Origin,
        u: &UpscaleOptions,
        preset: Preset,
        tier: TierDecision,
        media: &MediaInfo,
        cut: &ClipSpec,
        whole: bool,
        warnings: &mut Vec<String>,
    ) -> UpscalePlan {
        let v = media.first_video().expect("checked: the source has video");
        let target = (preset.width, preset.height);
        let display = v.display_size().unwrap_or(target);
        // Smart reframe pans horizontally (v1): it needs a face model and a source wider than
        // the target. Otherwise it degrades to a reframe that never cuts anything off.
        let overflow = geometry::plan(display, target, u.reframe).overflow;
        let smart_missing: Vec<&str> =
            SMART_FILTERS.iter().copied().filter(|f| !self.caps.filters.contains(*f)).collect();
        let effective = match (u.reframe, overflow, &self.caps.face) {
            (Reframe::Smart, Overflow::Horizontal, None) => {
                warnings.push("face model not found; using blurred background".into());
                Reframe::Blur
            }
            (Reframe::Smart, Overflow::Horizontal, Some(_)) if !smart_missing.is_empty() => {
                warnings.push(format!(
                    "the ffmpeg build lacks {} needed for smart reframe; using blurred background",
                    smart_missing.join(", ")
                ));
                Reframe::Blur
            }
            (Reframe::Smart, Overflow::Vertical, _) => {
                warnings.push("smart reframe pans horizontally only; using a centre crop".into());
                Reframe::Center
            }
            (r, ..) => r,
        };
        let geometry = geometry::plan(display, target, effective);
        let smart = effective == Reframe::Smart && geometry.overflow == Overflow::Horizontal;
        if geometry.no_upscale_needed {
            warnings.push(format!(
                "no upscaling needed: the source ({}x{}) is already at least {}x{}; it is only reframed and scaled down",
                display.0, display.1, target.0, target.1
            ));
        }
        if display.0.min(display.1) <= 240 {
            warnings.push(format!(
                "very small source ({}x{}): upscaling cannot invent detail",
                display.0, display.1
            ));
        }
        let colour_in = ColourTags {
            space: v.color_space.clone(),
            primaries: v.color_primaries.clone(),
            transfer: v.color_transfer.clone(),
            range: v.color_range.clone(),
        };
        let keep_hdr = u.hdr == HdrMode::Keep && v.is_hdr();
        let tonemap = u.hdr == HdrMode::Sdr && v.is_hdr();
        if keep_hdr {
            warnings.push(
                "HDR is kept without its mastering metadata (MaxCLL/mastering display are not carried)"
                    .into(),
            );
        }
        if u.hdr == HdrMode::Keep && !v.is_hdr() {
            warnings.push("the source is SDR; it stays SDR".into());
        }
        let src_fps = v.frame_rate.map(|(n, d)| f64::from(n) / f64::from(d));
        let fps_cap = src_fps.filter(|f| *f > f64::from(preset.max_fps) + 0.01).map(|_| preset.max_fps);
        let fps_out = match (fps_cap, src_fps) {
            (Some(c), _) => f64::from(c),
            (None, Some(f)) => f,
            (None, None) => 30.0,
        };
        let frame_rate = match (fps_cap, v.frame_rate) {
            (Some(c), _) => (c, 1),
            (None, Some((n, d))) if n > 0 && d > 0 => (n, d),
            _ => (30, 1),
        };
        let graph = GraphInput {
            trim: cut.trim.map(|t| (t.start, t.end)),
            deinterlace: v.is_interlaced(),
            square_pixels: v.sar.is_some_and(|(n, d)| n != d),
            tonemap,
            keep_hdr,
            colour_in: crate::upscale::graph::zscale_input(&colour_in, v.height.unwrap_or(0)),
            params_in: crate::upscale::graph::setparams_input(&colour_in, v.height.unwrap_or(0)),
            flatten_alpha: false,
            gpu: (tier.tier == Tier::Gpu).then_some(tier.device).flatten(),
            fps_cap,
            geometry,
            reframe: effective,
            // Until the analysis runs: centred, with the fallback branch available.
            track: smart.then(|| Track { x0: geometry.crop.map_or(0, |c| c.x), fallback: true }),
        };
        let audio: Vec<&crate::probe::StreamInfo> = cut
            .streams
            .maps
            .iter()
            .filter(|m| m.kind == crate::probe::StreamKind::Audio)
            .filter_map(|m| media.streams.iter().find(|s| s.index == m.index))
            .collect();
        // Copying is exact only for whole videos; a cut re-encodes so audio starts with the frame.
        let audio_copy = preset.audio.copy_if_aac && whole && audio.iter().all(|s| s.codec == "aac");
        let audio_kbps = (!audio.is_empty()).then_some(preset.audio.bitrate_k);
        let len = cut.requested.end.saturating_sub(cut.requested.start).as_secs_f64();
        let mut est = estimate::estimate(
            &preset,
            fps_out,
            len,
            audio_kbps,
            estimate::encode_fps(tier.tier, self.caps.capability.as_ref(), target),
        );
        // The AI worker runs alongside the encoder: the slower of the two sets the time.
        let ai_rate = self.caps.capability.as_ref().and_then(|r| r.tiers.ai.sec_per_mpx);
        if let (Tier::Ai, Some(spm)) = (tier.tier, ai_rate) {
            let st = crate::upscale::ai::stage(&graph);
            let mpx = f64::from(st.input.0) * f64::from(st.input.1) / 1e6;
            let ai = crate::upscale::capability::ai_secs(f64::from(spm), len * fps_out, mpx);
            est.secs = est.secs.max(ai);
            if let Some(w) = crate::upscale::tier::ai_too_slow(ai, len) {
                warnings.push(w);
            }
        }
        let limit_warnings = estimate::limit_warnings(&preset, &est, len);
        for w in &limit_warnings {
            self.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::LimitExceeded,
                origin: origin.clone(),
                message: w.clone(),
                detail: None,
            });
        }
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get() as u32).clamp(2, 8);
        let video_args = encode::video_args(&EncodeParams {
            preset: &preset,
            crf: u.crf,
            hdr_keep: keep_hdr.then_some(&colour_in),
            fps_out,
            threads,
            maxrate_kbps: estimate::maxrate_for_limit(&preset, &est, len, audio_kbps),
        });
        let ai = (tier.tier == Tier::Ai).then(|| self.caps.ai.clone()).flatten();
        let expected_video =
            if whole { v.duration.or(media.duration) } else { Some(cut.requested.end - cut.requested.start) };
        UpscalePlan {
            target,
            geometry,
            reframe: u.reframe,
            reframe_applied: effective,
            motion: u.motion,
            analysis: if smart { AnalysisState::Pending } else { AnalysisState::NotNeeded },
            detector: if smart { self.caps.face.clone() } else { None },
            analysis_size: reframe::decode::analysis_size(display),
            reframe_cache: self.caps.reframe_cache.clone(),
            // Whole videos keep their timestamps: the video may start after the audio.
            cmd_offset: if whole { (media.zero_point() - media.start_time).as_secs_f64() } else { 0.0 },
            tier,
            hdr: u.hdr,
            colour_in,
            graph,
            video_args,
            audio_copy,
            fps_out,
            frame_rate,
            ai,
            expected_video,
            estimate: est,
            limit_warnings,
            preset,
        }
    }

    /// Warns when an output folder lacks room for the upscale or GIF outputs planned into it
    /// (FR-021).
    pub(super) fn check_free_space(&mut self) {
        let needs = space_needs(&self.jobs);
        for message in estimate::check_space(&needs, estimate::free_space) {
            self.issues.push(Issue {
                severity: Severity::Warning,
                code: IssueCode::DiskFull,
                origin: Origin::Cli,
                message,
                detail: None,
            });
        }
    }

    /// For accurate clips with an explicit end inside the video stream: the video length the output
    /// must have, with a tolerance of 3 frames (plus 100 ms).
    pub(super) fn expected_video(
        &self,
        media: &MediaInfo,
        mode: Mode,
        start: Ts,
        end: Option<Ts>,
        frame: Ts,
    ) -> Option<(Ts, Ts)> {
        let end = end?;
        let video_len = media.first_video()?.duration?;
        (mode == Mode::Accurate && end <= video_len)
            .then(|| (end - start, Ts::from_micros(frame.as_micros() * 3 + 100_000)))
    }

    /// Resolves collisions for `output` (source files, outputs claimed earlier in this plan, files
    /// on disk) under the `on_exists` policy and claims the final path. Reports an issue and
    /// returns `None` when the job cannot get an output. `what` names the jobs in messages.
    pub(super) fn claim_output(
        &mut self,
        origin: &Origin,
        mut output: PathBuf,
        on_exists: OnExists,
        what: &str,
        warnings: &mut Vec<String>,
    ) -> Option<(PathBuf, Disposition)> {
        if self.is_source(&output) {
            self.error(
                origin,
                IssueCode::InvalidInput,
                format!("output {} would overwrite a source video", output.display()),
            );
            return None;
        }
        if self.claimed.contains(&path_key(&output)) {
            if on_exists == OnExists::Rename {
                output = self.free_name(&output);
            } else {
                self.error(
                    origin,
                    IssueCode::InvalidInput,
                    format!(
                        "two {what} would write {}; use a name template with {{index}} or --on-exists rename",
                        output.display()
                    ),
                );
                return None;
            }
        }
        let mut disposition = Disposition::Run;
        if output.exists() {
            match on_exists {
                OnExists::Skip => disposition = Disposition::Skip { reason: "output already exists".into() },
                OnExists::Overwrite => warnings.push("existing output will be replaced".into()),
                OnExists::Rename => output = self.free_name(&output),
            }
        }
        self.claimed.insert(path_key(&output));
        Some((output, disposition))
    }

    pub(super) fn is_source(&self, output: &Path) -> bool {
        // Canonicalize the nearest existing ancestor (resolving symlinked folders), then re-append
        // the parts that do not exist yet.
        let mut existing = output.to_path_buf();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.file_name().map(ToOwned::to_owned), existing.parent()) {
                (Some(name), Some(parent)) => {
                    rest.push(name);
                    existing = parent.to_path_buf();
                }
                _ => break,
            }
        }
        let mut resolved = std::fs::canonicalize(&existing).unwrap_or(existing);
        for part in rest.iter().rev() {
            resolved.push(part);
        }
        self.sources.contains(&path_key(&resolved))
    }

    /// `name_1.ext`, `name_2.ext`, … not on disk and not claimed in this plan.
    pub(super) fn free_name(&self, path: &Path) -> PathBuf {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        (1..)
            .map(|i| path.with_file_name(format!("{stem}_{i}{ext}")))
            .find(|p| !p.exists() && !self.claimed.contains(&path_key(p)))
            .expect("unbounded search")
    }
}

/// 64-bit run id. `RandomState` is keyed from the OS RNG, so temp names are not predictable
/// (prevents pre-planting a symlink at a temp path in a shared output folder).
pub(super) fn new_run_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// A lowered setting for a message: `[1920, 1080]` → `1920x1080`, a number as is.
fn setting_text(v: &serde_json::Value) -> String {
    match v.as_array().map(Vec::as_slice) {
        Some([w, h]) => format!("{w}x{h}"),
        _ => v.to_string(),
    }
}
