# Changelog

All notable changes to Proxy Pulse are documented in this file.

[中文版](CHANGELOG.zh-CN.md) | [返回](../README.md)

## [Unreleased]

### Fixed
- Docker image tags now use lowercase repository owner name
- Version fetch uses `cache: no-cache` with error logging for debugging
- Database import now auto-restarts the service instead of requiring manual restart

### Changed
- Moved changelogs to `docs/` folder

## [1.2.1] - 2026-03-10

### Added
- Database export/import endpoints (`GET/POST /api/v1/admin/db`)
- Export/import buttons in System Settings admin panel
- Version footer in admin page

### Fixed
- Proxy API dropdown z-index — `charts-row` stacking context above score distribution card

## [1.2.0] - 2026-03-10

### Added
- Auto-update system — checks GitHub Releases, downloads and replaces binary automatically
- System Settings admin page (auto-update toggle, default language/timezone/theme)
- Docker multi-arch image build (linux/amd64, linux/arm64) in CI release pipeline

## [1.1.3] - 2026-03-10

### Fixed
- Token selector moved below Country filter for better UX
- Dropdown z-index clipping issue with score distribution card

## [1.1.2] - 2026-03-10

### Added
- Searchable country filter in Proxy API export card
- Score distribution chart split: 80–100 range divided into 80–90 and 90–100
- Chart vertically centered

### Fixed
- Token create option positioned inside dropdown
- z-index clipping on token/country dropdowns
- CI fetches full git history for accurate tag detection

## [1.1.1] - 2026-03-10

### Changed
- **Breaking:** Scoring system rewritten — moved checker config from `config.yaml` to database
- `config.yaml` eliminated; all settings now managed via admin UI

### Added
- Synced `run.ps1` (Windows) with `run` (Unix) launch script
- Docs reorganized into `docs/` folder
- Smart CI version bump logic

## [1.0.9] - 2026-03-10

### Added
- Timezone personalization in user settings page

## [1.0.8] - 2026-03-10

### Fixed
- API links card overflow expanding beyond grid column boundary

## [1.0.7] - 2026-03-10

### Added
- Auto-convert GitHub blob URLs to raw URLs for subscription sources
- Run script supports `restart` command

## [1.0.6] - 2026-03-10

### Fixed
- Dynamic footer version fetched from `/api/v1/health` endpoint
- Token dropdown overflow clipping

## [1.0.5] - 2026-03-10

### Fixed
- Login page blank screen caused by `i18n.js` not loaded, leaving `opacity: 0`

## [1.0.4] - 2026-03-10

### Performance
- Added `checked_at` index on `check_logs` table for faster queries
- VACUUM after cleanup to reclaim disk space

## [1.0.3] - 2026-03-10

### Performance
- Daily log rotation with 7-day retention
- Memory monitor interval reduced from 1s to 60s
- Release binary optimizations: opt-level 3, thin-LTO, symbol stripping, single codegen unit

### Added
- CI auto-bumps patch version and creates release on compiled file changes

## [1.0.2] - 2026-03-10

### Added
- Docker support with multi-stage Dockerfile
- Pre-built binary Docker images (no in-container compilation)

## [1.0.1] - 2026-03-10

### Added
- Windows support
- Auto-open browser on launch

## [1.0.0] - 2026-03-10

### Added
- **Core proxy engine** — Rust/axum/SQLite stack with embedded static assets
- **Proxy checker** — concurrent health checks with adaptive retry backoff, 3-min full cycle coverage
- **Scoring system** — composite score based on latency, success rate, and uptime
- **Subscription sources** — import proxies from URLs with per-source auto-sync intervals and priority scheduling
- **Admin panel** — manage proxies, subscription sources, users, and system settings
- **Multi-user system** — role-based access (admin/user), avatar, personal settings
- **Authentication** — login page, password change, API key management with expiration
- **API endpoints** — JSON, TXT, CSV export formats with token-based access
- **Proxy API card** — token selector, country filter, format/sort/protocol options
- **Dashboard** — real-time stats, score distribution chart, latency distribution, protocol breakdown
- **i18n** — English, Simplified Chinese, Traditional Chinese, Japanese
- **Light/Dark theme** — system-aware theme switcher with per-user preference
- **Demo mode** — read-only mode for public showcases
- **Performance** — jemalloc allocator, optimized SQLite pool (8 connections), memory monitoring
- **Deployment** — auto-download run script, `rust-embed` for single-binary distribution
- **CI/CD** — GitHub Actions release workflow with cross-compilation (Linux amd64/arm64, macOS amd64/arm64, Windows)

[Unreleased]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.2.1...HEAD
[1.2.1]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.1.3...v1.2.0
[1.1.3]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.9...v1.1.1
[1.0.9]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.8...v1.0.9
[1.0.8]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.7...v1.0.8
[1.0.7]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.6...v1.0.7
[1.0.6]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.5...v1.0.6
[1.0.5]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.4...v1.0.5
[1.0.4]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.2...v1.0.4
[1.0.3]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/OpenInfra-Labs/Proxy-Pulse/releases/tag/v1.0.0
