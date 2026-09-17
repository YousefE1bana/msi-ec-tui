//! Generic safe reads of sysfs-like files. Reads only; no filesystem writes.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Maximum bytes accepted for one textual sysfs value.
pub const MAX_SYSFS_VALUE_BYTES: usize = 4096;

/// The maximum read is one byte above the limit so oversize is detectable.
const READ_CHUNK_LIMIT: usize = MAX_SYSFS_VALUE_BYTES + 1;

#[derive(Debug, Error)]
pub enum SysfsError {
    #[error("sysfs path not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("sysfs path denied: {}", .0.display())]
    PermissionDenied(PathBuf),
    #[error("sysfs value is not valid UTF-8: {}", .0.display())]
    InvalidUtf8(PathBuf),
    #[error("sysfs value is invalid at {path}: {value:?}")]
    InvalidValue { path: PathBuf, value: String },
    #[error("sysfs value exceeds {} bytes: {}", MAX_SYSFS_VALUE_BYTES, .0.display())]
    TooLarge(PathBuf),
    #[error("sysfs value contains NUL: {}", .0.display())]
    EmbeddedNul(PathBuf),
    #[error("sysfs I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Reads only up to the documented byte limit before any large allocation.
fn read_bounded(path: &Path) -> Result<Vec<u8>, SysfsError> {
    let file = fs::File::open(path).map_err(|error| map_io_error(path, error))?;
    let mut buffer = Vec::with_capacity(128);
    file.take(READ_CHUNK_LIMIT as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| map_io_error(path, error))?;
    Ok(buffer)
}

fn map_io_error(path: &Path, error: std::io::Error) -> SysfsError {
    match error.kind() {
        std::io::ErrorKind::NotFound => SysfsError::NotFound(path.to_path_buf()),
        std::io::ErrorKind::PermissionDenied => SysfsError::PermissionDenied(path.to_path_buf()),
        _ => SysfsError::Io {
            path: path.to_path_buf(),
            source: error,
        },
    }
}

fn read_string_impl(path: &Path) -> Result<String, SysfsError> {
    let bytes = read_bounded(path)?;
    if bytes.len() > MAX_SYSFS_VALUE_BYTES {
        return Err(SysfsError::TooLarge(path.to_path_buf()));
    }
    if bytes.contains(&0) {
        return Err(SysfsError::EmbeddedNul(path.to_path_buf()));
    }
    let text = String::from_utf8(bytes).map_err(|_| SysfsError::InvalidUtf8(path.to_path_buf()))?;
    Ok(text.trim_end_matches(['\n', '\r', ' ', '\t']).to_owned())
}

/// Parses a complete numeric sysfs value from a single bounded read.
fn read_numeric<T>(path: &Path) -> Result<T, SysfsError>
where
    T: std::str::FromStr,
{
    let value = read_string_impl(path)?;
    value.parse().map_err(|_| SysfsError::InvalidValue {
        path: path.to_path_buf(),
        value,
    })
}

pub trait SysfsReader {
    fn exists(&self, path: &Path) -> bool;

    fn read_string(&self, path: &Path) -> Result<String, SysfsError>;

    fn read_u8(&self, path: &Path) -> Result<u8, SysfsError>;

    fn read_u16(&self, path: &Path) -> Result<u16, SysfsError>;

    fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxSysfsReader;

impl SysfsReader for LinuxSysfsReader {
    fn exists(&self, path: &Path) -> bool {
        fs::exists(path).unwrap_or(false)
    }

    fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
        read_string_impl(path)
    }

    fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
        read_numeric(path)
    }

    fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
        read_numeric(path)
    }

    fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
        let entries = fs::read_dir(path).map_err(|error| map_io_error(path, error))?;
        let mut directories = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| map_io_error(path, error))?;
            let entry_path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| map_io_error(&entry_path, error))?;
            if file_type.is_dir() {
                directories.push(entry_path);
            }
        }
        directories.sort();
        Ok(directories)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::*;

    fn reader() -> LinuxSysfsReader {
        LinuxSysfsReader
    }

    fn write_file(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn exists_true_for_existing_file() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "present", b"1");
        assert!(reader().exists(&path));
    }

    #[test]
    fn exists_false_for_missing_path() {
        let dir = tempdir().unwrap();
        assert!(!reader().exists(&dir.path().join("missing")));
    }

    #[test]
    fn read_string_plain_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "mode", b"auto");
        assert_eq!(reader().read_string(&path).unwrap(), "auto");
    }

    #[test]
    fn read_string_trims_trailing_newline() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"auto\n");
        assert_eq!(reader().read_string(&path).unwrap(), "auto");
    }

    #[test]
    fn read_string_trims_trailing_crlf() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"auto\r\n");
        assert_eq!(reader().read_string(&path).unwrap(), "auto");
    }

    #[test]
    fn read_string_trims_trailing_spaces_and_tabs() {
        let dir = tempdir().unwrap();
        for contents in [b"auto \t".as_slice(), b"auto \r\n".as_slice()] {
            let path = write_file(dir.path(), "value", contents);
            assert_eq!(reader().read_string(&path).unwrap(), "auto");
        }
    }

    #[test]
    fn read_string_preserves_leading_whitespace() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b" auto\n");
        assert_eq!(reader().read_string(&path).unwrap(), " auto");
    }

    #[test]
    fn read_string_accepts_exactly_max_bytes() {
        let dir = tempdir().unwrap();
        let contents = vec![b'a'; MAX_SYSFS_VALUE_BYTES];
        let path = write_file(dir.path(), "value", &contents);
        assert_eq!(
            reader().read_string(&path).unwrap(),
            String::from_utf8(contents).unwrap()
        );
    }

    #[test]
    fn read_string_rejects_oversized_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", &[b'a'; MAX_SYSFS_VALUE_BYTES + 1]);
        match reader().read_string(&path) {
            Err(SysfsError::TooLarge(reported)) => assert_eq!(reported, path),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_string_rejects_embedded_nul() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"auto\0value");
        match reader().read_string(&path) {
            Err(SysfsError::EmbeddedNul(reported)) => assert_eq!(reported, path),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_string_rejects_invalid_utf8() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", &[0xff, 0xfe]);
        match reader().read_string(&path) {
            Err(SysfsError::InvalidUtf8(reported)) => assert_eq!(reported, path),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_string_missing_file_is_not_found() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        match reader().read_string(&missing) {
            Err(SysfsError::NotFound(reported)) => assert_eq!(reported, missing),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_string_allows_empty_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"");
        assert_eq!(reader().read_string(&path).unwrap(), "");
    }

    #[test]
    fn read_u8_parses_valid_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"63\n");
        assert_eq!(reader().read_u8(&path).unwrap(), 63);
    }

    #[test]
    fn read_u8_rejects_overflow() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"256");
        match reader().read_u8(&path) {
            Err(SysfsError::InvalidValue {
                path: reported,
                value,
            }) => {
                assert_eq!(reported, path);
                assert_eq!(value, "256");
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_u8_rejects_negative_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"-1");
        match reader().read_u8(&path) {
            Err(SysfsError::InvalidValue { value, .. }) => assert_eq!(value, "-1"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_u8_rejects_partial_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"12abc");
        match reader().read_u8(&path) {
            Err(SysfsError::InvalidValue { value, .. }) => assert_eq!(value, "12abc"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_u16_parses_valid_values() {
        let dir = tempdir().unwrap();
        for (contents, expected) in [(b"0".as_slice(), 0), (b"150\n".as_slice(), 150)] {
            let path = write_file(dir.path(), "value", contents);
            assert_eq!(reader().read_u16(&path).unwrap(), expected);
        }
    }

    #[test]
    fn read_u16_rejects_overflow() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"65536");
        match reader().read_u16(&path) {
            Err(SysfsError::InvalidValue { value, .. }) => assert_eq!(value, "65536"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn read_u16_rejects_empty_value() {
        let dir = tempdir().unwrap();
        let path = write_file(dir.path(), "value", b"");
        assert!(matches!(
            reader().read_u16(&path),
            Err(SysfsError::InvalidValue { .. })
        ));
    }

    #[test]
    fn list_dirs_returns_only_immediate_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("fan")).unwrap();
        fs::create_dir(dir.path().join("fan").join("nested")).unwrap();
        write_file(dir.path(), "fw_version", b"1.0");
        assert_eq!(
            reader().list_dirs(dir.path()).unwrap(),
            vec![dir.path().join("fan")]
        );
    }

    #[test]
    fn list_dirs_excludes_files() {
        let dir = tempdir().unwrap();
        write_file(dir.path(), "fw_version", b"1.0");
        write_file(dir.path(), "other", b"x");
        assert!(reader().list_dirs(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn list_dirs_sorts_deterministically() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("b_dir")).unwrap();
        fs::create_dir(dir.path().join("a_dir")).unwrap();
        write_file(dir.path(), "aaa.txt", b"x");
        assert_eq!(
            reader().list_dirs(dir.path()).unwrap(),
            vec![dir.path().join("a_dir"), dir.path().join("b_dir")]
        );
    }

    #[test]
    fn list_dirs_missing_root_is_not_found() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");
        match reader().list_dirs(&missing) {
            Err(SysfsError::NotFound(reported)) => assert_eq!(reported, missing),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn list_dirs_ignores_symlinked_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("real_dir")).unwrap();
        symlink(dir.path().join("real_dir"), dir.path().join("linked_dir")).unwrap();
        symlink(
            dir.path().join("fw_version"),
            dir.path().join("linked_file"),
        )
        .unwrap();
        write_file(dir.path(), "fw_version", b"1.0");
        assert_eq!(
            reader().list_dirs(dir.path()).unwrap(),
            vec![dir.path().join("real_dir")]
        );
    }
}
