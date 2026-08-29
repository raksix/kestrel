//! File and text hashing, as ShareX's "hash check" tool.
//!
//! Reads in chunks rather than loading the file: this gets pointed at ISOs and
//! disk images, and a hash tool that runs out of memory on the files people
//! most want to verify is no use.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use md5::Digest as _;

/// 1 MiB. Large enough that syscall overhead disappears, small enough that
/// hashing a 10 GB image never costs more than this much memory.
const CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub const ALL: [Algorithm; 4] = [
        Algorithm::Md5,
        Algorithm::Sha1,
        Algorithm::Sha256,
        Algorithm::Sha512,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Md5 => "MD5",
            Algorithm::Sha1 => "SHA-1",
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Sha512 => "SHA-512",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, HashError>;

pub fn hash_bytes(algorithm: Algorithm, bytes: &[u8]) -> String {
    let mut hasher = Hasher::new(algorithm);
    hasher.update(bytes);
    hasher.finish()
}

pub fn hash_file(algorithm: Algorithm, path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Hasher::new(algorithm);
    let mut buffer = vec![0u8; CHUNK];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finish())
}

/// Every algorithm at once, which is what the tool actually shows.
pub fn hash_file_all(path: &Path) -> Result<Vec<(Algorithm, String)>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hashers: Vec<(Algorithm, Hasher)> = Algorithm::ALL
        .iter()
        .map(|a| (*a, Hasher::new(*a)))
        .collect();
    let mut buffer = vec![0u8; CHUNK];

    // One pass feeding every hasher, rather than re-reading the file four
    // times. On a large file that is the difference between seconds and
    // minutes.
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for (_, hasher) in hashers.iter_mut() {
            hasher.update(&buffer[..read]);
        }
    }

    Ok(hashers
        .into_iter()
        .map(|(algorithm, hasher)| (algorithm, hasher.finish()))
        .collect())
}

/// Whether a hash matches, compared case-insensitively and ignoring spaces.
///
/// Hashes get pasted from web pages and READMEs, where they arrive in either
/// case and sometimes with stray whitespace. Rejecting those would be pedantry,
/// not safety.
pub fn matches(expected: &str, actual: &str) -> bool {
    let normalise = |value: &str| {
        value
            .chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(|c| c.to_lowercase())
            .collect::<String>()
    };
    let expected = normalise(expected);
    !expected.is_empty() && expected == normalise(actual)
}

enum Hasher {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}

impl Hasher {
    fn new(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Md5 => Hasher::Md5(md5::Md5::new()),
            Algorithm::Sha1 => Hasher::Sha1(sha1::Sha1::new()),
            Algorithm::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
            Algorithm::Sha512 => Hasher::Sha512(sha2::Sha512::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Hasher::Md5(h) => h.update(bytes),
            Hasher::Sha1(h) => h.update(bytes),
            Hasher::Sha256(h) => h.update(bytes),
            Hasher::Sha512(h) => h.update(bytes),
        }
    }

    fn finish(self) -> String {
        fn hex(bytes: impl AsRef<[u8]>) -> String {
            bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
        }
        match self {
            Hasher::Md5(h) => hex(h.finalize()),
            Hasher::Sha1(h) => hex(h.finalize()),
            Hasher::Sha256(h) => hex(h.finalize()),
            Hasher::Sha512(h) => hex(h.finalize()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Published vectors, so a wrong wiring shows up rather than a
    /// self-consistent but incorrect result.
    #[test]
    fn known_vectors_for_the_empty_input() {
        assert_eq!(
            hash_bytes(Algorithm::Md5, b""),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            hash_bytes(Algorithm::Sha1, b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hash_bytes(Algorithm::Sha256, b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn known_vectors_for_abc() {
        assert_eq!(
            hash_bytes(Algorithm::Md5, b"abc"),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hash_bytes(Algorithm::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_bytes(Algorithm::Sha512, b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn hashing_a_file_matches_hashing_its_bytes() {
        let path = std::env::temp_dir().join(format!("kestrel-hash-{}", std::process::id()));
        let contents = b"kestrel";
        std::fs::write(&path, contents).unwrap();

        assert_eq!(
            hash_file(Algorithm::Sha256, &path).unwrap(),
            hash_bytes(Algorithm::Sha256, contents)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_larger_than_one_chunk_hashes_correctly() {
        // The chunked reader is the part most likely to be wrong, and it only
        // shows up past the buffer size.
        let path = std::env::temp_dir().join(format!("kestrel-big-{}", std::process::id()));
        let contents = vec![7u8; CHUNK * 2 + 12345];
        std::fs::write(&path, &contents).unwrap();

        assert_eq!(
            hash_file(Algorithm::Sha256, &path).unwrap(),
            hash_bytes(Algorithm::Sha256, &contents)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn one_pass_over_a_file_gives_the_same_answers_as_four() {
        let path = std::env::temp_dir().join(format!("kestrel-all-{}", std::process::id()));
        std::fs::write(&path, b"kestrel tools").unwrap();

        for (algorithm, digest) in hash_file_all(&path).unwrap() {
            assert_eq!(
                digest,
                hash_file(algorithm, &path).unwrap(),
                "{algorithm:?}"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(hash_file(Algorithm::Md5, Path::new("/no/such/file")).is_err());
    }

    #[test]
    fn comparison_tolerates_how_hashes_are_actually_pasted() {
        let digest = hash_bytes(Algorithm::Sha256, b"abc");
        assert!(matches(&digest.to_uppercase(), &digest));
        assert!(matches(&format!("  {digest}\n"), &digest));
        assert!(!matches(&digest, "deadbeef"));
    }

    #[test]
    fn an_empty_expected_hash_never_matches() {
        // Otherwise leaving the field blank would read as "verified".
        let digest = hash_bytes(Algorithm::Sha256, b"abc");
        assert!(!matches("", &digest));
        assert!(!matches("   ", &digest));
    }

    #[test]
    fn every_algorithm_has_a_display_name() {
        for algorithm in Algorithm::ALL {
            assert!(!algorithm.name().is_empty());
        }
    }
}
