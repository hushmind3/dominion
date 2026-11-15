//! Test-only cleanup that preserves assertions on product behavior.
//! A still-open SQLite handle is released when its Rust AppState drops at
//! the end of the test. Windows forbids unlinking that file during the
//! test even though the durable DB observations have already passed.
use std::path::Path;
pub(crate) fn cleanup_dir(path: impl AsRef<Path>) {
    match std::fs::remove_dir_all(path.as_ref()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // ERROR_SHARING_VIOLATION: expected only when another fixture-owned
        // DB handle is still live. Other I/O errors remain test failures.
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(32) => {}
        Err(e) => panic!("test fixture cleanup failed at {}: {e}",path.as_ref().display()),
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn teardown_removes_unlocked_fixture() {
        let dir=std::env::temp_dir().join(format!("dominion-test-cleanup-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("observed.txt"),"proof").unwrap();
        super::cleanup_dir(&dir);
        assert!(!dir.exists());
    }
}
