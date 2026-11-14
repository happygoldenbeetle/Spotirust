//! Optional application cache adapter. Imported files are never removed here.
use librespot_core::FileId;
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, OnceLock},
};

pub trait CachedReader: Read + Seek + Send + Sync {
    fn byte_len(&self) -> u64;
}
pub trait PartialReader: CachedReader {
    /// Logical byte ranges containing verified cache chunks, as (start, length).
    fn available_ranges(&self) -> Vec<(u64, u64)>;
}
pub trait CacheSource: Read + Seek {}
impl<T: Read + Seek> CacheSource for T {}
pub trait CacheBackend: Send + Sync {
    fn open(&self, file: FileId) -> io::Result<Option<Box<dyn CachedReader>>>;
    fn partial(&self, _file: FileId) -> io::Result<Option<Box<dyn PartialReader>>> {
        Ok(None)
    }
    /// Reject for this process only. Implementations must not delete imported data.
    fn reject(&self, file: FileId);
    fn writable(&self) -> bool {
        false
    }
    fn store(&self, _file: FileId, _source: &mut dyn CacheSource) -> io::Result<()> {
        Ok(())
    }
    fn store_partial(
        &self,
        _file: FileId,
        _source: &mut dyn CacheSource,
        _ranges: &[(u64, u64)],
    ) -> io::Result<()> {
        Ok(())
    }
}
static BACKEND: OnceLock<Arc<dyn CacheBackend>> = OnceLock::new();
pub fn set_cache_backend(backend: Arc<dyn CacheBackend>) -> bool {
    BACKEND.set(backend).is_ok()
}
pub(crate) fn backend() -> Option<&'static Arc<dyn CacheBackend>> {
    BACKEND.get()
}
pub fn open_cached_file(file: FileId) -> io::Result<Option<Box<dyn CachedReader>>> {
    match backend() {
        Some(backend) => backend.open(file),
        None => Ok(None),
    }
}
pub fn open_partial_cached_file(file: FileId) -> io::Result<Option<Box<dyn PartialReader>>> {
    match backend() {
        Some(backend) => backend.partial(file),
        None => Ok(None),
    }
}
pub fn reject_cached_file(file: FileId) {
    if let Some(backend) = backend() {
        backend.reject(file);
    }
}

/// Whether the installed application adapter accepts completed downloads.
pub fn cache_writes_enabled() -> bool {
    backend().is_some_and(|backend| backend.writable())
}

/// Called only with a completed encrypted CDN download, before stream decryption.
pub fn store_completed_file(file: FileId, source: &mut dyn CacheSource) -> io::Result<()> {
    if let Some(backend) = backend()
        && backend.writable()
    {
        let position = source.stream_position()?;
        let result = backend.store(file, source);
        source.seek(SeekFrom::Start(position))?;
        result?;
    }
    Ok(())
}

pub fn store_partial_file(
    file: FileId,
    source: &mut dyn CacheSource,
    ranges: &[(u64, u64)],
) -> io::Result<()> {
    if let Some(backend) = backend()
        && backend.writable()
    {
        let position = source.stream_position()?;
        let result = backend.store_partial(file, source, ranges);
        source.seek(SeekFrom::Start(position))?;
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::atomic::{AtomicBool, Ordering},
    };
    struct ConsumingBackend(AtomicBool);
    impl CacheBackend for ConsumingBackend {
        fn open(&self, _: FileId) -> io::Result<Option<Box<dyn CachedReader>>> {
            Ok(None)
        }
        fn reject(&self, _: FileId) {}
        fn writable(&self) -> bool {
            true
        }
        fn store(&self, _: FileId, source: &mut dyn CacheSource) -> io::Result<()> {
            io::copy(source, &mut io::sink())?;
            if self.0.load(Ordering::Relaxed) {
                Err(io::Error::other("test failure"))
            } else {
                Ok(())
            }
        }
        fn store_partial(
            &self,
            file: FileId,
            source: &mut dyn CacheSource,
            ranges: &[(u64, u64)],
        ) -> io::Result<()> {
            assert_eq!(ranges, &[(0, 3)]);
            self.store(file, source)
        }
    }
    #[test]
    fn restores_download_cursor_for_other_caches_even_after_writer_failure() {
        let backend = Arc::new(ConsumingBackend(AtomicBool::new(true)));
        assert!(set_cache_backend(backend.clone()));
        let mut source = Cursor::new(vec![1, 2, 3, 4, 5]);
        source.set_position(2);
        assert!(store_completed_file(FileId([0; 20]), &mut source).is_err());
        assert_eq!(source.position(), 2);
        assert!(store_partial_file(FileId([0; 20]), &mut source, &[(0, 3)]).is_err());
        assert_eq!(source.position(), 2);
        backend.0.store(false, Ordering::Relaxed);
        store_completed_file(FileId([0; 20]), &mut source).unwrap();
        assert_eq!(source.position(), 2);
        store_partial_file(FileId([0; 20]), &mut source, &[(0, 3)]).unwrap();
        assert_eq!(source.position(), 2);
        assert_eq!(source.into_inner(), vec![1, 2, 3, 4, 5]);
    }
}
