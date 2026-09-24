use std::process::Command;

#[test]
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
        output.stderr, b"glaux-server: no listening server is implemented; use migrate or check-schema explicitly.\n",
        "the limitation must be explicit"
    );
}

#[test]
fn database_commands_require_explicit_configuration() {
    for command in ["migrate", "check-schema"] {
        let output = Command::new(env!("CARGO_BIN_EXE_glaux-server"))
            .arg(command)
            .env_remove("GLAUX_DATABASE_URL")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"glaux-server: a valid GLAUX_DATABASE_URL is required for the explicit database command.\n");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_glaux-server"))
        .arg("serve")
        .env_remove("GLAUX_DATABASE_URL")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
