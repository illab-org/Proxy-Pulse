# Proxy Pulse

**开源代理池管理与网络质量监控系统**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](../LICENSE)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)]()
[![Axum](https://img.shields.io/badge/web-axum%200.7-blue.svg)]()
[![SQLite](https://img.shields.io/badge/database-SQLite-003B57.svg)]()

> **[English Documentation](../README.md)** | **[法律条款](LEGAL_CN.md)**

---

## 概述

Proxy Pulse 是一款开源的代理池生命周期管理工具，专为**合法的网络基础设施监控、服务质量（QoS）测试以及企业代理资源管理**而设计。它帮助运维工程师、网络管理员和质量保障（QA）团队高效地管理、验证和监控代理服务器的健康状态。

基于 **Rust** 构建，具备高性能与低资源占用，内嵌 SQLite 数据库，实现零外部依赖部署。

## 合规使用场景

| 使用场景 | 说明 |
|---|---|
| **企业代理管理** | 管理和监控企业内部代理服务器池，用于内部网络路由和负载均衡。 |
| **网络质量监控** | 持续测试代理延迟、可用性和吞吐量，确保满足 SLA 要求。 |
| **API 与 Web 服务测试** | 验证 Web 服务在通过不同网络路径访问时是否正确响应。 |
| **学术与安全研究** | 研究网络性能、代理协议行为和连接模式。 |
| **CDN 与边缘节点测试** | 验证内容分发和边缘节点在分布式基础设施中的可访问性。 |
| **自动化 QA 流水线** | 将代理健康检查集成到 CI/CD 工作流中。 |

> **⚠️ 本软件不得用于绕过网络安全控制、规避互联网审查、访问受限内容或任何违反适用法律法规的活动。** 详见 [法律条款](LEGAL_CN.md)。

---

## 快速启动

无需安装 Rust —— 只需下载 `run` 脚本即可自动完成一切：

**Linux / macOS：**

```bash
curl -fsSL -o run https://raw.githubusercontent.com/OpenInfra-Labs/Proxy-Pulse/main/run && chmod +x run
./run
```

**Windows (PowerShell)：**

```powershell
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/OpenInfra-Labs/Proxy-Pulse/main/run.ps1" -OutFile run.ps1
.\run.ps1
```

**命令说明：**

| 命令 | 说明 |
|------|------|
| `./run` | 启动（自动下载最新二进制） |
| `./run status` | 查看运行状态 |
| `./run stop` | 停止服务 |
| `./run update` | 更新脚本和二进制到最新版本 |

`run` 脚本会自动：
- 检测当前操作系统和 CPU 架构（支持 Linux、macOS、Windows × amd64/arm64）
- 从 GitHub Releases 下载对应的预编译二进制文件
- 每次启动时检查脚本和二进制是否有更新
- 在桌面系统上自动打开浏览器访问控制面板
- 在后台启动服务

---

## 部署与运行

### 环境要求

- **Rust** 1.75+（通过 [rustup](https://rustup.rs/) 安装）
- 无其他外部依赖（SQLite 已内嵌）

### 从源码构建

```bash
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse
cargo build --release
```

### 配置

Proxy Pulse 开箱即用，所有配置均可通过环境变量和管理面板设置：

**环境变量：**

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DATABASE_URL` | `sqlite://proxy_pulse.db?mode=rwc` | 数据库连接 URL |
| `HOST` | `0.0.0.0` | 监听地址 |
| `PORT` | `8080` | 监听端口 |

**检测配置**（通过管理面板 → 检测设置）：

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| 检测间隔 | 60 秒 | 代理检测周期 |
| 请求超时 | 10 秒 | 单个代理超时时间 |
| 最大并发数 | 200 | 并发检测任务数 |
| 检测目标 | httpbin.org, cloudflare.com | 健康检测目标 URL |

### 运行

```bash
# 运行（使用默认配置）
cargo run --release

# 自定义数据库路径
DATABASE_URL="sqlite:///data/proxy.db?mode=rwc" cargo run --release

# 自定义端口
PORT=3000 cargo run --release

# 演示模式（所有写入 API 返回 403）
cargo run --release -- --demo
```

### 访问

| 地址 | 说明 |
|---|---|
| `http://localhost:8080` | Web 仪表盘 |
| `http://localhost:8080/admin` | 管理面板 |
| `http://localhost:8080/api/v1/health` | 健康检查接口 |

### Systemd 服务（Linux）

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

## 功能特性

### 1. 订阅源管理
通过管理面板从多个来源聚合代理：
- **URL 订阅** — GitHub 托管列表、内部注册中心、公开代理 API
- **独立同步间隔** — 每个订阅源可设置独立的同步频率（5 分钟至 24 小时）
- 支持在管理界面中添加、删除、启用/禁用和手动同步

### 2. 智能代理去重
使用 `ip:port` 作为唯一标识自动去重，消除冗余健康检查和资源浪费。

### 3. 持续可用性检测
- TCP 连接测试 + HTTP 往返验证
- 响应时间测量
- **并行**多目标检测 — 每代理并发检测所有目标
- 可在 **3 分钟**内完成所有存活代理的轮训检测

### 4. 自适应退避机制
智能退避机制，减少对失败代理的无效检测：

| 连续失败次数 | 下次检测间隔 |
|---|---|
| 1 | 3 分钟 |
| 2 | 10 分钟 |
| 3 | 30 分钟 |
| 4–5 | 1–3 小时 |
| 6–7 | 6–12 小时 |
| 8+ | 24–48 小时 |

### 5. 代理健康评分
每个代理获得综合健康评分（0–100），基于五个加权组件：

| 评分组件 | 最高分 | 计算方式 |
|---|---|---|
| **成功率** | 60 分 | `成功率 × 60` |
| **成功计数** | 10 分 | 相对于池中最大成功次数的比例 |
| **国家分级** | 6 分 | 一级地区 6 分、二级 4.5 分、三级 3 分 |
| **协议类型** | 4 分 | SOCKS5=4、HTTPS=3、SOCKS4=2、HTTP=1 |
| **延迟** | 20 分 | ≤100ms=20、≥5000ms=0，线性插值 |

### 6. 代理元数据检测
- **国家/地区** — 通过 ip-api.com → ipinfo.io → ipwho.is 级联查询
- **协议类型** — HTTP、HTTPS、SOCKS4、SOCKS5
- **匿名等级** — 透明、匿名、高匿

### 7. REST API

#### 公开端点
```
GET  /api/v1/proxy/random          # 获取随机健康代理
GET  /api/v1/proxy/top?limit=10    # 获取评分最高的代理
GET  /api/v1/proxy/country/:code   # 按国家代码筛选
GET  /api/v1/proxy/all             # 所有代理（分页）
GET  /api/v1/proxy/json            # 导出健康代理为 JSON
GET  /api/v1/proxy/txt             # 导出为纯文本（ip:port）
GET  /api/v1/proxy/csv             # 导出为 CSV
GET  /api/v1/proxy/stats           # 代理池统计及分布
GET  /api/v1/health                # 健康检查
```

#### 管理端点
```
GET  /api/v1/admin/proxy/list           # 列出所有代理（含管理详情）
POST /api/v1/admin/proxy/import         # 批量导入代理
POST /api/v1/admin/proxy/purge-dead     # 清除失效代理
POST /api/v1/admin/proxy/delete/:id     # 删除指定代理
GET  /api/v1/admin/source/list          # 列出订阅源
POST /api/v1/admin/source/add           # 添加订阅源
POST /api/v1/admin/source/delete/:id    # 删除订阅源
POST /api/v1/admin/source/:id/toggle    # 启用/禁用订阅源
POST /api/v1/admin/source/sync          # 触发手动同步
GET  /api/v1/admin/settings/checker     # 获取检测配置
POST /api/v1/admin/settings/checker     # 保存检测配置
```

### 8. Web 仪表盘
赛博朋克风格的仪表盘 — 代理统计概览、延迟分布图、协议分布环形图、评分直方图、Top 代理表格、API 导出卡片。

### 9. 管理面板
- 代理列表（状态、评分、延迟、成功/失败次数、下次检测时间）
- 批量导入代理
- 订阅源管理（独立同步间隔）
- 检测设置（间隔、超时、并发数、目标 URL）
- 用户管理
- 演示模式指示器

### 10. 国际化（i18n）
- 🇺🇸 English · 🇨🇳 简体中文 · 🇹🇼 繁體中文 · 🇯🇵 日本語
- 语言自动从浏览器检测，可在界面中手动切换

### 11. 演示模式
使用 `--demo` 标志启用只读模式 — 所有写入 API 返回 `403 Forbidden`，管理面板显示演示横幅。

---

## 技术栈

| 组件 | 技术 |
|---|---|
| 编程语言 | Rust 2021 Edition |
| Web 框架 | axum 0.7 |
| 数据库 | SQLite（通过 sqlx 0.7） |
| HTTP 客户端 | reqwest 0.12（支持 SOCKS） |
| 异步运行时 | tokio |
| 前端 | 原生 HTML/CSS/JS + Chart.js 4.4（编译嵌入二进制） |
| CI/CD | GitHub Actions（自动构建 6 平台） |

---

## 项目结构

```
Proxy-Pulse/
├── src/
│   ├── main.rs          # 入口点、服务器启动
│   ├── api/             # REST API 路由（公开 + 管理）
│   ├── auth/            # 认证与授权、API Key
│   ├── db/              # 数据库操作（代理、统计、订阅、认证、系统设置）
│   ├── checker.rs       # 代理健康检测与评分
│   ├── scheduler.rs     # 后台任务调度器
│   ├── sources.rs       # 代理订阅源同步
│   ├── config.rs        # 检测配置定义
│   ├── models.rs        # 数据结构
│   └── mem_monitor.rs   # 内存使用监控
├── static/              # 前端资源（编译嵌入二进制）
│   ├── index.html       # 仪表盘页面
│   ├── admin.html       # 管理面板
│   ├── login.html       # 登录页面
│   ├── settings.html    # 设置页面
│   ├── css/             # 赛博朋克风格样式
│   ├── js/              # 仪表盘逻辑 + 国际化引擎
│   └── i18n/            # 翻译文件（en、zh-CN、zh-TW、ja）
├── docs/                # 项目文档
├── run                  # 快速启动脚本（Linux/macOS）
├── run.ps1              # 快速启动脚本（Windows）
├── Cargo.toml           # Rust 依赖配置
└── LICENSE              # MIT 许可证
```

---

## 法律合规

本项目承诺完全遵守所有适用法律法规。本软件**严禁**用于：

1. 翻越防火墙或规避网络审查
2. 非法数据采集或传播违法信息
3. 网络攻击或未经授权的访问
4. 未经许可的互联网经营活动

**用户须确保其使用完全符合所在地区所有适用的法律法规。** 详细法律条款请参见 [LEGAL_CN.md](LEGAL_CN.md)。

---

## 贡献

欢迎贡献！请确保所有贡献符合本项目的法律和道德标准。

## 许可证

本项目基于 [MIT 许可证](../LICENSE) 授权。版权所有 (c) 2026 OpenInfra Labs。

---

> **免责声明：** 本软件仅供合法用途使用。作者和贡献者对任何滥用行为不承担任何责任。完整条款请参见 [LEGAL_CN.md](LEGAL_CN.md)。
