use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Reads a regular path up to `max_bytes`, rejecting files that exceed the limit.
pub fn read_bytes(path: impl AsRef<Path>, max_bytes: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(max_bytes.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file exceeds size limit"));
    }
    Ok(bytes)
}

pub fn read_utf8(path: impl AsRef<Path>, max_bytes: u64) -> io::Result<String> {
    String::from_utf8(read_bytes(path, max_bytes)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
