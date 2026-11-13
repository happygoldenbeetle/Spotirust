# Spotify storage format foundation

Experimental primitives reconstructed and compared with Spotify for Windows 1.3.8.620. This is **not a working cache backend** and is not connected to Spotirust playback.

Implemented primitives include index record validation and updates that preserve unknown fields, record encryption and decryption with an explicit key, payload transforms at arbitrary physical offsets, file IV and name construction, Adler32 checksums, preference value precedence, and capacity arithmetic.

Native comparison covers synthetic record encryption and decryption, synthetic payload transforms, and 161 capacity calculations. The fixtures contain public test keys and generated data, not the user's cache or account information.

Not implemented or verified: deriving storage state from a real cache, complete file headers and availability maps, file allocation and locking, automatic size defaults, full eviction behavior, cache transactions, imported audio decoding, or bidirectional application interoperability.

The crate performs no filesystem operations and cannot delete cache files. It cannot be used to route the official cache into librespot's writable cache.

Run checks with:

```text
cargo test --manifest-path crates/spotify-storage-format/Cargo.toml
```
