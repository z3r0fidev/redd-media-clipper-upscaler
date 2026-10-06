//! US4 acceptance scenarios: discover real videos in a folder tree.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use rmedia_core::discover::{RejectReason, ScanOptions, ScanResult, scan};
use rmedia_core::{Error, Result};
use rmedia_testkit::{Fixture, fixture, require_ffmpeg};
use tokio_util::sync::CancellationToken;

/// root/
///   a.mp4  d.ts  fake.mp4(text)  photo.mp4(jpeg)  cover.mp4(audio+art)  notes.txt  pack.zip
///   .hidden.mp4  dl.mp4.part  .x.rmedia-r-1.part
///   sub/b.mkv   sub/deep/c.webm   .cache/e.mp4   [loop -> root (unix)]
fn tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let cp = |f: Fixture, rel: &str| {
        let dst = r.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(fixture(f), dst).unwrap();
    };
    cp(Fixture::Gop50Mp4, "a.mp4");
    cp(Fixture::OffsetTs, "d.ts");
    cp(Fixture::MultiMkv, "sub/b.mkv");
    cp(Fixture::SmallWebm, "sub/deep/c.webm");
    cp(Fixture::FakeMp4, "fake.mp4");
    cp(Fixture::PhotoMp4, "photo.mp4");
    cp(Fixture::CoverArtM4a, "cover.mp4");
    cp(Fixture::NoAudioMp4, ".hidden.mp4");
    cp(Fixture::NoAudioMp4, ".cache/e.mp4");
    cp(Fixture::NoAudioMp4, "dl.mp4.part");
    cp(Fixture::NoAudioMp4, ".x.rmedia-r-1.part");
    std::fs::write(r.join("notes.txt"), "hello").unwrap();
    std::fs::write(r.join("pack.zip"), b"PK\x03\x04 not opened").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(r, r.join("sub/loop")).unwrap();
    d
}

async fn run(root: &Path, opts: ScanOptions) -> Result<ScanResult> {
    let (tools, _) = engine();
    scan(&tools, &[root.to_path_buf()], &opts, &|_| {}, &CancellationToken::new()).await
}

fn names(r: &ScanResult) -> Vec<String> {
    r.videos.iter().map(|v| v.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn finds_exactly_the_real_videos() {
    require_ffmpeg!();
    let d = tree();
    let r = run(d.path(), ScanOptions::default()).await.unwrap();
    let mut got = names(&r);
    got.sort();
    assert_eq!(got, ["a.mp4", "b.mkv", "c.webm", "d.ts"]);
    let reason = |n: &str| r.rejected.iter().find(|x| x.path.ends_with(n)).map(|x| x.reason);
    assert_eq!(reason("fake.mp4"), Some(RejectReason::Unreadable));
    assert_eq!(reason("photo.mp4"), Some(RejectReason::Image));
    assert_eq!(reason("cover.mp4"), Some(RejectReason::NoVideoStream));
    assert_eq!(r.rejected.len(), 3, "{:?}", r.rejected);
    assert_eq!(r.skipped.temp, 2);
    assert!(r.skipped.hidden >= 2, "{:?}", r.skipped); // .hidden.mp4 and .cache/
    #[cfg(unix)]
    assert_eq!(r.skipped.symlink, 1);
    let a = &r.videos.iter().find(|v| v.path.ends_with("a.mp4")).unwrap();
    assert_eq!(a.first_video().unwrap().width, Some(320));
}

#[tokio::test(flavor = "multi_thread")]
async fn options_filter_the_walk() {
    require_ffmpeg!();
    let d = tree();
    let sorted = |r: ScanResult| {
        let mut n = names(&r);
        n.sort();
        n
    };
    let flat = run(d.path(), ScanOptions { no_recursive: true, ..Default::default() }).await.unwrap();
    assert_eq!(sorted(flat), ["a.mp4", "d.ts"]);
    let mkv =
        run(d.path(), ScanOptions { include: vec!["*.MKV".into()], ..Default::default() }).await.unwrap();
    assert_eq!(sorted(mkv), ["b.mkv"]);
    let no_deep = run(d.path(), ScanOptions { exclude: vec!["**/deep/**".into()], ..Default::default() })
        .await
        .unwrap();
    assert_eq!(sorted(no_deep), ["a.mp4", "b.mkv", "d.ts"]);
    let hidden = run(d.path(), ScanOptions { hidden: true, ..Default::default() }).await.unwrap();
    assert_eq!(sorted(hidden), [".hidden.mp4", "a.mp4", "b.mkv", "c.webm", "d.ts", "e.mp4"]);
    let depth1 = run(d.path(), ScanOptions { max_depth: Some(2), ..Default::default() }).await.unwrap();
    assert_eq!(sorted(depth1), ["a.mp4", "b.mkv", "d.ts"]);
    assert!(matches!(
        run(d.path(), ScanOptions { include: vec!["a[".into()], ..Default::default() }).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn file_roots_and_duplicates() {
    require_ffmpeg!();
    let d = tree();
    let (tools, _) = engine();
    let roots: Vec<PathBuf> =
        vec![d.path().join("a.mp4"), d.path().to_path_buf(), d.path().join("notes.txt")];
    let r = scan(&tools, &roots, &ScanOptions::default(), &|_| {}, &CancellationToken::new()).await.unwrap();
    assert_eq!(r.videos.iter().filter(|v| v.path.ends_with("a.mp4")).count(), 1, "deduplicated");
    assert!(r.rejected.iter().any(|x| x.path.ends_with("notes.txt")), "explicit non-video file is reported");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_stops_the_scan() {
    require_ffmpeg!();
    let d = tree();
    let (tools, _) = engine();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = scan(&tools, &[d.path().to_path_buf()], &ScanOptions::default(), &|_| {}, &cancel).await;
    assert!(matches!(r, Err(Error::Cancelled)));
}
