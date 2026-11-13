# Spotify storage format foundation

Experimental storage support reconstructed and compared with Spotify for Windows 1.3.8.620. Complete imported audio streams can now be read through Spotirust's playback library. Bidirectional compatibility is still in progress.

Implemented primitives include index record validation and updates that preserve unknown fields, record encryption and decryption with an explicit key, payload transforms at arbitrary physical offsets, file IV and name construction, Adler32 checksums, preference value precedence, and capacity arithmetic.

Native comparison covers synthetic record encryption and decryption, synthetic payload transforms, and 161 capacity calculations. The fixtures contain public test keys and generated data, not the user's cache or account information.

`policy` provides the reference version's initialized capacity defaults, all 14 realm defaults, retention deadlines, and a pure eviction planner. It matches 350 native retention decisions and 254 native eviction plans, including equal deadline ordering, protected and open entries, realm quotas, total capacity, minimum ages, day rounding, and free disk space. The planner performs no deletions. Its specialized sorter follows Microsoft STL's algorithm with its license retained in `LICENSE-MICROSOFT-STL.txt`.

`reader::Catalog` opens an explicitly selected snapshot with separately obtained storage state and validates every encrypted index record. Complete streams have bounded reads and seeks. Partial streams expose only checksum marked available ranges and reject reads of missing chunks. Both validate the footer, metadata, and each returned chunk. The application rejects decoder failures in memory without deleting imported files.

The playback adapter seeds its temporary download file from verified partial ranges and starts its first request at the first missing range. A local test reused 2,326,528 bytes, decoded and sought successfully, then fetched the remaining bytes and published the completed entry. All 23 partial files in the verification snapshot passed reads of their 10,494 available chunks (171,899,537 bytes).

`complete::write_complete` writes complete payloads, metadata, chunk checksums, and native footers into a caller supplied output stream. It reproduces the native encrypted or plaintext layout and preserves unknown footer fields. It does not open files, publish an index, or enable application cache writes. Local comparison reproduced all 656 complete files in a copied cache byte for byte (2,514,898,757 bytes and 153,617 chunks). These counts describe this verification dataset, not all Spotify caches.

`store::WorkingStore` publishes complete audio files and encrypted index records. It can create a new directory or exclusively open a distinct copy whose header matches an original opened read only. It rejects overlapping roots, index junctions outside the copy, and hard linked original indexes. Complete registered entries remain intact. Partial entries can be completed in a new file version while preserving their opaque metadata and original bytes. Failed writes stop further publication and retain unpublished files for inspection. Three completed entries, including an upgraded partial file, were retained by an isolated reference client.

The application reads `%APPDATA%/Spotirust/storage-profile.json`, or an explicit `SPOTIRUST_STORAGE_PROFILE` override. The private JSON contains `snapshot_root`, `storage_key_hex` (32 hex characters), and `naming_state_hex` (40 hex characters). State extraction is not automatic yet. Keep this file outside version control. The snapshot is never passed to librespot's writable cache.

While importing, the player inherits supported streaming quality from the official account's user preferences. This prevents a 320 kbps cache from being missed because librespot otherwise defaults to 160 kbps. A full local player test, including its real output stage muted during verification, started cached playback and sought to 30 seconds with zero audio downloads. An offscreen application launch also loaded the default profile and matched the official 320 kbps setting.

An explicit `writable: true` enables the experimental completed download callback into a separate working copy. This is used only in the disposable write tests so far. Caps and cleanup are not scheduled by that callback yet. The original index is opened read only solely to validate that the writable index is separate and has a matching header. Failure to enable writes retains imported playback.

Not implemented or verified: automatic initialization from a pasted cache alone, persistence of newly downloaded partial ranges before completion, live configuration overrides and cleanup scheduling, crash recovery after an interrupted index write, or full bidirectional application playback interoperability. Replaced partial files remain as unindexed files until native cleanup; the reference client removed the superseded test file on startup. The eviction planner is not yet scheduled by Spotirust. No cache deletion runs automatically in Spotirust.

Run checks with:

```text
cargo test --manifest-path crates/spotify-storage-format/Cargo.toml
```
