# ws-endpoint-monitor

WebSocket endpoint monitor for Substrate-based blockchain nodes with Prometheus metrics.

## Overview

This tool continuously monitors the health of a single Substrate blockchain node's WebSocket endpoint by periodically connecting and fetching the finalized block head. Connection results are tracked and exposed as Prometheus metrics via an HTTP endpoint.

## Installation

```bash
cargo install --path .
```

## Usage

### Basic Usage

```bash
ws-endpoint-monitor wss://your-node.example.com
```

### Advanced Configuration

```bash
ws-endpoint-monitor \
  wss://rpc.polkadot.io \
  --monitor-interval 30 \
  --monitor-connection-timeout 5 \
  --monitor-request-timeout 10 \
  --server-addr 0.0.0.0 \
  --server-port 3000 \
  --verbose
```

## Configuration Options

The node URL is a required positional argument:

- `<NODE_URL>` - WebSocket URL of the Substrate node to monitor (e.g., `wss://rpc.polkadot.io`)

Optional parameters:

| Option                         | Default   | Description                   |
| ------------------------------ | --------- | ----------------------------- |
| `--monitor-interval`           | `60`      | Seconds between checks        |
| `--monitor-connection-timeout` | `5`       | Connection timeout (seconds)  |
| `--monitor-request-timeout`    | `5`       | RPC request timeout (seconds) |
| `--server-addr`                | `0.0.0.0` | HTTP server bind address      |
| `--server-port`                | `3000`    | HTTP server port              |
| `--verbose`                    | `false`   | Enable debug logging          |

## Metrics

Metrics are available at `http://<server-addr>:<server-port>/metrics` in Prometheus format:

```
check_count{endpoint="wss://rpc.polkadot.io",result="SUCCESS"} 42
check_count{endpoint="wss://rpc.polkadot.io",result="TIMEOUT"} 3
```
