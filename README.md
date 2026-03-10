# Proxy Pulse

**Open-source proxy pool management and network quality monitoring system.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)]()
[![Axum](https://img.shields.io/badge/web-axum%200.7-blue.svg)]()
[![SQLite](https://img.shields.io/badge/database-SQLite-003B57.svg)]()

> **[中文文档](docs/README_CN.md)** | **[Legal Terms](docs/LEGAL.md)**

---

## Overview

Proxy Pulse is an open-source proxy pool lifecycle management tool designed for **legitimate network infrastructure monitoring, quality-of-service (QoS) testing, and enterprise proxy resource management**. It helps DevOps engineers, network administrators, and QA teams efficiently manage, validate, and monitor the health of proxy servers in their infrastructure.

Built with **Rust** for high performance and low resource usage, with an embedded SQLite database for zero-dependency deployment.

## Intended Use Cases

This project is built exclusively for **lawful and compliant** purposes, including but not limited to:

| Use Case | Description |
|---|---|
| **Enterprise Proxy Management** | Manage and monitor corporate proxy server pools for internal network routing and load balancing. |
| **Network QoS Monitoring** | Continuously test proxy latency, availability, and throughput to ensure SLA compliance. |
| **API & Web Service Testing** | Validate that web services respond correctly when accessed through different network paths. |
| **Academic & Security Research** | Study network performance, proxy protocol behavior, and connectivity patterns for research purposes. |
| **CDN & Edge Testing** | Verify content delivery and edge node accessibility across distributed infrastructure. |
| **Automated QA Pipelines** | Integrate proxy health checks into CI/CD workflows to ensure test environments are reachable. |

> **⚠️ This software is NOT designed, intended, or authorized for bypassing network security controls, circumventing internet censorship, accessing restricted content, or any activity that violates applicable laws and regulations.** See [Legal Terms](docs/LEGAL.md) for full details.

---

## Quick Start

No Rust required — just download the `run` script and it handles everything:

**Linux / macOS:**

```bash
curl -fsSL -o run https://raw.githubusercontent.com/OpenInfra-Labs/Proxy-Pulse/main/run && chmod +x run
./run
```

**Windows (PowerShell):**

```powershell
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/OpenInfra-Labs/Proxy-Pulse/main/run.ps1" -OutFile run.ps1
.\run.ps1
```

**Commands:**

| Command | Description |
|---------|-------------|
| `./run` | Start (auto-downloads latest binary) |
| `./run status` | Check if running |
| `./run stop` | Stop the service |
| `./run update` | Update script & binary to latest |

The `run` script automatically:
- Detects your OS and CPU architecture (Linux, macOS, Windows × amd64/arm64)
- Downloads the correct pre-compiled binary from GitHub Releases
- Checks for script and binary updates on every start
- Opens the dashboard in your browser (on desktop systems)
- Starts the service in the background

---

## Deployment & Running

### Prerequisites

- **Rust** 1.75+ (install via [rustup](https://rustup.rs/))
- No other dependencies required (SQLite is embedded)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse

# Build release binary
cargo build --release

# The binary is at target/release/proxy-pulse
```

### Configuration

Proxy Pulse works out of the box with sensible defaults. All settings are managed via environment variables and the Admin Panel:

**Environment Variables:**

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `sqlite://proxy_pulse.db?mode=rwc` | Database connection URL |
| `HOST` | `0.0.0.0` | Listen address |
| `PORT` | `8080` | Listen port |

**Checker Settings** (via Admin Panel → Checker Settings):

| Setting | Default | Description |
|---------|---------|-------------|
| Check Interval | 60s | How often proxies are checked |
| Request Timeout | 10s | Per-proxy timeout |
| Max Concurrent | 200 | Concurrent check tasks |
| Check Targets | httpbin.org, cloudflare.com | Health check target URLs |

### Run

```bash
# Run with defaults
cargo run --release

# Custom database path
DATABASE_URL="sqlite:///data/proxy.db?mode=rwc" cargo run --release

# Custom port
PORT=3000 cargo run --release

# Demo mode (all write APIs return 403)
cargo run --release -- --demo
```

### Access

| URL | Description |
|---|---|
| `http://localhost:8080` | Web Dashboard |
| `http://localhost:8080/admin` | Admin Panel |
| `http://localhost:8080/api/v1/health` | Health Check |

### Systemd Service (Linux)

```ini
[Unit]
Description=Proxy Pulse
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/proxy-pulse
ExecStart=/opt/proxy-pulse/proxy-pulse
Environment=PORT=8080
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

---

## Features

### 1. Subscription Source Management
Aggregate proxies from multiple configurable sources via the Admin Panel:
- **URL subscriptions** — GitHub-hosted lists, internal registries, public proxy APIs
- **Per-source sync intervals** — Each subscription can have its own sync frequency (5 min to 24 hours)

Sources can be added, removed, enabled/disabled, and manually synced from the admin interface.

### 2. Smart Proxy Deduplication
Automatic deduplication using `ip:port` as the unique identifier, eliminating redundant health checks and resource waste across all sources.

### 3. Continuous Availability Checking
Scheduled proxy health validation including:
- TCP connection testing
- HTTP round-trip verification through the proxy
- Response time measurement
- **Parallel** multi-target testing (`httpbin.org`, `cloudflare.com`) — all targets checked concurrently per proxy
- Capable of rechecking all alive proxies within **3 minutes** (200 concurrent × parallel targets)

### 4. Adaptive Retry Backoff
Intelligent backoff mechanism to reduce unnecessary checks on failing proxies. Successfully checked proxies are rechecked every **3 minutes**:

| Consecutive Failures | Next Check Interval |
|---|---|
| 1 | 3 minutes |
| 2 | 10 minutes |
| 3 | 30 minutes |
| 4 | 1 hour |
| 5 | 3 hours |
| 6 | 6 hours |
| 7 | 12 hours |
| 8 | 24 hours |
| 9+ | 48 hours |

### 5. Proxy Health Scoring
Each proxy receives a composite health score (0–100) based on five weighted components:

| Component | Max Score | Calculation |
|---|---|---|
| **Success Rate** | 60 pts | `success_rate × 60` |
| **Success Count** | 10 pts | Relative to the highest success count in the pool |
| **Country Tier** | 6 pts | Tier 1 (US, GB, DE, JP, SG…) = 6, Tier 2 = 4.5, Tier 3 = 3 |
| **Protocol Type** | 4 pts | SOCKS5 = 4, HTTPS = 3, SOCKS4 = 2, HTTP = 1 |
| **Latency** | 20 pts | ≤100ms = 20, ≥5000ms = 0, linear interpolation |

### 6. Proxy Metadata Detection
Automatically detect proxy metadata:
- **Country / Region** — via ip-api.com → ipinfo.io → ipwho.is cascade
- **Protocol** — HTTP, HTTPS, SOCKS4, SOCKS5
- **Anonymity Level** — Transparent, Anonymous, Elite

### 7. REST API
Full REST API for proxy consumption and management:

#### Public Endpoints
```
GET  /api/v1/proxy/random          # Random healthy proxy
GET  /api/v1/proxy/top?limit=10    # Top-scored proxies
GET  /api/v1/proxy/country/:code   # Filter by country code
GET  /api/v1/proxy/all             # All proxies (paginated)
GET  /api/v1/proxy/json            # Export as JSON (?sort=score&limit=10&country=US)
GET  /api/v1/proxy/txt             # Export as plain text (ip:port)
GET  /api/v1/proxy/csv             # Export as CSV
GET  /api/v1/proxy/countries       # List distinct alive countries
GET  /api/v1/proxy/stats           # Pool statistics & distributions
GET  /api/v1/health                # Health check
```

#### Admin Endpoints
```
GET  /api/v1/admin/proxy/list           # List all proxies (with admin details)
POST /api/v1/admin/proxy/import         # Bulk import proxies
POST /api/v1/admin/proxy/purge-dead     # Delete dead proxies
POST /api/v1/admin/proxy/delete/:id     # Delete a specific proxy
GET  /api/v1/admin/source/list          # List subscription sources
POST /api/v1/admin/source/add           # Add subscription source
POST /api/v1/admin/source/delete/:id    # Delete a source
POST /api/v1/admin/source/:id/toggle    # Enable/disable a source
POST /api/v1/admin/source/sync          # Trigger manual sync
GET  /api/v1/admin/settings/checker     # Get checker configuration
POST /api/v1/admin/settings/checker     # Save checker configuration
```

Response example:
```json
{
  "success": true,
  "data": {
    "ip": "1.2.3.4",
    "port": 8080,
    "protocol": "http",
    "country": "us",
    "score": 82.5,
    "latency_ms": 120.0,
    "success_rate": 95.0,
    "success_count": 10,
    "fail_count": 1
  }
}
```

### 8. Web Dashboard
A cyberpunk-themed dashboard showing:
- Total / Alive / Dead proxy counts with average score and latency
- Latency distribution chart
- Protocol distribution (doughnut chart)
- Score distribution histogram
- Top proxies table with one-click copy
- Proxy API card with format toggle (JSON / TXT / CSV) and sorting options

### 9. Admin Panel
Full management interface:
- Proxy list with status, score, latency, success/fail counts, next check time
- Bulk import proxies (one per line)
- Subscription source management with per-source sync intervals
- Checker settings (interval, timeout, concurrency, target URLs)
- User management
- Demo mode indicator (when `--demo` flag is used)

### 10. Internationalization (i18n)
Multi-language support with four built-in locales:
- 🇺🇸 English (`en`)
- 🇨🇳 简体中文 (`zh-CN`)
- 🇹🇼 繁體中文 (`zh-TW`)
- 🇯🇵 日本語 (`ja`)

Language auto-detects from the browser and can be switched from the UI.

### 11. Demo Mode
Run with `--demo` flag to enable a read-only demo mode:
- All write/mutation API endpoints return `403 Forbidden`
- A banner is displayed in the admin panel
- Useful for public demo deployments

---

## Tech Stack

| Component | Technology |
|---|---|
| Language | Rust 2021 Edition |
| Web Framework | axum 0.7 |
| Database | SQLite (via sqlx 0.7) |
| HTTP Client | reqwest 0.12 (with SOCKS support) |
| Async Runtime | tokio |
| Frontend | Vanilla HTML/CSS/JS + Chart.js 4.4 (embedded in binary) |
| CI/CD | GitHub Actions (6-platform builds) |

---

## Project Structure

```
Proxy-Pulse/
├── src/
│   ├── main.rs          # Entry point, server setup
│   ├── api/             # REST API routes (public + admin)
│   ├── auth/            # Authentication, authorization, API keys
│   ├── db/              # Database operations (proxies, stats, subscriptions, auth, settings)
│   ├── checker.rs       # Proxy health checker & scorer
│   ├── scheduler.rs     # Background task scheduler
│   ├── sources.rs       # Proxy subscription source sync
│   ├── config.rs        # Checker configuration definition
│   ├── models.rs        # Data structures
│   └── mem_monitor.rs   # Memory usage monitor
├── static/              # Frontend assets (embedded in binary)
│   ├── index.html       # Dashboard page
│   ├── admin.html       # Admin panel
│   ├── login.html       # Login page
│   ├── settings.html    # Settings page
│   ├── css/             # Cyberpunk theme styles
│   ├── js/              # Dashboard logic + i18n engine
│   └── i18n/            # Translation files (en, zh-CN, zh-TW, ja)
├── docs/                # Documentation
│   ├── README_CN.md     # Chinese documentation
│   ├── LEGAL.md         # Legal terms (EN)
│   └── LEGAL_CN.md      # Legal terms (CN)
├── .github/workflows/
│   └── release.yml      # Build & Release (6 platforms)
├── run                  # Quick-start script (Linux/macOS)
├── run.ps1              # Quick-start script (Windows)
├── Cargo.toml           # Rust dependencies
└── LICENSE              # MIT License
```

---

## Legal Compliance

This project is committed to full compliance with all applicable laws and regulations, including but not limited to:

- **People's Republic of China Cybersecurity Law** (《中华人民共和国网络安全法》)
- **People's Republic of China Data Security Law** (《中华人民共和国数据安全法》)
- **People's Republic of China Personal Information Protection Law** (《中华人民共和国个人信息保护法》)
- **Regulations on the Security Protection of Computer Information Systems** (《计算机信息系统安全保护条例》)
- **Administrative Measures for Internet Information Services** (《互联网信息服务管理办法》)

**Users are solely responsible for ensuring their use of this software complies with all applicable local, national, and international laws and regulations.** The maintainers do not endorse or encourage any illegal use. See [Legal Terms](docs/LEGAL.md) for complete details.

---

## Contributing

Contributions are welcome! Please ensure all contributions comply with the project's legal and ethical standards.

## License

This project is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 OpenInfra Labs.

---

> **Disclaimer:** This software is provided for lawful purposes only. The authors and contributors assume no liability for misuse. See [Legal Terms](docs/LEGAL.md) for full details.
