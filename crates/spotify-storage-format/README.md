# Spotify storage format foundation

Experimental storage support reconstructed and compared with Spotify for Windows 1.3.8.620. Complete imported audio streams can now be read through Spotirust's playback library. Bidirectional compatibility is still in progress.

Implemented primitives include index record validation and updates that preserve unknown fields, record encryption and decryption with an explicit key, payload transforms at arbitrary physical offsets, file IV and name construction, Adler32 checksums, preference value precedence, and capacity arithmetic.

Native comparison covers synthetic record encryption and decryption, synthetic payload transforms, and 161 capacity calculations. The fixtures contain public test keys and generated data, not the user's cache or account information.

`policy` provides the reference version's initialized capacity defaults, all 14 realm defaults, retention deadlines, and a pure eviction planner. It matches 350 native retention decisions and 254 native eviction plans, including equal deadline ordering, protected and open entries, realm quotas, total capacity, minimum ages, day rounding, and free disk space. The planner performs no deletions. Its specialized sorter follows Microsoft STL's algorithm with its license retained in `LICENSE-MICROSOFT-STL.txt`.

`reader::Catalog` opens an explicitly selected snapshot with separately obtained storage state, validates every encrypted index record, and serves complete realm 0 files. It validates the footer, metadata, and each audio chunk before returning its bytes. Reads and seeks stop at logical audio length, excluding the native storage trailer. Partial files are cache misses. The application rejects decoder failures in memory without deleting imported files.

`complete::write_complete` writes complete payloads, metadata, chunk checksums, and native footers into a caller supplied output stream. It reproduces the native encrypted or plaintext layout and preserves unknown footer fields. It does not open files, publish an index, or enable application cache writes. Local comparison reproduced all 656 complete files in a copied cache byte for byte (2,514,898,757 bytes and 153,617 chunks). These counts describe this verification dataset, not all Spotify caches.

`store::WorkingStore` publishes complete audio files and encrypted index records. It can create a new directory or exclusively open a distinct copy whose header matches an original opened read only. It rejects overlapping roots, index junctions outside the copy, and hard linked original indexes. It never deletes files or overwrites an existing registered audio entry. Failed writes stop further publication and retain unpublished files for inspection. Two newly written entries were retained by an isolated reference client and read back through the playback adapter.

Set `SPOTIRUST_STORAGE_PROFILE` to a private local JSON file with `snapshot_root`, `storage_key_hex` (32 hex characters), and `naming_state_hex` (40 hex characters). No discovery or state extraction runs automatically. Keep this file outside version control. The snapshot is never passed to librespot's writable cache.

An explicit `writable: true` enables the experimental completed download callback into a separate working copy. This is used only in the disposable write tests so far. Caps and cleanup are not scheduled by that callback yet; partial entry replacement is also pending. The original index is opened read only solely to validate that the writable index is separate and has a matching header. Failure to enable writes retains imported playback.

Not implemented or verified: automatic initialization from a pasted cache alone, partial file availability maps, live configuration overrides and cleanup scheduling, crash recovery after an interrupted index write, or full bidirectional application playback interoperability. The eviction planner is not yet scheduled by the application. No cache deletion runs automatically.

Run checks with:

```text
cargo test --manifest-path crates/spotify-storage-format/Cargo.toml
```
