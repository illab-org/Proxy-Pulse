# Proxy Pulse

**Open-source proxy pool management and network quality monitoring system.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Go-00ADD8.svg)]()

> **[中文文档](README_CN.md)** | **[Legal Disclaimer](DISCLAIMER.md)** | **[Terms of Use](TERMS_OF_USE.md)**

---

## Overview

Proxy Pulse is an open-source proxy pool lifecycle management tool designed for **legitimate network infrastructure monitoring, quality-of-service (QoS) testing, and enterprise proxy resource management**. It helps DevOps engineers, network administrators, and QA teams efficiently manage, validate, and monitor the health of proxy servers in their infrastructure.

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

> **⚠️ This software is NOT designed, intended, or authorized for bypassing network security controls, circumventing internet censorship, accessing restricted content, or any activity that violates applicable laws and regulations.** See [DISCLAIMER.md](DISCLAIMER.md) and [TERMS_OF_USE.md](TERMS_OF_USE.md) for full details.

## Features

### 1. Proxy Source Aggregation
Aggregate proxies from multiple configurable sources:
- **Upstream proxy lists** (e.g., GitHub-hosted lists, internal registries)
- **Public proxy directories** (free proxy provider APIs)
- **Local proxy files** (static configuration files)

Sources are synchronized on a configurable schedule.

### 2. Smart Proxy Deduplication
Automatic deduplication using `ip:port` as the unique identifier, eliminating redundant health checks and resource waste.

### 3. Continuous Availability Checking
Scheduled proxy health validation including:
- TCP connection success rate
- HTTP round-trip verification
- Response time measurement

Check intervals are configurable (e.g., 1 min, 5 min).

### 4. Adaptive Check Backoff
Intelligent backoff mechanism to reduce unnecessary checks on failing proxies:

| Consecutive Failures | Check Interval |
|---|---|
| 1 | 1 minute |
| 3 | 5 minutes |
| 5 | 15 minutes |
| 10+ | 60 minutes |

### 5. Proxy Health Scoring
Each proxy receives a composite health score (0–100) based on:
- Success rate
- Average response latency
- Uptime stability

```
Example: 1.2.3.4:8080  score=92
```

Consumers can filter by minimum score threshold.

### 6. Multi-Target Testing
Validate proxies against multiple test endpoints:
- `httpbin.org`
- `github.com`
- `google.com`
- `cloudflare.com`

This measures proxy capability across different network conditions.

### 7. Proxy Metadata Detection
Automatically detect proxy metadata:
- **Country / Region** (GeoIP)
- **Protocol** (HTTP, HTTPS, SOCKS4, SOCKS5)
- **Anonymity Level** (Transparent, Anonymous, Elite)

### 8. Proxy History Tracking
Full historical records per proxy:
- Total success / failure counts
- Average latency over time
- Last successful check timestamp
- Stability trend analysis

### 9. REST API
Simple and clean REST API for proxy consumption:

```
GET /api/v1/proxy/random       # Get a random healthy proxy
GET /api/v1/proxy/top          # Get top-scored proxies
GET /api/v1/proxy/country/us   # Filter by country
GET /api/v1/proxy/stats        # Pool statistics
```

Response example:
```json
{
  "proxy": "1.2.3.4:8080",
  "protocol": "http",
  "country": "US",
  "score": 92,
  "latency_ms": 120
}
```

### 10. Web Dashboard
A lightweight web dashboard showing:
- Total proxy count
- Available (healthy) proxy count
- Country distribution chart
- Latency distribution histogram
- Real-time health status

---

## Quick Start

```bash
# Clone the repository
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse

# Build
go build -o proxy-pulse ./cmd/proxy-pulse

# Run with default configuration
./proxy-pulse --config config.yaml
```

## Configuration

See [`config.example.yaml`](config.example.yaml) for a complete configuration reference.

```yaml
server:
  port: 8080

sources:
  sync_interval: 30m
  providers:
    - type: file
      path: ./proxies.txt
    - type: url
      url: https://example.com/proxy-list.txt

checker:
  interval: 5m
  timeout: 10s
  targets:
    - https://httpbin.org/ip
    - https://www.cloudflare.com

scoring:
  min_score: 60
```

---

## Legal Compliance

This project is committed to full compliance with all applicable laws and regulations, including but not limited to:

- **People's Republic of China Cybersecurity Law** (《中华人民共和国网络安全法》)
- **People's Republic of China Data Security Law** (《中华人民共和国数据安全法》)
- **People's Republic of China Personal Information Protection Law** (《中华人民共和国个人信息保护法》)
- **Regulations on the Security Protection of Computer Information Systems** (《计算机信息系统安全保护条例》)
- **Administrative Measures for Internet Information Services** (《互联网信息服务管理办法》)

**Users are solely responsible for ensuring their use of this software complies with all applicable local, national, and international laws and regulations.** The maintainers do not endorse or encourage any illegal use. See [DISCLAIMER.md](DISCLAIMER.md) and [TERMS_OF_USE.md](TERMS_OF_USE.md) for complete legal terms.

---

## Contributing

Contributions are welcome! Please read our contributing guidelines and ensure all contributions comply with the project's legal and ethical standards.

## License

This project is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 OpenInfra Labs.

---

> **Disclaimer:** This software is provided for lawful purposes only. The authors and contributors assume no liability for misuse. See [DISCLAIMER.md](DISCLAIMER.md) for full terms.
