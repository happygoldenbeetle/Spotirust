# Copied Spotify cache initialization

Windows setup for storage copied from Spotify 1.3.8.620. The installed native module is checked against its validated SHA256 before its version specific layout is inspected. Spotify itself is not bundled or modified.

Initialization starts a new helper with a restricted Low integrity token and a kill on close job. That helper checks that write opens to the original cache directories are denied before launching an isolated native profile. Only the copied 512 byte index header is placed in that profile. No source audio or saved login credentials are copied. The native window starts hidden and is parked off screen; only processes created inside the owned job are stopped.

The helper validates candidate storage state against copied encrypted index records. The parent then validates every index record and every registered native filename before atomically publishing a private profile. Initial setup never replaces an existing destination profile. Settings refresh uses a guarded atomic replacement of that private profile without a backup. Keep profile files outside version control. The small isolated setup directory is retained for diagnostics and contains no source audio.

The desktop app automatically initializes a pasted cache when its profile is absent. Locations checked are `%APPDATA%/Spotirust/Cache`, `%APPDATA%/Spotirust`, `Cache` beside the executable, and the executable directory itself. Both `Data` and `Storage/index.dat` must exist. `SPOTIRUST_CACHE_ROOT` selects an explicit copied location. Existing configured profiles remain authoritative.

Explicit setup is also available without starting the GUI:

```text
spotirust.exe --import-spotify-cache DIRECTORY [--profile-output FILE] [--writable]
```

The standalone `cache-init` binary accepts the same arguments. Explicit setup defaults to read only; automatic setup uses a writable separate copy. Both reject overlaps with original Spotify directories, including a configured external storage location. In the verified local dataset the native Rust setup validated all 679 records and 294 audio entries without another audio copy. An automatic setup test initialized a disposable copied cache and read verified cached bytes through the actual application adapter.

The helper captures the active byte override, capacity configuration, and three native realm override maps. The application fingerprints persisted storage preferences, account properties, and the native module metadata. A change triggers recapture at startup or within 30 seconds while running. Runtime verification uses a synthetic record under the current key, allowing the working index to remain exclusively held and readers to continue. Writes and cleanup pause until verification succeeds.

Limitations: this initializer supports the pinned native build and current Windows storage layout. Settings are those exposed by the clean restricted native instance. Transient settings that exist only in the source process and never reach persisted files are not synchronized; do not assume copying an INI setting reproduces its active value.
