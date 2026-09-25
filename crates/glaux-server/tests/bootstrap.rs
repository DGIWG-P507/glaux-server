use std::process::Command;

#[test]
fn startup_requires_an_explicit_command() {
    // Serving must be requested with explicit configuration, never by default.
    let output = Command::new(env!("CARGO_BIN_EXE_glaux-server"))
        .output()
        .expect("the built bootstrap executable must run");

    assert_eq!(
        output.status.code(),
        Some(2),
        "implicit startup must fail"
    );
    assert!(
        output.stdout.is_empty(),
        "no successful response is available"
    );
    assert_eq!(
        output.stderr, b"glaux-server: an explicit command is required; use --help.\n",
        "the command requirement must be explicit"
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
