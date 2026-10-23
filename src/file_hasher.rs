use crate::hasheus_error::HasheusError;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

const HASH_LENGTH: usize = 64;
// small enough to report progress often, and large enough to read quickly.
const CHUNK_SIZE_BYTES: usize = 1024 * 1024;

// whether there's a file for the hash, since only a file can be published.
#[derive(Debug)]
pub struct FileHash {
    pub hash: String,
    pub has_file: bool,
}

// reports how many bytes it has hashed after each chunk, since a large file can
// take a while.
pub fn hash(target: &Path, on_progress: impl FnMut(u64)) -> Result<FileHash, HasheusError> {
    // a file that happens to be named like a hash still gets hashed.
    if !target.exists()
        && let Some(hash) = parse_hash(target)
    {
        return Ok(FileHash {
            hash,
            has_file: false,
        });
    }

    let hash = hash_file(target, on_progress).map_err(|cause| HasheusError::ReadFile {
        path: target.to_path_buf(),
        cause,
    })?;

    Ok(FileHash {
        hash,
        has_file: true,
    })
}

fn parse_hash(target: &Path) -> Option<String> {
    let target = target.to_str()?;
    let is_hash =
        target.len() == HASH_LENGTH && target.bytes().all(|byte| byte.is_ascii_hexdigit());

    is_hash.then(|| target.to_ascii_lowercase())
}

fn hash_file(path: &Path, mut on_progress: impl FnMut(u64)) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; CHUNK_SIZE_BYTES];
    let mut hashed_byte_count = 0;

    loop {
        let read_byte_count = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read_byte_count) => read_byte_count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };

        hasher.update(&buffer[..read_byte_count]);
        hashed_byte_count += read_byte_count as u64;
        on_progress(hashed_byte_count);
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn treats_a_hash_that_isnt_a_file_as_the_hash() {
        let file_hash = hash(Path::new(&"AB".repeat(32)), |_| unreachable!()).unwrap();

        assert_eq!(file_hash.hash, "ab".repeat(32));
        assert!(!file_hash.has_file);
    }

    #[test]
    fn hashes_a_file() {
        let path = std::env::temp_dir().join("hasheus_cli_file_hasher_test.txt");
        fs::write(&path, "hello").unwrap();

        let file_hash = hash(&path, |_| {}).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(
            file_hash.hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(file_hash.has_file);
    }

    #[test]
    fn hashes_a_file_in_chunks_and_reports_progress() {
        let path = std::env::temp_dir().join("hasheus_cli_file_hasher_chunks_test.txt");
        fs::write(&path, "a".repeat(2 * CHUNK_SIZE_BYTES + 1)).unwrap();
        let mut progress = Vec::new();

        let hash = hash(&path, |hashed_byte_count| progress.push(hashed_byte_count));
        fs::remove_file(&path).unwrap();

        assert_eq!(
            hash.unwrap().hash,
            "16a429a1e5162fcf59e3079724763e9afafcef1e2c7f862571e9267b3d959113"
        );
        let chunk_size = CHUNK_SIZE_BYTES as u64;
        assert_eq!(progress, [chunk_size, 2 * chunk_size, 2 * chunk_size + 1]);
    }

    #[test]
    fn fails_on_a_missing_file_that_isnt_a_hash() {
        let error = hash(Path::new("missing.iso"), |_| {}).unwrap_err();

        assert!(
            error
                .to_string()
                .starts_with("failed to read missing.iso: ")
        );
    }

    #[test]
    fn doesnt_treat_a_hash_of_the_wrong_length_as_a_hash() {
        assert_eq!(parse_hash(Path::new(&"a".repeat(63))), None);
    }

    #[test]
    fn doesnt_treat_non_hex_as_a_hash() {
        assert_eq!(parse_hash(Path::new(&"g".repeat(64))), None);
    }
}
