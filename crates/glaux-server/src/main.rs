use std::process::ExitCode;
use glaux_server::storage::{check_schema, migrate, StorageError};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Connection, PgConnection};
use std::time::Duration;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 1 || (arguments[0] != "migrate" && arguments[0] != "check-schema") {
        eprintln!("glaux-server: no listening server is implemented; use migrate or check-schema explicitly.");
        return ExitCode::from(2);
    }
    let Some(options) = std::env::var("GLAUX_DATABASE_URL").ok()
        .and_then(|value| value.parse::<PgConnectOptions>().ok()) else {
        eprintln!("glaux-server: a valid GLAUX_DATABASE_URL is required for the explicit database command.");
        return ExitCode::from(2);
    };
    // Network database connections authenticate the server; the isolated test
    // example configures its own private loopback connection separately.
    let options = options.ssl_mode(PgSslMode::VerifyFull)
        .options([("statement_timeout", "10000"), ("lock_timeout", "5000")]);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        eprintln!("glaux-server: database command runtime unavailable.");
        return ExitCode::FAILURE;
    };
    let result = runtime.block_on(async { tokio::time::timeout(Duration::from_secs(30), async {
        let mut connection = PgConnection::connect_with(&options).await?;
        let result = if arguments[0] == "migrate" {
            migrate(&mut connection).await
        } else {
            check_schema(&mut connection).await
        };
        connection.close().await?;
        result
    }).await });
    match result {
        Ok(Ok(())) => { println!("Database command completed."); ExitCode::SUCCESS }
        Ok(Err(error)) => {
            let error: StorageError = error;
            eprintln!("glaux-server: {error}."); ExitCode::FAILURE
        }
        Err(_) => { eprintln!("glaux-server: database command timed out."); ExitCode::FAILURE }
    }
}
