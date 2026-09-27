//! Browser-only production-router fixture. No database or binary-startup claim.
use axum::{
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use glaux_server::{
    discovery,
    http_boundary::{HttpBoundary, Limits},
};
use std::{io::Write, time::Duration};

async fn record(request: Request, next: Next) -> Response {
    println!(
        "Discovery browser request: {}",
        serde_json::json!({
            "method": request.method().as_str(), "uri": request.uri().to_string()
        })
    );
    next.run(request).await
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    assert_eq!(
        std::env::args().len(),
        1,
        "No listener overrides are permitted"
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let boundary = HttpBoundary::new(Some(&root), Limits::default()).unwrap();
    let routes = discovery::router(&boundary).unwrap();
    let routes = boundary.router(routes).layer(middleware::from_fn(record));
    println!("Discovery browser ready: {root}");
    std::io::stdout().flush().unwrap();
    axum::serve(listener, routes)
        .with_graceful_shutdown(async {
            tokio::select! {
                result = tokio::signal::ctrl_c() => result.unwrap(),
                () = tokio::time::sleep(Duration::from_secs(120)) => (),
            }
        })
        .await
        .unwrap();
    println!("Discovery browser stopped.");
}
