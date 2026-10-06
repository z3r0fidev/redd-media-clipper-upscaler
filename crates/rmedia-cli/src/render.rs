//! Output rendering: human (progress bars on stderr, results on stdout) or `--json`
//! (contracts/json-output.md). Every JSON document carries `"schema": 1`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use rmedia_core::event::{Event, EventSink, JobId, JobOutcome, RunSummary};
use rmedia_core::plan::ffargs::display_args;
use rmedia_core::plan::{Disposition, Plan, PlannedJob, Severity};
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
            println!(
                "     from {}  {} → {}  [{}]",
                j.source.display(),
                j.requested.start,
                j.requested.end,
                format!("{:?}", j.mode).to_lowercase()
            );
            if let Some(p) = j.predicted
                && p.start != j.requested.start
            {
                println!(
                    "     fast mode starts at the keyframe at {} ({} early)",
                    p.start,
                    j.requested.start - p.start
                );
            }
            for w in &j.warnings {
                println!("     warning: {w}");
            }
            if self.verbose > 0 {
                println!("     ffmpeg {}", shell_words(&display_args(&j.args())));
            }
        }
        let skipped = plan.jobs.iter().filter(|j| j.disposition != Disposition::Run).count();
        println!(
            "{} clip(s) planned, {skipped} would be skipped. Nothing was written (dry run).",
            plan.jobs.len() - skipped
        );
    }

    pub fn summary(&self, s: &RunSummary) {
        if self.json {
            return; // the summary line was streamed by the sink
        }
        for j in &s.jobs {
            match &j.outcome {
                JobOutcome::Succeeded { output, actual, adjusted } => {
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

fn print_json(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, v);
    let _ = writeln!(out);
}

fn origin_fields(o: &Origin, v: &mut serde_json::Map<String, Value>) {
    match o {
        Origin::Line { file, line } => {
            v.insert("file".into(), json!(file));
            v.insert("line".into(), json!(line));
        }
        Origin::JsonPath { file, path } => {
            v.insert("file".into(), json!(file));
            v.insert("json_path".into(), json!(path));
        }
        Origin::Cli | Origin::Gui => {}
    }
}

fn job_doc(j: &PlannedJob) -> Value {
    let mut v = json!({
        "id": j.id,
        "source": j.source,
        "output": j.output,
        "mode": j.mode,
        "requested": j.requested,
        "warnings": j.warnings,
        "argv": display_args(&j.args()),
    });
    let m = v.as_object_mut().expect("object");
    match &j.disposition {
        Disposition::Run => m.insert("status".into(), json!("planned")),
        Disposition::Skip { reason } => {
            m.insert("skip_reason".into(), json!(reason));
            m.insert("status".into(), json!("skip"))
        }
    };
    if let Some(p) = j.predicted {
        m.insert("predicted".into(), json!(p));
    }
    v
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
                    Event::JobWarning { job_id, message } => {
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
