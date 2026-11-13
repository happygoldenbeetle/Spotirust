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

No cache writer or eviction behavior is added by these patches yet.
