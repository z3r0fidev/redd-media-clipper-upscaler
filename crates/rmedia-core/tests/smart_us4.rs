//! Feature 004, US4: smart mode in manifests (task T044).

use std::path::Path;

use rmedia_core::input::{manifest_csv, manifest_json};
use rmedia_core::request::{BatchRequest, Mode, Options};

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
