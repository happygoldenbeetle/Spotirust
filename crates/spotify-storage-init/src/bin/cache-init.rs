fn main() {
    let status=spotify_storage_init::cli().unwrap_or_else(|| {
        eprintln!("Usage: cache-init --import-spotify-cache DIRECTORY [--profile-output FILE] [--writable]");
        2
    });
    std::process::exit(status);
}
