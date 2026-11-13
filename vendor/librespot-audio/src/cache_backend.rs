//! Optional application cache adapter. Imported files are never removed here.
use librespot_core::FileId;
use std::{
    io::{self, Read, Seek},
    sync::{Arc, OnceLock},
};

pub trait CachedReader: Read + Seek + Send + Sync {
    fn byte_len(&self) -> u64;
}
pub trait CacheBackend: Send + Sync {
    fn open(&self, file: FileId) -> io::Result<Option<Box<dyn CachedReader>>>;
    /// Reject for this process only. Implementations must not delete imported data.
    fn reject(&self, file: FileId);
}
static BACKEND: OnceLock<Arc<dyn CacheBackend>> = OnceLock::new();
pub fn set_cache_backend(backend: Arc<dyn CacheBackend>) -> bool {
    BACKEND.set(backend).is_ok()
}
pub(crate) fn backend() -> Option<&'static Arc<dyn CacheBackend>> {
    BACKEND.get()
}
pub fn reject_cached_file(file: FileId) {
    if let Some(backend) = backend() {
        backend.reject(file);
    }
}
