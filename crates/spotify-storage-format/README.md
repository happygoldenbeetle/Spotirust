# Spotify storage format foundation

Experimental storage support reconstructed and compared with Spotify for Windows 1.3.8.620. Complete imported audio streams can now be read through Spotirust's playback library. Bidirectional compatibility is still in progress.

Implemented primitives include index record validation and updates that preserve unknown fields, record encryption and decryption with an explicit key, payload transforms at arbitrary physical offsets, file IV and name construction, Adler32 checksums, preference value precedence, and capacity arithmetic.

Native comparison covers synthetic record encryption and decryption, synthetic payload transforms, and 161 capacity calculations. The fixtures contain public test keys and generated data, not the user's cache or account information.

`reader::Catalog` opens an explicitly selected snapshot with separately obtained storage state, validates every encrypted index record, and serves complete realm 0 files. Reads and seeks stop at logical audio length, excluding the native storage trailer. Partial files are cache misses. The application rejects decoder failures in memory without deleting imported files.

Set `SPOTIRUST_STORAGE_PROFILE` to a private local JSON file with `snapshot_root`, `storage_key_hex` (32 hex characters), and `naming_state_hex` (40 hex characters). No discovery or state extraction runs automatically. Keep this file outside version control. The snapshot is never passed to librespot's writable cache.

Not implemented or verified: automatic initialization from a pasted cache alone, complete file writers and availability maps, file allocation and locking, automatic size defaults, full eviction behavior, cache transactions, or bidirectional application interoperability. No storage caps or eviction behavior are silently substituted.

Run checks with:

```text
cargo test --manifest-path crates/spotify-storage-format/Cargo.toml
```
