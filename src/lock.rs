use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use anyhow::{Context, Result};

pub struct FileLock {
    file: std::fs::File,
}

impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("Failed to open lock file {:?}", path))?;

        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(Self { file })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
