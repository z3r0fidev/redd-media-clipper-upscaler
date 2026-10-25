//! Output rendering: human (progress bars on stderr, results on stdout) or `--json`
//! (contracts/json-output.md). Every JSON document carries `"schema": 1`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use rmedia_core::event::{Event, EventSink, JobId, JobMeasure, JobOutcome, RunSummary};
use rmedia_core::pathser::lossy;
use rmedia_core::plan::ffargs::display_args;
use rmedia_core::plan::{Disposition, JobSpec, Plan, PlannedJob, Severity};
use rmedia_core::request::Origin;
use serde_json::{Value, json};

use crate::exit;

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub json: bool,
    pub quiet: bool,
    pub verbose: u8,
}

impl Output {
    pub fn new(json: bool, quiet: bool, verbose: u8) -> Self {
        Output { json, quiet, verbose }
    }

    pub fn input_error(&self, msg: &str) -> u8 {
        self.error_doc("INVALID_INPUT", msg);
        exit::INVALID_INPUT
    }

    pub fn engine_error(&self, msg: &str) -> u8 {
        self.error_doc("ENGINE_MISSING", msg);
        exit::ENGINE
    }

    fn error_doc(&self, code: &str, msg: &str) {
        if self.json {
            print_json(&json!({"schema": 1, "type": "error", "code": code, "message": msg}));
        }
        eprintln!("error: {msg}");
    }

    /// Input-file problems found before planning (exit 3).
    pub fn issues(&self, issues: &[rmedia_core::plan::Issue]) -> u8 {
        let plan =
            Plan { run_id: String::new(), jobs: Vec::new(), issues: issues.to_vec(), cancelled: false };
        self.plan_issues(&plan);
        exit::INVALID_INPUT
    }

    pub fn plan_issues(&self, plan: &Plan) {
        if self.json {
            print_json(&plan_doc(plan));
        }
        for i in &plan.issues {
            let level = if i.severity == Severity::Error { "error" } else { "warning" };
            eprintln!("{level}: {i}");
        }
    }

    pub fn plan_warnings(&self, plan: &Plan) {
        if self.json || self.quiet {
            return;
        }
        for i in plan.issues.iter().filter(|i| i.severity == Severity::Warning) {
            eprintln!("warning: {}", i.message);
        }
        for j in &plan.jobs {
            for w in &j.warnings {
                eprintln!("warning: {}: {w}", file_name(&j.output));
            }
        }
    }

    /// Dry run: the plan, nothing written.
    pub fn plan(&self, plan: &Plan) {
        if self.json {
            print_json(&plan_doc(plan));
            return;
        }
        for j in &plan.jobs {
            let status = match &j.disposition {
                Disposition::Run => "would write".to_owned(),
                Disposition::Skip { reason } => format!("would skip ({reason})"),
            };
            println!("#{:<3} {status}: {}", j.id, j.output.display());
            let how = j.mode().map_or_else(|| j.kind().to_owned(), |m| format!("{m:?}").to_lowercase());
            println!(
                "     from {}  {} → {}  [{how}]",
                j.source.display(),
                j.requested.start,
                j.requested.end
            );
            if let Some(p) = j.predicted()
                && p.start != j.requested.start
            {
                println!(
                    "     fast mode starts at the keyframe at {} ({} early)",
                    p.start,
                    j.requested.start - p.start
                );
            }
            if let JobSpec::Upscale(u) = &j.spec {
                let up = &u.up;
                println!(
                    "     preset {} {}x{}, reframe {}, tier {} ({})",
                    up.preset.id,
                    up.target.0,
                    up.target.1,
                    format!("{:?}", up.reframe).to_lowercase(),
                    format!("{:?}", up.tier.tier).to_lowercase(),
                    up.tier.reason
                );
                if up.tier.suggest.is_some() {
                    println!(
                        "     suggestion: --tier ai may recover more detail from this low-resolution source"
                    );
                }
                println!("     {}", reframe_line(&up.reframe_applied, &up.analysis));
                println!(
                    "     estimate ~{}, ~{:.0} s",
                    rmedia_core::upscale::estimate::human_bytes(up.estimate.bytes),
                    up.estimate.secs.max(1.0)
                );
            }
            if let JobSpec::Gif(g) = &j.spec {
                gif_lines(&g.gif);
            }
            for w in &j.warnings {
                println!("     warning: {w}");
            }
            if self.verbose > 0 {
                for c in j.commands() {
                    println!("     {} {}", program_name(c.program), shell_words(&display_args(&c.args)));
                }
            }
        }
        for i in plan.issues.iter().filter(|i| i.severity == Severity::Warning) {
            println!("warning: {}", i.message);
        }
        let skipped = plan.jobs.iter().filter(|j| j.disposition != Disposition::Run).count();
        let noun = if plan.jobs.iter().all(|j| j.kind() == "clip") { "clip(s)" } else { "job(s)" };
        println!(
            "{} {noun} planned, {skipped} would be skipped. Nothing was written (dry run).",
            plan.jobs.len() - skipped
        );
    }

    pub fn summary(&self, s: &RunSummary) {
        if self.json {
            return; // the summary line was streamed by the sink
        }
        for j in &s.jobs {
            match &j.outcome {
                JobOutcome::Succeeded { output, measure: JobMeasure::Clip { actual, adjusted } } => {
                    let note = if *adjusted {
                        format!(
                            "  (actual {} → {}, requested {} → {})",
                            actual.start, actual.end, j.requested.start, j.requested.end
                        )
                    } else {
                        String::new()
                    };
                    println!("✓ {}{note}", output.display());
                }
                JobOutcome::Succeeded { output, measure: JobMeasure::Upscale { .. } } => {
                    println!("✓ {}", output.display());
                }
                JobOutcome::Succeeded {
                    output,
                    measure:
                        JobMeasure::Gif {
                            bytes,
                            width,
                            height,
                            fps,
                            colours,
                            dither,
                            engine,
                            search,
                            trimmed,
                            ..
                        },
                } => {
                    let palette = match (colours, dither) {
                        (Some(c), Some(d)) => format!(", {c} colours, dither {}", d.label()),
                        _ => String::new(),
                    };
                    println!(
                        "✓ {}  ({}, {width}x{height}, {fps:.3} fps{palette}, {})",
                        output.display(),
                        rmedia_core::upscale::estimate::human_bytes(*bytes),
                        format!("{engine:?}").to_lowercase()
                    );
                    if let Some(r) = search {
                        let lowered = lowered_text(&r.lowered);
                        println!("  budget search: {} encodes{lowered}", r.encodes);
                    }
                    if let Some(t) = trimmed {
                        println!("  trimmed to {} → {} to fit the budget", t.start, t.end);
                    }
                }
                JobOutcome::Skipped { reason } => println!("- {} skipped: {reason}", j.output.display()),
                JobOutcome::Failed { error } => {
                    println!("✗ {} failed: {}", j.output.display(), error.message);
                    if self.verbose > 0
                        && let Some(tail) = &error.stderr_tail
                    {
                        eprintln!("{tail}");
                    }
                }
                JobOutcome::Cancelled => println!("✗ {} cancelled", j.output.display()),
            }
        }
        println!(
            "{} succeeded, {} skipped, {} failed, {} cancelled",
            s.succeeded, s.skipped, s.failed, s.cancelled
        );
    }
}

/// The GIF part of a dry-run job (contracts/cli.md › Human dry run).
fn gif_lines(g: &rmedia_core::gif::GifPlan) {
    use rmedia_core::gif::plan::EstimateFrom;
    use rmedia_core::upscale::estimate::human_bytes;
    let t = &g.target;
    let unverified = if t.verified { "" } else { ", limits unverified" };
    println!(
        "     target {} ({}{unverified}), {}, engine {} ({})",
        t.id,
        clean(&t.name),
        g.format.ext(),
        format!("{:?}", g.engine).to_lowercase(),
        g.engine_reason
    );
    let s = &g.start;
    let timing = s.timing();
    let fps = rmedia_core::gif::timing::fps_text(s.rate);
    let fps = if timing.exact {
        format!("{fps} fps")
    } else {
        format!("{fps} fps (≈ {:.2} cs, approximate)", timing.avg_cs)
    };
    let palette = if s.gifski.is_some() {
        String::new()
    } else {
        format!(", {} colours, dither {}", s.colours, s.dither.label())
    };
    println!("     {}x{}, {fps}{palette}", s.width, s.height);
    if let Some(b) = g.budget {
        println!("     budget {} soft / {} hard", human_bytes(b.soft), human_bytes(b.hard));
    }
    match (g.estimate.bytes, g.estimate.from) {
        (Some(bytes), from) => {
            let how = if from == EstimateFrom::Search { "after the budget search" } else { "from a sample" };
            println!("     estimate ~{} ({how}){}", human_bytes(bytes), lowered_text(&g.estimate.lowered));
        }
        (None, _) => println!("     estimate pending (not sampled)"),
    }
}

/// `; lowered lossy 30 → 90, width 720 → 584` (FR-012), or nothing.
fn lowered_text(lowered: &[rmedia_core::gif::plan::Lowered]) -> String {
    let show = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => o
            .iter()
            .map(
                |(k, v)| {
                    if k == "type" { v.as_str().unwrap_or_default().to_owned() } else { format!("{k} {v}") }
                },
            )
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    };
    if lowered.is_empty() {
        return String::new();
    }
    let parts: Vec<String> =
        lowered.iter().map(|l| format!("{} {} → {}", l.setting, show(&l.from), show(&l.to))).collect();
    format!("; lowered {}", parts.join(", "))
}

fn program_name(p: rmedia_core::plan::Program) -> &'static str {
    use rmedia_core::plan::Program;
    match p {
        Program::Ffmpeg => "ffmpeg",
        Program::AiWorker => "rmedia-sr",
        Program::Gifsicle => "gifsicle",
        Program::Gifski => "gifski",
    }
}

/// Replaces control characters (incl. ANSI escapes) in untrusted text before printing it.
pub fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn shell_words(args: &[String]) -> String {
    args.iter()
        .map(
            |a| {
                if a.contains([' ', '"', '\'']) {
                    format!("\"{}\"", a.replace('"', "\\\""))
                } else {
                    a.clone()
                }
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn print_json(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, v);
    let _ = writeln!(out);
}

fn origin_fields(o: &Origin, v: &mut serde_json::Map<String, Value>) {
    match o {
        Origin::Line { file, line } => {
            v.insert("file".into(), json!(lossy(file)));
            v.insert("line".into(), json!(line));
        }
        Origin::JsonPath { file, path } => {
            v.insert("file".into(), json!(lossy(file)));
            v.insert("json_path".into(), json!(path));
        }
        Origin::Cli | Origin::Gui => {}
    }
}

fn job_doc(j: &PlannedJob) -> Value {
    let mut v = json!({
        "id": j.id,
        "source": lossy(&j.source),
        "output": lossy(&j.output),
        "requested": j.requested,
        "warnings": j.warnings,
        "argv": display_args(&j.args()),
    });
    let m = v.as_object_mut().expect("object");
    if let Some(mode) = j.mode() {
        m.insert("mode".into(), json!(mode));
    }
    match &j.disposition {
        Disposition::Run => m.insert("status".into(), json!("planned")),
        Disposition::Skip { reason } => {
            m.insert("skip_reason".into(), json!(reason));
            m.insert("status".into(), json!("skip"))
        }
    };
    if let Some(p) = j.predicted() {
        m.insert("predicted".into(), json!(p));
    }
    if !j.notices.is_empty() {
        m.insert("notices".into(), json!(j.notices));
    }
    if let JobSpec::Upscale(u) = &j.spec {
        m.insert("kind".into(), json!("upscale"));
        m.insert("upscale".into(), u.up.to_json());
        if let Some(p) = j.pipeline() {
            m.insert(
                "pipeline".into(),
                json!([display_args(&p.decode), display_args(&p.worker), display_args(&p.encode)]),
            );
        }
        if let Some(cwd) = &j.scratch {
            m.insert("cwd".into(), json!(lossy(cwd)));
        }
    }
    if let JobSpec::Gif(g) = &j.spec {
        m.insert("kind".into(), json!("gif"));
        m.insert("gif".into(), g.gif.to_json());
        let pipeline: Vec<Value> = j.commands().iter().map(|c| json!(display_args(&c.args))).collect();
        m.insert("pipeline".into(), json!(pipeline));
    }
    v
}

/// `reframe: smart, 2 shots; fallback 00:40–00:44` (contracts/cli.md › Human dry run).
fn reframe_line(
    applied: &rmedia_core::upscale::Reframe,
    analysis: &rmedia_core::upscale::AnalysisState,
) -> String {
    use rmedia_core::upscale::AnalysisState;
    let name = format!("{applied:?}").to_lowercase();
    match analysis {
        AnalysisState::NotNeeded => format!("reframe: {name}"),
        AnalysisState::Pending => format!("reframe: {name} (analysis pending)"),
        AnalysisState::Ready(a, _) => {
            let mmss = |t: rmedia_core::Ts| {
                let s = t.as_secs_f64().round() as u64;
                format!("{:02}:{:02}", s / 60, s % 60)
            };
            let ranges: Vec<String> =
                a.fallback_ranges.iter().map(|r| format!("{}–{}", mmss(r.start), mmss(r.end))).collect();
            let fallback = if ranges.is_empty() {
                String::new()
            } else {
                format!("; blurred background {}", ranges.join(", "))
            };
            format!("reframe: {name}, {} shot(s){fallback}", a.shots.len())
        }
    }
}

fn plan_doc(plan: &Plan) -> Value {
    let issues: Vec<Value> = plan
        .issues
        .iter()
        .map(|i| {
            let mut m = serde_json::Map::new();
            m.insert("severity".into(), json!(i.severity));
            m.insert("code".into(), json!(i.code));
            origin_fields(&i.origin, &mut m);
            m.insert("message".into(), json!(i.message));
            if i.code == rmedia_core::error::IssueCode::BudgetUnreachable {
                // At plan time only the static floor is checked (contracts/json-output.md).
                m.insert("detail".into(), json!({"min_bytes": rmedia_core::gif::plan::MIN_BUDGET}));
            }
            Value::Object(m)
        })
        .collect();
    let skipped = plan.jobs.iter().filter(|j| j.disposition != Disposition::Run).count();
    let errors = plan.issues.iter().filter(|i| i.severity == Severity::Error).count();
    json!({
        "schema": 1,
        "type": "plan",
        "run_id": plan.run_id,
        "jobs": plan.jobs.iter().map(job_doc).collect::<Vec<_>>(),
        "issues": issues,
        "summary": {"planned": plan.jobs.len() - skipped, "skipped": skipped, "errors": errors},
    })
}

/// Event sink for a run.
pub enum RenderSink {
    Json,
    Human { multi: MultiProgress, labels: HashMap<JobId, String>, bars: Mutex<HashMap<JobId, ProgressBar>> },
}

impl std::fmt::Debug for RenderSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RenderSink::Json => "RenderSink::Json",
            RenderSink::Human { .. } => "RenderSink::Human",
        })
    }
}

pub fn sink(out: &Output, plan: &Plan) -> Arc<RenderSink> {
    if out.json {
        return Arc::new(RenderSink::Json);
    }
    let target = if out.quiet { ProgressDrawTarget::hidden() } else { ProgressDrawTarget::stderr() };
    Arc::new(RenderSink::Human {
        multi: MultiProgress::with_draw_target(target),
        labels: plan.jobs.iter().map(|j| (j.id, file_name(&j.output))).collect(),
        bars: Mutex::new(HashMap::new()),
    })
}

impl RenderSink {
    pub fn finish(&self) {
        if let RenderSink::Human { multi, .. } = self {
            let _ = multi.clear();
        }
    }
}

impl EventSink for RenderSink {
    fn emit(&self, event: Event) {
        match self {
            RenderSink::Json => {
                let mut v = match &event {
                    Event::RunFinished { summary } => {
                        let mut v = serde_json::to_value(summary).expect("summary json");
                        v["type"] = json!("summary");
                        v
                    }
                    e => serde_json::to_value(e).expect("event json"),
                };
                v["schema"] = json!(1);
                print_json(&v);
            }
            RenderSink::Human { multi, labels, bars } => {
                let mut bars = bars.lock().expect("bars");
                match event {
                    Event::JobStarted { job_id } => {
                        let pb = multi.add(ProgressBar::new(1000));
                        pb.set_style(
                            ProgressStyle::with_template("{prefix} [{bar:28}] {percent:>3}% {msg}")
                                .expect("template")
                                .progress_chars("=> "),
                        );
                        pb.set_prefix(labels.get(&job_id).cloned().unwrap_or_default());
                        bars.insert(job_id, pb);
                    }
                    Event::JobProgress { job_id, fraction, eta_ms, speed, .. } => {
                        if let Some(pb) = bars.get(&job_id) {
                            pb.set_position((fraction * 1000.0) as u64);
                            let mut msg = String::new();
                            if let Some(s) = speed {
                                msg.push_str(&format!("{s:.1}x "));
                            }
                            if let Some(ms) = eta_ms {
                                msg.push_str(&format!("{}s left", ms.div_ceil(1000)));
                            }
                            pb.set_message(msg);
                        }
                    }
                    Event::JobWarning { job_id, message, .. } => {
                        let label = labels.get(&job_id).cloned().unwrap_or_default();
                        let _ = multi.println(format!("warning: {label}: {message}"));
                    }
                    Event::JobFinished { job_id, outcome } => {
                        if let Some(pb) = bars.remove(&job_id) {
                            let msg = match outcome {
                                JobOutcome::Succeeded { .. } => "done",
                                JobOutcome::Skipped { .. } => "skipped",
                                JobOutcome::Failed { .. } => "failed",
                                JobOutcome::Cancelled => "cancelled",
                            };
                            pb.finish_with_message(msg);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
