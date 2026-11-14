use spotify_storage_init::initialize_copy;

#[test]
fn an_existing_profile_is_preserved_before_any_source_or_native_access() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("storage-profile.json");
    let bytes = b"existing private profile must remain exact";
    std::fs::write(&profile, bytes).unwrap();
    assert!(initialize_copy(&directory.path().join("missing-cache"), &profile, true).is_err());
    assert_eq!(std::fs::read(profile).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn a_direct_helper_call_without_a_restricted_token_performs_no_setup_writes() {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        spotify_storage_init::helper(directory.path(), directory.path(), directory.path()).is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn profile_refresh_is_atomic_without_a_backup_and_rejects_a_stale_writer() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile.json");
    std::fs::write(&profile, b"old owned metadata").unwrap();
    spotify_storage_init::replace_profile(&profile, b"old owned metadata", b"new owned metadata")
        .unwrap();
    assert_eq!(std::fs::read(&profile).unwrap(), b"new owned metadata");
    assert!(
        spotify_storage_init::replace_profile(&profile, b"old owned metadata", b"stale overwrite")
            .is_err()
    );
    assert_eq!(std::fs::read(&profile).unwrap(), b"new owned metadata");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
