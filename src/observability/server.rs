use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use anyhow::Result;
use hyper::body::Bytes;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, StatusCode};
use tokio::sync::oneshot;

use super::MetricsCollector;

pub struct MetricsServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    address: SocketAddr,
}

impl MetricsServer {
    pub fn start(listen: SocketAddr, collector: MetricsCollector) -> Result<Self> {
        let (tx, rx) = oneshot::channel::<()>();
        let (addr_tx, addr_rx) = mpsc::channel();
        let collector = Arc::new(collector);

        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build metrics runtime");

            let collector = collector.clone();

            runtime.block_on(async move {
                let make_svc = make_service_fn(move |_| {
                    let collector = collector.clone();
                    async move {
                        Ok::<_, hyper::Error>(service_fn(move |req| {
                            let collector = collector.clone();
                            async move { handle_request(req, collector).await }
                        }))
                    }
                });

                let builder = hyper::Server::try_bind(&listen).expect("bind metrics server");
                let addr = builder.local_addr();
                addr_tx.send(addr).ok();
                let server = builder.serve(make_svc);
                let graceful = server.with_graceful_shutdown(async move {
                    let _ = rx.await;
                });

                if let Err(err) = graceful.await {
                    tracing::error!(error = %err, "Metrics server error");
                }
            });
        });

        let address = addr_rx.recv().unwrap_or(listen);

        Ok(Self {
            shutdown_tx: Some(tx),
            thread: Some(thread),
            address,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MetricsServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn handle_request(
    req: Request<Body>,
    collector: Arc<MetricsCollector>,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/metrics") => {
            let snapshot = collector.snapshot();
            let body = snapshot.to_prometheus();
            Ok(Response::new(Body::from(body)))
        }
        (&Method::GET, "/metrics.json") => {
            let snapshot = collector.snapshot();
            let body = serde_json::to_vec(&snapshot).unwrap_or_else(|_| b"{}".to_vec());
            Ok(Response::builder()
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap())
        }
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(Bytes::from_static(b"Not Found")))
            .unwrap()),
    }
}
