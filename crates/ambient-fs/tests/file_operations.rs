//! Behavioural tests for the ambient filesystem boundary.
//!
//! These run on the host and under Miri (`make miri`, with isolation
//! disabled so that Miri forwards file-system calls). They exercise the safe
//! wrapper surface that the rest of the host trusts, and are the only Miri
//! coverage of the descriptor-owning `File` wrapper. Advisory locking calls
//! `flock`, which Miri does not implement, so that test is ignored there.

use std::io::{Read, Seek, SeekFrom, Write};

use ambient_fs::{File, OpenOptions};
use tempfile::tempdir;

#[test]
fn write_then_read_round_trips_bytes_and_text() {
    let dir = tempdir().expect("create scratch directory");
    let path = dir.path().join("note.txt");

    ambient_fs::write(&path, "hello ambient").expect("write the file");

    assert_eq!(
        ambient_fs::read(&path).expect("read bytes"),
        b"hello ambient"
    );
    assert_eq!(
        ambient_fs::read_to_string(&path).expect("read text"),
        "hello ambient"
    );
    assert_eq!(
        ambient_fs::metadata(&path).expect("stat").len(),
        "hello ambient".len() as u64
    );
}

#[test]
fn create_dir_all_builds_nested_directories_and_remove_dir_all_clears_them() {
    let dir = tempdir().expect("create scratch directory");
    let nested = dir.path().join("a/b/c");

    ambient_fs::create_dir_all(&nested).expect("create nested directories");
    assert!(ambient_fs::metadata(&nested).expect("stat").is_dir());
    assert!(ambient_fs::try_exists(&nested).expect("probe existence"));

    ambient_fs::remove_dir_all(dir.path().join("a")).expect("remove the tree");
    assert!(!ambient_fs::try_exists(&nested).expect("probe existence"));
}

#[test]
fn rename_and_remove_move_content_between_paths() {
    let dir = tempdir().expect("create scratch directory");
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    ambient_fs::write(&first, "payload").expect("write the file");

    ambient_fs::rename(&first, &second).expect("rename");
    assert!(!ambient_fs::try_exists(&first).expect("probe existence"));
    assert_eq!(
        ambient_fs::read_to_string(&second).expect("read"),
        "payload"
    );

    ambient_fs::remove_file(&second).expect("remove");
    assert!(ambient_fs::read(&second).is_err());
}

#[test]
#[cfg_attr(miri, ignore = "Miri does not implement the copy_file_range syscall")]
fn copy_duplicates_content_and_reports_its_length() {
    let dir = tempdir().expect("create scratch directory");
    let source = dir.path().join("source");
    let copy = dir.path().join("copy");
    ambient_fs::write(&source, "payload").expect("write the file");

    assert_eq!(ambient_fs::copy(&source, &copy).expect("copy"), 7);

    assert_eq!(ambient_fs::read_to_string(&copy).expect("read"), "payload");
    assert_eq!(
        ambient_fs::read_to_string(&source).expect("read"),
        "payload"
    );
}

#[test]
fn read_dir_lists_every_entry_with_its_type() {
    let dir = tempdir().expect("create scratch directory");
    ambient_fs::write(dir.path().join("file"), "x").expect("write the file");
    ambient_fs::create_dir(dir.path().join("sub")).expect("create a directory");

    let mut seen: Vec<(String, bool)> = ambient_fs::read_dir(dir.path())
        .expect("list the directory")
        .map(|entry| {
            let entry = entry.expect("read an entry");
            let is_dir = entry.file_type().expect("entry type").is_dir();
            (entry.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    seen.sort();

    assert_eq!(
        seen,
        vec![("file".to_string(), false), ("sub".to_string(), true)]
    );
}

#[test]
fn file_handles_read_write_and_seek_through_the_wrapper() {
    let dir = tempdir().expect("create scratch directory");
    let path = dir.path().join("handle");

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .expect("open for read and write");
    file.write_all(b"0123456789")
        .expect("write through the handle");
    file.seek(SeekFrom::Start(4)).expect("seek");
    let mut tail = String::new();
    file.read_to_string(&mut tail).expect("read the tail");

    assert_eq!(tail, "456789");
    assert_eq!(file.metadata().expect("stat the handle").len(), 10);
}

#[test]
fn create_new_refuses_to_replace_an_existing_file() {
    let dir = tempdir().expect("create scratch directory");
    let path = dir.path().join("once");

    File::create_new(&path).expect("first creation succeeds");

    let error = File::create_new(&path).expect_err("second creation must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
}

#[test]
fn a_descriptor_survives_conversion_to_and_from_std() {
    let dir = tempdir().expect("create scratch directory");
    let path = dir.path().join("round-trip");
    ambient_fs::write(&path, "kept").expect("write the file");

    let std_file = File::open(&path).expect("open").into_std();
    let mut wrapped = File::from_std(std_file);
    let mut contents = String::new();
    wrapped.read_to_string(&mut contents).expect("read");

    assert_eq!(contents, "kept");
}

#[test]
#[cfg_attr(miri, ignore = "Miri does not implement flock")]
fn an_exclusive_lock_blocks_a_second_handle_until_unlocked() {
    let dir = tempdir().expect("create scratch directory");
    let path = dir.path().join("lock");
    let first = File::create(&path).expect("create the lock file");
    let second = File::open(&path).expect("open a second handle");

    first.lock_exclusive().expect("take the lock");
    assert!(second.try_lock_exclusive().is_err(), "lock must be held");
    first.unlock().expect("release the lock");
    second.try_lock_exclusive().expect("lock is now free");
}
