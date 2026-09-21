use std::process::Command;

#[test]
#[ignore = "disposable CI control; must not be merged"]
fn unfinished_server_does_not_report_success() {
    // Independent bootstrap contract: no listening service exists yet.
    // A silent, successful placeholder executable must not satisfy this test.
    let output = Command::new(env!("CARGO_BIN_EXE_glaux-server"))
        .output()
        .expect("the built bootstrap executable must run");

    assert_eq!(
        output.status.code(),
        Some(2),
        "unfinished startup must fail"
    );
    assert!(
        output.stdout.is_empty(),
        "no successful response is available"
    );
    assert_eq!(
        output.stderr, b"glaux-server: bootstrap only; no server commands are implemented yet.\n",
        "the limitation must be explicit"
    );
}
