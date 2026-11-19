//! Parsers that turn user files into requests. Each reports every problem at once with its origin.

pub mod cliplist;
pub mod manifest_csv;
pub mod manifest_json;
pub mod pathlist;

use std::io::Read;
use std::path::Path;

/// The largest clip list, manifest or path list read (security L4): far above any real one
/// (a 16 MiB clip list holds about 400,000 clips), and it bounds memory for a hostile file.
pub const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Reads a user-supplied input file, refusing one larger than [`MAX_INPUT_BYTES`]. The read
/// itself is bounded too, for files that report no size or grow while they are read.
pub fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let too_big = || format!("the file is larger than 16 MiB ({})", path.display());
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_INPUT_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(too_big());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use crate::request::Options;

    /// Security L4: clip lists and manifests are read up to `MAX_INPUT_BYTES`; a bigger file is
    /// refused with a message instead of being read whole.
    #[test]
    fn oversized_lists_and_manifests_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let big = |name: &str| {
            let p = d.path().join(name);
            std::fs::File::create(&p).unwrap().set_len(super::MAX_INPUT_BYTES + 1).unwrap();
            p
        };
        let said =
            |issues: Vec<crate::plan::Issue>| issues.into_iter().map(|i| i.message).collect::<Vec<_>>();
        let too_big = |m: &[String]| m.len() == 1 && m[0].contains("larger than 16 MiB");
        let m = said(super::cliplist::load(&big("clips.txt")).unwrap_err());
        assert!(too_big(&m), "{m:?}");
        let m = said(super::manifest_json::load(&big("m.json"), &Options::default()).unwrap_err());
        assert!(too_big(&m), "{m:?}");
        let m = said(super::manifest_csv::load(&big("m.csv"), &Options::default()).unwrap_err());
        assert!(too_big(&m), "{m:?}");
    }

    /// A file that never ends (or grows while it is read) stops at the cap too.
    #[cfg(unix)]
    #[test]
    fn an_endless_file_stops_at_the_cap() {
        let err = super::read_capped(std::path::Path::new("/dev/zero")).unwrap_err();
        assert!(err.contains("larger than 16 MiB"), "{err}");
    }
}
