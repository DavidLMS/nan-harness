use super::{acquire_process_lock, tokens_match, write_receipt};
use crate::paths::private_directory;
use crate::protocol::{PROTOCOL_VERSION, Receipt};
use nan_harness_private_fs::open_private_read;

#[test]
fn process_election_and_receipt_replacement_are_deterministic() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let directory = temporary.path().join("coordinator/v1");
    private_directory(&directory).expect("coordinator directory should be private");
    let lock = acquire_process_lock(&directory).expect("first process should win election");
    assert!(acquire_process_lock(&directory).is_err());

    for generation in ["first", "second"] {
        write_receipt(
            &directory,
            &Receipt {
                protocol_version: PROTOCOL_VERSION,
                port: 42,
                token: "private-token".to_owned(),
                generation: generation.to_owned(),
                pid: 42,
            },
        )
        .expect("receipt should replace safely");
    }
    let (file, _) = open_private_read(&directory.join("receipt.json"))
        .expect("receipt should be private and readable");
    let receipt: Receipt = serde_json::from_reader(file).expect("receipt should contain JSON");
    assert_eq!(receipt.generation, "second");
    assert!(tokens_match("private-token", &receipt.token));
    assert!(!tokens_match("private-token", "wrong-token"));
    drop(lock);
    acquire_process_lock(&directory).expect("election lock should be released");
}
