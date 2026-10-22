//! rmedia core: all media logic shared by the CLI and the desktop app.

pub mod discover;
pub mod doctor;
pub mod error;
pub mod event;
pub mod exec;
pub mod input;
pub mod models;
pub mod paths;
pub mod pathser;
pub mod plan;
pub mod preset;
pub mod preview;
pub mod probe;
pub mod request;
pub mod settings;
pub mod time;
pub mod tools;
pub mod upscale;

pub use error::{Error, ErrorInfo, Result};
pub use time::{EndSpec, Ts};

/// Lower-case hex of `bytes` (digests print this way in caches, fingerprints and pins).
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn hex_of_a_sha256_digest_matches_the_standard_vector() {
        assert_eq!(
            super::hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(super::hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
