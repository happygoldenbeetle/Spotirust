# Librespot patches

`librespot-audio` and `librespot-playback` are copied from the published 0.8.0
crates. Both retain the upstream MIT license. Upstream source:
https://github.com/librespot-org/librespot/tree/v0.8.0

The audio crate adds an optional application supplied read only cache backend,
a cached reader with an explicit logical byte length, and a process local reject
callback. The playback crate routes imported decoder failures to that callback
instead of the ordinary cache file deletion method. Default caching is unchanged
when no adapter is installed. Credentials and audio stream keys still use the
ordinary authenticated session.

The completed download callback can send encrypted CDN bytes to an explicitly
enabled application writer. It also exposes whether that writer is active. The
patches themselves do not own cache paths, delete files, or schedule eviction.
