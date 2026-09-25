//! Health-only listener. No CSAPI resource operation or authentication adapter yet.
use crate::configuration::Configuration;
use crate::storage::check_schema;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing::get};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::fmt;
use std::time::Duration;
use tokio::net::TcpListener;

#[derive(Clone)]
struct Health {
    pool: PgPool,
    timeout: Duration,
}

impl Health {
    async fn storage_ready(&self) -> bool {
        matches!(
            tokio::time::timeout(self.timeout, async {
                let mut connection = self.pool.acquire().await?;
                check_schema(&mut connection).await
            })
            .await,
            Ok(Ok(()))
        )
    }
}

fn response(status: StatusCode, body: &'static str) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn live() -> Response {
    response(StatusCode::OK, "alive\n")
}

async fn ready(State(health): State<Health>) -> Response {
    if health.storage_ready().await {
        response(StatusCode::OK, "ready\n")
    } else {
        response(StatusCode::SERVICE_UNAVAILABLE, "not ready\n")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeError {
    Storage,
    Listener,
    Signal,
    Serving,
    Shutdown,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Storage => "required storage or schema unavailable",
            Self::Listener => "health listener unavailable",
            Self::Signal => "shutdown signal unavailable",
            Self::Serving => "health listener failed",
            Self::Shutdown => "bounded health shutdown did not complete",
        })
    }
}

impl std::error::Error for RuntimeError {}

pub async fn serve(config: Configuration) -> Result<(), RuntimeError> {
    // Bound connection count and acquisition time; no new connection per probe.
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .min_connections(0)
        .acquire_timeout(config.timeout())
        .connect_lazy_with(config.database());
    let health = Health {
        pool: pool.clone(),
        timeout: config.timeout(),
    };
    if !health.storage_ready().await {
        pool.close().await;
        return Err(RuntimeError::Storage);
    }
    let listener = TcpListener::bind(config.listener())
        .await
        .map_err(|_| RuntimeError::Listener)?;
    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(health);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| RuntimeError::Signal)?;
    // Register shutdown handling before declaring the listener usable.
    let signal = async move {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = terminate.recv() => Ok(()),
                result = tokio::signal::ctrl_c() => result.map_err(|_| RuntimeError::Signal),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .map_err(|_| RuntimeError::Signal)
        }
    };
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });
    println!("Health listener ready.");
    tokio::select! {
        result = &mut server => {
            tokio::time::timeout(Duration::from_secs(5), pool.close()).await.map_err(|_| RuntimeError::Shutdown)?;
            result.map_err(|_| RuntimeError::Serving)?.map_err(|_| RuntimeError::Serving)
        },
        signal_result = signal => {
            let _ = shutdown_tx.send(());
            // One deadline covers both HTTP draining and checked-out storage work.
            match tokio::time::timeout(Duration::from_secs(5), async {
                (&mut server).await.map_err(|_| RuntimeError::Serving)?.map_err(|_| RuntimeError::Serving)?;
                pool.close().await;
                signal_result
            }).await {
                Ok(result) => result,
                Err(_) => { server.abort(); Err(RuntimeError::Shutdown) }
            }
        }
    }
}
