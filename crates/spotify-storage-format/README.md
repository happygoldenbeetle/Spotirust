# Spotify storage format foundation

Experimental storage support reconstructed and compared with Spotify for Windows 1.3.8.620. Complete imported audio streams can now be read through Spotirust's playback library. Bidirectional compatibility is still in progress.

Implemented primitives include index record validation and updates that preserve unknown fields, record encryption and decryption with an explicit key, payload transforms at arbitrary physical offsets, file IV and name construction, Adler32 checksums, preference value precedence, and capacity arithmetic.

Native comparison covers synthetic record encryption and decryption, synthetic payload transforms, and 161 capacity calculations. The fixtures contain public test keys and generated data, not the user's cache or account information.

`policy` provides the reference version's initialized capacity defaults, all 14 realm defaults, retention deadlines, and a pure eviction planner. It matches 350 native retention decisions and 254 native eviction plans, including equal deadline ordering, protected and open entries, realm quotas, total capacity, minimum ages, day rounding, and free disk space. The planner performs no deletions. Its specialized sorter follows Microsoft STL's algorithm with its license retained in `LICENSE-MICROSOFT-STL.txt`.

`reader::Catalog` opens an explicitly selected snapshot with separately obtained storage state, validates every encrypted index record, and serves complete realm 0 files. It validates the footer, metadata, and each audio chunk before returning its bytes. Reads and seeks stop at logical audio length, excluding the native storage trailer. Partial files are cache misses. The application rejects decoder failures in memory without deleting imported files.

`complete::write_complete` writes complete payloads, metadata, chunk checksums, and native footers into a caller supplied output stream. It reproduces the native encrypted or plaintext layout and preserves unknown footer fields. It does not open files, publish an index, or enable application cache writes. Local comparison reproduced all 656 complete files in a copied cache byte for byte (2,514,898,757 bytes and 153,617 chunks). These counts describe this verification dataset, not all Spotify caches.

Set `SPOTIRUST_STORAGE_PROFILE` to a private local JSON file with `snapshot_root`, `storage_key_hex` (32 hex characters), and `naming_state_hex` (40 hex characters). No discovery or state extraction runs automatically. Keep this file outside version control. The snapshot is never passed to librespot's writable cache.

Not implemented or verified: automatic initialization from a pasted cache alone, partial file availability maps, file allocation and locking, live configuration overrides and cleanup scheduling, cache transactions, or bidirectional application interoperability. The writer and eviction planner are not yet registered as the application's write backend. No cache deletion runs automatically.

Run checks with:

```text
cargo test --manifest-path crates/spotify-storage-format/Cargo.toml
```
