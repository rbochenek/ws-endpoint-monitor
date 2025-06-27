//! WebSocket endpoint monitor for Substrate-based blockchain nodes with Prometheus metrics
//!
//! This application monitors the health of a WebSocket connection to a Substrate node
//! by periodically attempting to connect and fetch the finalized block head.
//! Results are exposed as Prometheus metrics via an HTTP endpoint.

use actix_web::{App, HttpResponse, HttpServer, get, web};
use anyhow::Result;
use clap::Parser;
use jsonrpsee::{core::client::ClientT, rpc_params, ws_client::WsClientBuilder};
use prometheus::{Counter, Encoder, Opts, Registry, TextEncoder};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time;
use tracing::{Level, event};
use tracing_subscriber::FmtSubscriber;

/// Command line arguments
#[derive(Clone, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// WebSocket URL of the Substrate node to monitor.
    ///
    /// This should be a valid WebSocket endpoint (ws:// or wss://).
    #[arg(long, default_value = "wss://mainnet.liberland.org")]
    monitor_url: String,

    /// Interval between connection checks in seconds.
    ///
    /// The monitor will attempt to connect to the node at this interval.
    #[arg(long, default_value_t = 60)]
    monitor_interval: u64,

    /// Timeout for establishing WebSocket connection in seconds.
    ///
    /// If the connection cannot be established within this time, it's marked as failed.
    #[arg(long, default_value_t = 5)]
    monitor_connection_timeout: u64,

    /// Timeout for RPC requests in seconds.
    ///
    /// After connection is established, this timeout applies to individual RPC calls.
    #[arg(long, default_value_t = 5)]
    monitor_request_timeout: u64,

    /// HTTP server bind address.
    ///
    /// The address where the metrics endpoint will be exposed.
    #[arg(long, default_value = "0.0.0.0")]
    server_addr: String,

    /// HTTP server port.
    ///
    /// The port where the metrics endpoint will be exposed.
    #[arg(long, default_value_t = 3000)]
    server_port: u16,

    /// Enable verbose logging.
    ///
    /// When set, changes log level from INFO to DEBUG.
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

/// Shared application state containing metrics counters.
#[derive(Clone)]
struct AppState {
    /// The WebSocket endpoint being monitored.
    ws_endpoint: String,
    /// Counter for successful connection attempts.
    success: Arc<AtomicUsize>,
    /// Counter for failed connection attempts.
    failure: Arc<AtomicUsize>,
}

/// Initializes logging, spawns the connection monitor task, and starts the HTTP server
/// for metrics exposure.
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing subscriber with appropriate log level
    let subscriber = FmtSubscriber::builder()
        .with_max_level(if args.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set default tracing subscriber");

    // Initialize shared atomic counters
    let success_counter = Arc::new(AtomicUsize::new(0));
    let failure_counter = Arc::new(AtomicUsize::new(0));

    // Create application state
    let app_state = AppState {
        ws_endpoint: args.monitor_url.clone(),
        success: Arc::clone(&success_counter),
        failure: Arc::clone(&failure_counter),
    };

    // Spawn connection monitor task
    let _connection_monitor = tokio::spawn(connection_monitor(
        args.monitor_url,
        args.monitor_interval,
        args.monitor_connection_timeout,
        args.monitor_request_timeout,
        success_counter,
        failure_counter,
    ));

    // Start HTTP server for metrics endpoint
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(metrics_handler)
    })
    .bind((args.server_addr, args.server_port))?
    .run()
    .await?;

    Ok(())
}

/// Monitors WebSocket connection health by periodically connecting and making RPC calls.
///
/// This function runs indefinitely, attempting to:
/// 1. Establish a WebSocket connection to the node
/// 2. Make an RPC call to fetch the finalized block head
/// 3. Update success/failure counters based on the result
///
/// # Arguments
///
/// * `url` - WebSocket URL of the node to monitor
/// * `interval` - Seconds between connection attempts
/// * `connection_timeout` - Timeout for establishing connection
/// * `request_timeout` - Timeout for RPC requests
/// * `success` - Atomic counter for successful checks
/// * `failure` - Atomic counter for failed checks
async fn connection_monitor(
    url: String,
    interval: u64,
    connection_timeout: u64,
    request_timeout: u64,
    success: Arc<AtomicUsize>,
    failure: Arc<AtomicUsize>,
) {
    let mut interval = time::interval(Duration::from_secs(interval));
    let connection_timeout = Duration::from_secs(connection_timeout);
    let request_timeout = Duration::from_secs(request_timeout);

    loop {
        interval.tick().await;

        // Attempt to connect to the node
        match WsClientBuilder::new()
            .connection_timeout(connection_timeout)
            .request_timeout(request_timeout)
            .build(&url)
            .await
        {
            Ok(client) => {
                // Connection established, attempt to get the finalized block head
                match client
                    .request::<String, _>("chain_getFinalizedHead", rpc_params![])
                    .await
                {
                    Ok(resp) => {
                        // Success: valid response received
                        event!(Level::DEBUG, "Successful check, finalized head: {resp}");
                        success.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        // Failure: RPC request failed
                        event!(Level::WARN, "Check failed during RPC request: {e}");
                        failure.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(e) => {
                // Failure: could not establish connection
                event!(Level::WARN, "Check failed during connection: {e}");
                failure.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// HTTP handler for the `/metrics` endpoint.
///
/// Returns Prometheus-formatted metrics showing the current success and failure counts
/// for the monitored WebSocket endpoint.
#[get("/metrics")]
async fn metrics_handler(data: web::Data<AppState>) -> HttpResponse {
    let success = data.success.load(Ordering::Relaxed);
    let failure = data.failure.load(Ordering::Relaxed);

    prometheus_output(&data.ws_endpoint, success, failure)
}

/// Generates Prometheus-formatted metrics output.
///
/// Creates counter metrics with appropriate labels and returns them as an HTTP response
/// with the correct content type for Prometheus scraping.
///
/// # Arguments
///
/// * `endpoint` - The WebSocket endpoint being monitored (used as label)
/// * `success` - Current success count
/// * `failure` - Current failure count
fn prometheus_output(endpoint: &str, success: usize, failure: usize) -> HttpResponse {
    // Create counter metrics with endpoint label
    let counter_opts = Opts::new("check_count", "Number of connection check results")
        .const_label("endpoint", endpoint);
    let success_counter =
        Counter::with_opts(counter_opts.clone().const_label("result", "SUCCESS")).unwrap();
    let failure_counter =
        Counter::with_opts(counter_opts.const_label("result", "TIMEOUT")).unwrap();

    // Create and populate registry
    let r = Registry::new();
    r.register(Box::new(success_counter.clone())).unwrap();
    r.register(Box::new(failure_counter.clone())).unwrap();

    // Set counter values
    success_counter.inc_by(success as f64);
    failure_counter.inc_by(failure as f64);

    // Encode metrics to Prometheus text format
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metric_families = r.gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    // Return metrics with appropriate content type
    HttpResponse::Ok()
        .content_type(encoder.format_type())
        .body(buffer)
}
