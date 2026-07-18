use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

// ---------------------------------------------------------------------------
// ObjectStore trait
// ---------------------------------------------------------------------------

/// Abstraction over an object store (local filesystem or S3-compatible).
///
/// All URIs use the `s3://bucket/key` scheme regardless of the backing
/// implementation; the local implementation maps each bucket to a directory
/// under `root`.
pub(crate) trait ObjectStore: Send + Sync {
    /// Read the full content of an object.
    fn get(&self, uri: &str) -> Result<Vec<u8>>;

    /// Write `bytes` atomically (temp-rename) so partial writes are not
    /// visible to concurrent readers.
    fn put_atomic(&self, uri: &str, bytes: &[u8]) -> Result<()>;

    /// List all objects whose URI starts with `prefix_uri` and whose name
    /// ends with `include_suffix`, sorted lexicographically.
    fn list(&self, prefix_uri: &str, include_suffix: &str) -> Result<Vec<String>>;
}

// ---------------------------------------------------------------------------
// LocalObjectStore
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    #[allow(dead_code)]
    pub fn put(&self, uri: &str, bytes: &[u8]) -> Result<()> {
        let path = self.local_path(uri)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes).with_context(|| format!("failed to write {uri}"))
    }

    pub fn local_path_for_export(&self, uri: &str) -> Result<PathBuf> {
        self.local_path(uri)
    }

    #[allow(dead_code)]
    pub fn local_path_for_test(&self, uri: &str) -> Result<PathBuf> {
        self.local_path(uri)
    }

    fn local_path(&self, uri: &str) -> Result<PathBuf> {
        let Some(rest) = uri.strip_prefix("s3://") else {
            bail!("unsupported object URI scheme for {uri}; expected s3://");
        };
        let mut parts = rest.split('/');
        let bucket = parts.next().unwrap_or_default();
        if bucket.is_empty() || bucket == "." || bucket == ".." {
            bail!("invalid object URI bucket in {uri}");
        }

        let mut path = self.root.clone();
        path.push(bucket);
        for part in parts {
            if part.is_empty() {
                continue;
            }
            if part == "." || part == ".." || part.contains('\\') {
                bail!("invalid object URI path segment in {uri}");
            }
            path.push(part);
        }
        Ok(path)
    }

    fn list_inner(
        prefix_uri: &str,
        dir: &Path,
        include_suffix: &str,
        uris: &mut Vec<String>,
    ) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                Self::list_inner(&format!("{prefix_uri}/{name}"), &path, include_suffix, uris)?;
            } else if name.ends_with(include_suffix) {
                uris.push(format!("{prefix_uri}/{name}"));
            }
        }
        Ok(())
    }
}

impl ObjectStore for LocalObjectStore {
    fn get(&self, uri: &str) -> Result<Vec<u8>> {
        fs::read(self.local_path(uri)?).with_context(|| format!("failed to read {uri}"))
    }

    fn put_atomic(&self, uri: &str, bytes: &[u8]) -> Result<()> {
        let path = self.local_path(uri)?;
        write_file_atomic(&path, bytes).with_context(|| format!("failed to write {uri}"))
    }

    fn list(&self, prefix_uri: &str, include_suffix: &str) -> Result<Vec<String>> {
        let prefix_path = self.local_path(prefix_uri)?;
        let mut uris = Vec::new();
        if !prefix_path.exists() {
            return Ok(uris);
        }
        Self::list_inner(
            prefix_uri.trim_end_matches('/'),
            &prefix_path,
            include_suffix,
            &mut uris,
        )?;
        uris.sort();
        Ok(uris)
    }
}

// ---------------------------------------------------------------------------
// Atomic file write
// ---------------------------------------------------------------------------

pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_file_atomic_with(path, |temp_path| {
        fs::write(temp_path, bytes)
            .with_context(|| format!("failed to write temporary output {}", temp_path.display()))
    })
}

pub fn write_file_atomic_with(
    path: &Path,
    write_temp: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .context("output path must include a valid file name")?;
    let temp_path = parent.join(format!(
        ".{filename}.swiftpipe-tmp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_nanos()
    ));

    write_temp(&temp_path)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error)
            .with_context(|| format!("failed to move temporary output to {}", path.display()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Artifact GC
// ---------------------------------------------------------------------------

/// Delete outbox job directories whose last-modified time is older than
/// `max_age`.  Only removes directories directly under
/// `<object_root>/swiftpipe-outbox/jobs/`.
///
/// Returns the number of directories removed.
pub(crate) fn gc_outbox_jobs(object_root: &Path, max_age: Duration) -> usize {
    let jobs_dir = object_root.join("swiftpipe-outbox").join("jobs");
    if !jobs_dir.exists() {
        return 0;
    }
    let cutoff = SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    let entries = match fs::read_dir(&jobs_dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(error = %err, "gc: failed to read outbox jobs dir");
            return 0;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let mtime = path
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if mtime < cutoff {
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    tracing::info!(path = %path.display(), "gc: removed old job directory");
                    removed += 1;
                }
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "gc: failed to remove job directory");
                }
            }
        }
    }
    removed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn local_object_store_maps_s3_uris_under_root() {
        let root = temp_root("swiftpipe-api-store");
        let store = LocalObjectStore::new(&root);
        store
            .put("s3://bucket/key/file.fin", b"hello")
            .expect("put");
        assert_eq!(
            store.get("s3://bucket/key/file.fin").expect("get"),
            b"hello"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn local_object_store_rejects_path_traversal() {
        let root = temp_root("swiftpipe-api-store-traversal");
        let store = LocalObjectStore::new(&root);

        for uri in [
            "s3://bucket/../escape.fin",
            "s3://bucket/a/../../escape.fin",
            "s3://../bucket/file.fin",
            "file:///tmp/file.fin",
        ] {
            assert!(store.put(uri, b"bad").is_err(), "{uri} should be rejected");
        }
    }

    #[test]
    fn atomic_write_replaces_existing_file_and_removes_temp() {
        let root = temp_root("swiftpipe-api-atomic");
        fs::create_dir_all(&root).expect("root");
        let output = root.join("output.fin");
        fs::write(&output, b"old").expect("old");

        write_file_atomic(&output, b"new").expect("atomic write");

        assert_eq!(fs::read(&output).expect("read"), b"new");
        assert!(fs::read_dir(&root).expect("read dir").all(|entry| !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .contains("swiftpipe-tmp")));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn gc_removes_old_job_directories() {
        let root = temp_root("swiftpipe-gc-test");
        let jobs_dir = root.join("swiftpipe-outbox").join("jobs");
        fs::create_dir_all(&jobs_dir).expect("jobs dir");

        // Create two job dirs; one old (simulate by using a very short max_age).
        fs::create_dir(jobs_dir.join("old-job")).expect("old-job dir");
        fs::create_dir(jobs_dir.join("new-job")).expect("new-job dir");

        // Zero-duration max_age means everything is old.
        let removed = gc_outbox_jobs(&root, Duration::from_secs(0));
        assert_eq!(removed, 2, "both job dirs should be removed");
        assert!(!jobs_dir.join("old-job").exists());
        assert!(!jobs_dir.join("new-job").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }
}
