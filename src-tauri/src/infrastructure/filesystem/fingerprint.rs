use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub size_bytes: u64,
    pub modified_nanos: Option<u128>,
    pub blake3: String,
}

pub fn fingerprint_file(path: &Path) -> io::Result<FileFingerprint> {
    let metadata_before = fs::metadata(path)?;
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let metadata_after = fs::metadata(path)?;
    let before_modified = metadata_before.modified().ok();
    let after_modified = metadata_after.modified().ok();
    if metadata_before.len() != metadata_after.len() || before_modified != after_modified {
        return Err(io::Error::other("file changed while fingerprinting"));
    }
    Ok(FileFingerprint {
        size_bytes: metadata_after.len(),
        modified_nanos: metadata_after
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos()),
        blake3: hasher.finalize().to_hex().to_string(),
    })
}

pub fn fingerprint_from_bytes(bytes: &[u8], metadata: &fs::Metadata) -> FileFingerprint {
    FileFingerprint {
        size_bytes: metadata.len(),
        modified_nanos: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos()),
        blake3: blake3::hash(bytes).to_hex().to_string(),
    }
}
