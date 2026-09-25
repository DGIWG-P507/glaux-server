use glaux_server::storage::{StorageError, check_schema, migrate};
use glaux_server::{configuration::Configuration, runtime::serve};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{Connection, PgConnection};
use std::process::ExitCode;
use std::path::Path;
use std::time::Duration;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "help") {
        println!("Glaux Server (health-only foundation; no CSAPI routes yet).\nUsage: glaux-server check-config CONFIG.json | serve CONFIG.json | migrate | check-schema\ncheck-config validates configuration and secret references without connecting to storage.\nserve checks existing schema; it never runs migrations. Only /health/live and /health/ready exist.\nDatabase administrative commands require GLAUX_DATABASE_URL; migrate is explicit.\nSee docs/runtime-configuration.md for fields, bounds and security limits.");
        return ExitCode::SUCCESS;
    }
    if arguments.len() == 2 && (arguments[0] == "serve" || arguments[0] == "check-config") {
        let config = match Configuration::load(Path::new(&arguments[1])) {
            Ok(config) => config,
            Err(error) => { eprintln!("glaux-server: {error}."); return ExitCode::from(2); }
        };
        if arguments[0] == "check-config" {
            println!("Configuration valid; secrets redacted.");
            return ExitCode::SUCCESS;
        }
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            eprintln!("glaux-server: serving runtime unavailable.");
            return ExitCode::FAILURE;
        };
        return match runtime.block_on(serve(config)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => { eprintln!("glaux-server: {error}."); ExitCode::FAILURE }
        };
    }
    if arguments.len() != 1 || (arguments[0] != "migrate" && arguments[0] != "check-schema") {
        eprintln!(
            "glaux-server: an explicit command is required; use --help."
        );
        return ExitCode::from(2);
    }
    let Some(options) = std::env::var("GLAUX_DATABASE_URL")
        .ok()
        .and_then(|value| value.parse::<PgConnectOptions>().ok())
    else {
        eprintln!(
            "glaux-server: a valid GLAUX_DATABASE_URL is required for the explicit database command."
        );
        return ExitCode::from(2);
    };
    // SQLx attempts TLS even on Unix sockets unless explicitly disabled.
    // Only its actual Unix-socket route is exempt from network TLS verification.
    let ssl_mode = if options.get_socket().is_some() || options.get_host().starts_with('/') {
        PgSslMode::Disable
    } else {
        PgSslMode::VerifyFull
    };
    let options = options
        .ssl_mode(ssl_mode)
        .options([("statement_timeout", "10000"), ("lock_timeout", "5000")]);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("glaux-server: database command runtime unavailable.");
        return ExitCode::FAILURE;
    };
    let result = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut connection = PgConnection::connect_with(&options).await?;
            let result = if arguments[0] == "migrate" {
                migrate(&mut connection).await
            } else {
                check_schema(&mut connection).await
            };
            connection.close().await?;
            result
        })
        .await
    });
    match result {
        Ok(Ok(())) => {
            println!("Database command completed.");
            ExitCode::SUCCESS
        }
        Ok(Err(error)) => {
            let error: StorageError = error;
            eprintln!("glaux-server: {error}.");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("glaux-server: database command timed out.");
            ExitCode::FAILURE
        }
    }
}
