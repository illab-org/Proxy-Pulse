# Proxy Pulse

**Open-source proxy pool management and network quality monitoring system.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)]()

> **[中文文档](README_CN.md)**

---

## About

Proxy Pulse is an open-source proxy pool lifecycle management tool designed for **legitimate network infrastructure monitoring, quality-of-service (QoS) testing, and enterprise proxy resource management**. It helps DevOps engineers, network administrators, and QA teams efficiently manage, validate, and monitor the health of proxy servers in their infrastructure.

### Key Features

- **Multi-source proxy aggregation** — URL subscriptions, local files, manual import
- **Automated health checking** — Parallel multi-target checking with adaptive retry backoff
- **Proxy scoring system** — Composite score based on success rate, latency, and stability
- **Metadata detection** — Country, protocol, and anonymity level
- **REST API** — Full API for proxy consumption (JSON, TXT, CSV export)
- **Web Dashboard** — Real-time monitoring with charts and statistics
- **Admin Panel** — Source management, user management, bulk operations
- **i18n** — English, 简体中文, 繁體中文, 日本語
- **Cross-platform** — Linux (amd64/arm64), macOS (amd64/arm64)

> **⚠️ This software is NOT designed, intended, or authorized for bypassing network security controls, circumventing internet censorship, accessing restricted content, or any activity that violates applicable laws and regulations.**

---

## Quick Start

This branch contains pre-compiled binaries. No build tools required.

### 1. Download

```bash
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse
```

### 2. Configure

```bash
cp config.example.yaml config.yaml
# Edit config.yaml as needed
```

### 3. Run

```bash
# Start (auto-detects your OS and architecture)
./run

# Check status
./run status

# Stop
./run stop
```

The `run` script automatically selects the correct binary for your platform from the `build/` directory and starts it in the background.

### 4. Access

| URL | Description |
|---|---|
| `http://localhost:8080` | Web Dashboard |
| `http://localhost:8080/admin` | Admin Panel |
| `http://localhost:8080/settings` | Personal Settings |

On first launch you will be prompted to create an admin account.

---

## Supported Platforms

| Platform | Binary |
|---|---|
| Linux x86_64 | `build/proxy-pulse-linux-amd64` |
| Linux ARM64 | `build/proxy-pulse-linux-arm64` |
| macOS x86_64 | `build/proxy-pulse-darwin-amd64` |
| macOS ARM64 (Apple Silicon) | `build/proxy-pulse-darwin-arm64` |

---

## Configuration

```yaml
server:
  host: "0.0.0.0"
  port: 8080

database:
  url: "sqlite://proxy_pulse.db?mode=rwc"

sources:
  sync_interval_secs: 1800
  providers:
    - type: file
      path: ./proxies.txt

checker:
  interval_secs: 60
  timeout_secs: 10
  max_concurrent: 200
  targets:
    - https://httpbin.org/ip
    - https://www.cloudflare.com/cdn-cgi/trace

scoring:
  min_score: 60
  weight_success_rate: 0.4
  weight_latency: 0.35
  weight_stability: 0.25
```

---

## API Endpoints

### Public
```
GET  /api/v1/proxy/random          # Random healthy proxy
GET  /api/v1/proxy/top?limit=10    # Top-scored proxies
GET  /api/v1/proxy/country/:code   # Filter by country code
GET  /api/v1/proxy/all             # All proxies (paginated)
GET  /api/v1/proxy/json            # Export healthy proxies as JSON
GET  /api/v1/proxy/txt             # Export as plain text (ip:port)
GET  /api/v1/proxy/csv             # Export as CSV
GET  /api/v1/proxy/stats           # Pool statistics
GET  /api/v1/health                # Health check
```

### Admin (requires authentication)
```
GET  /api/v1/admin/proxy/list      # List all proxies
POST /api/v1/admin/proxy/import    # Bulk import
POST /api/v1/admin/proxy/purge-dead # Purge dead proxies
POST /api/v1/admin/proxy/delete/:id # Delete proxy
GET  /api/v1/admin/source/list     # List sources
POST /api/v1/admin/source/add      # Add source
POST /api/v1/admin/source/delete/:id # Delete source
POST /api/v1/admin/source/sync     # Manual sync all
```

---

## Building from Source

If you want the **latest development version** or need to customize the build, switch to the [`source`](https://github.com/OpenInfra-Labs/Proxy-Pulse/tree/source) branch:

```bash
git checkout source
```

### Requirements

- **Rust** 1.75+ (install via [rustup](https://rustup.rs/))
- No other dependencies required (SQLite is embedded)

### Build & Run

```bash
cargo build --release
./target/release/proxy-pulse
```

> **Note:** Building from the `source` branch requires a Rust development environment and takes longer than using the pre-compiled binaries on the `main` branch.

---

## License

This project is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 OpenInfra Labs.
