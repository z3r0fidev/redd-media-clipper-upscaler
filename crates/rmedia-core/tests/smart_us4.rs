//! Feature 004, US4: smart mode in batches and manifests (contracts/manifest-smart.md).

use std::path::Path;

use rmedia_core::input::{manifest_csv, manifest_json};
use rmedia_core::request::{BatchRequest, Mode, Options, PartialOptions};
use rmedia_core::settings::Settings;

fn resolved(req: &BatchRequest) -> Vec<(Mode, bool)> {
    req.resolved().map(|o| (o.mode, o.smart_fallback)).collect()
}

/// T005: `smart` and `smart_fallback` parse from JSON (three levels), CSV and settings, and layer
/// clip over video over run.
#[test]
fn mode_parses_everywhere() {
    let json = r#"{"version":1,
      "defaults":{"mode":"smart","output_dir":"clips"},
      "videos":[
        {"input":"/v/match.mp4","clips":[
          {"start":"3.32","end":"20.72"},
          {"start":"61","end":"+30","options":{"smart_fallback":false}}]},
        {"input":"/v/phone.mp4","options":{"mode":"accurate"},"clips":[{"start":0,"end":5}]},
        {"input":"/v/x.mkv","options":{"smart_fallback":false},
         "clips":[{"start":0,"end":5},{"start":6,"end":9,"options":{"smart_fallback":true}}]}]}"#;
    let req = manifest_json::parse(Path::new("/v/m.json"), json, &Options::default()).expect("valid");
    assert_eq!(
        resolved(&req),
        [
            (Mode::Smart, true),
            (Mode::Smart, false),
            (Mode::Accurate, true),
            (Mode::Smart, false),
            (Mode::Smart, true)
        ]
    );

    let csv = "input,start,end,mode,smart_fallback\n/v/a.mp4,1,5,SMART,\n/v/a.mp4,6,9,smart,false\n\
               /v/b.mp4,0,3,fast,true\n/v/b.mp4,4,8,,\n";
    let cli = Options { mode: Mode::Accurate, ..Options::default() };
    let req = manifest_csv::parse(Path::new("/v/m.csv"), csv, &cli).expect("valid");
    assert_eq!(
        resolved(&req),
        [(Mode::Smart, true), (Mode::Smart, false), (Mode::Fast, true), (Mode::Accurate, true)]
    );

    let s: Settings =
        serde_json::from_str(r#"{"defaults":{"mode":"smart","smart_fallback":false}}"#).unwrap();
    assert_eq!((s.defaults.mode, s.defaults.smart_fallback), (Some(Mode::Smart), Some(false)));
    let o = s.defaults.apply(&Options::default());
    assert_eq!((o.mode, o.smart_fallback), (Mode::Smart, false));
}

#[test]
fn bad_csv_values_name_every_choice() {
    let csv = "input,start,end,mode,smart_fallback\n/v/a.mp4,1,5,quick,\n/v/a.mp4,1,5,smart,maybe\n";
    let issues = manifest_csv::parse(Path::new("/v/m.csv"), csv, &Options::default()).unwrap_err();
    let messages: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            r#"mode must be fast, accurate or smart, not "quick""#,
            r#"smart_fallback must be true or false, not "maybe""#
        ]
    );
}

/// 0.3.0 inputs keep their meaning: no `smart_fallback` anywhere means the default (fallback on),
/// and options serialize without the new field unless it is `false`.
#[test]
fn inputs_from_0_3_0_are_unchanged() {
    let s: Settings = serde_json::from_str(r#"{"defaults":{"mode":"accurate"},"jobs":2}"#).unwrap();
    assert_eq!(s.defaults, PartialOptions { mode: Some(Mode::Accurate), ..PartialOptions::default() });
    let o = Options::default();
    assert!(o.smart_fallback);
    let v = serde_json::to_value(&o).unwrap();
    assert!(v.get("smart_fallback").is_none(), "{v}");
    let strict = Options { smart_fallback: false, ..Options::default() };
    assert_eq!(serde_json::to_value(&strict).unwrap()["smart_fallback"], false);
    let back: Options = serde_json::from_value(v).unwrap();
    assert_eq!(back, o);

    let csv = "input,start,end,mode\n/v/a.mp4,1,5,accurate\n";
    let req = manifest_csv::parse(Path::new("/v/m.csv"), csv, &Options::default()).unwrap();
    assert_eq!(resolved(&req), [(Mode::Accurate, true)]);
    assert_eq!(manifest_csv::write(&req).lines().next(), Some("input,start,end,duration,name,mode,output"));
}

fn from_json(text: &str) -> BatchRequest {
    manifest_json::parse(Path::new("/m/jobs.json"), text, &Options::default()).unwrap()
}

/// T044 (contracts/manifest-smart.md › CSV): the writer adds `smart_fallback`, right after
/// `mode`, only when a row sets it; plain exports keep the 0.3.0 header; write → read gives the
/// same options for every clip.
#[test]
fn csv_writer_round_trip() {
    let strict = from_json(
        r#"{"version":1,"defaults":{"mode":"smart"},"videos":[
            {"input":"/v/match.mp4","clips":[{"start":"3.32","end":"20.72"},
                {"start":"61","end":"+30","options":{"smart_fallback":false}}]},
            {"input":"/v/phone.mp4","options":{"mode":"accurate"},"clips":[{"start":0,"end":5}]}]}"#,
    );
    let text = manifest_csv::write(&strict);
    assert!(text.starts_with("input,start,end,duration,name,mode,smart_fallback,output\n"), "{text}");
    let back = manifest_csv::parse(Path::new("/m/jobs.csv"), &text, &Options::default()).unwrap();
    let modes: Vec<(Mode, bool)> = back.resolved().map(|o| (o.mode, o.smart_fallback)).collect();
    assert_eq!(modes, [(Mode::Smart, true), (Mode::Smart, false), (Mode::Accurate, true)]);
    assert_eq!(back.resolved().collect::<Vec<_>>(), strict.resolved().collect::<Vec<_>>());

    // Smart without strict rows, and a 0.3.0 manifest: the header stays as it was.
    for json in [
        r#"{"version":1,"defaults":{"mode":"smart"},"videos":[{"input":"/v/a.mp4","clips":[{"start":1,"end":5}]}]}"#,
        r#"{"version":1,"videos":[{"input":"/v/a.mp4","clips":[{"start":1,"end":5}]}]}"#,
    ] {
        let req = from_json(json);
        let text = manifest_csv::write(&req);
        assert!(text.starts_with("input,start,end,duration,name,mode,output\n"), "{text}");
        let back = manifest_csv::parse(Path::new("/m/jobs.csv"), &text, &Options::default()).unwrap();
        assert_eq!(back.resolved().collect::<Vec<_>>(), req.resolved().collect::<Vec<_>>());
    }
}
