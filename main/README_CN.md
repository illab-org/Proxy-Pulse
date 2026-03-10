# Proxy Pulse

**开源代理池管理与网络质量监控系统**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)]()

> **[English](README.md)**

---

## 简介

Proxy Pulse 是一个开源的代理池全生命周期管理工具，专为**合法的网络基础设施监控、服务质量（QoS）测试和企业代理资源管理**而设计。帮助运维工程师、网络管理员和测试团队高效管理、验证和监控代理服务器的健康状况。

### 核心功能

- **多源代理聚合** — URL 订阅、本地文件、手动批量导入
- **自动健康检查** — 并行多目标检测，自适应重试退避
- **代理评分系统** — 基于成功率、延迟、稳定性的综合评分
- **元数据检测** — 国家/地区、协议类型、匿名等级
- **REST API** — 完整的代理获取接口（JSON、TXT、CSV 导出）
- **Web 仪表盘** — 实时监控图表和统计数据
- **管理面板** — 订阅源管理、用户管理、批量操作
- **多语言** — English、简体中文、繁體中文、日本語
- **跨平台** — Linux (amd64/arm64)、macOS (amd64/arm64)

> **⚠️ 本软件不用于、不旨在、也未被授权用于绕过网络安全控制、规避互联网审查、访问受限内容，或任何违反适用法律法规的活动。**

---

## 快速开始

本分支包含预编译的二进制文件，无需安装任何编译工具。

### 1. 下载

```bash
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse
```

### 2. 配置

```bash
cp config.example.yaml config.yaml
# 根据需要编辑 config.yaml
```

### 3. 运行

```bash
# 启动（自动检测当前系统和架构）
./run

# 查看运行状态
./run status

# 停止
./run stop
```

`run` 脚本会自动从 `build/` 目录中选择适合当前平台的二进制文件，并在后台启动。

### 4. 访问

| 地址 | 说明 |
|---|---|
| `http://localhost:8080` | Web 仪表盘 |
| `http://localhost:8080/admin` | 管理面板 |
| `http://localhost:8080/settings` | 个人设置 |

首次启动时会提示创建管理员账户。

---

## 支持平台

| 平台 | 二进制文件 |
|---|---|
| Linux x86_64 | `build/proxy-pulse-linux-amd64` |
| Linux ARM64 | `build/proxy-pulse-linux-arm64` |
| macOS x86_64 | `build/proxy-pulse-darwin-amd64` |
| macOS ARM64 (Apple Silicon) | `build/proxy-pulse-darwin-arm64` |

---

## 配置说明

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

## API 接口

### 公开接口
```
GET  /api/v1/proxy/random          # 随机获取健康代理
GET  /api/v1/proxy/top?limit=10    # 评分最高的代理
GET  /api/v1/proxy/country/:code   # 按国家代码筛选
GET  /api/v1/proxy/all             # 所有代理（分页）
GET  /api/v1/proxy/json            # 导出健康代理（JSON）
GET  /api/v1/proxy/txt             # 导出纯文本（ip:port）
GET  /api/v1/proxy/csv             # 导出 CSV
GET  /api/v1/proxy/stats           # 代理池统计
GET  /api/v1/health                # 健康检查
```

### 管理接口（需认证）
```
GET  /api/v1/admin/proxy/list      # 代理列表
POST /api/v1/admin/proxy/import    # 批量导入
POST /api/v1/admin/proxy/purge-dead # 清除死亡代理
POST /api/v1/admin/proxy/delete/:id # 删除代理
GET  /api/v1/admin/source/list     # 订阅源列表
POST /api/v1/admin/source/add      # 添加订阅源
POST /api/v1/admin/source/delete/:id # 删除订阅源
POST /api/v1/admin/source/sync     # 手动同步所有源
```

---

## 从源码构建

如果你想使用**最新的开发版本**或需要自定义构建，请切换到 [`source`](https://github.com/OpenInfra-Labs/Proxy-Pulse/tree/source) 分支：

```bash
git checkout source
```

### 环境要求

- **Rust** 1.75+（通过 [rustup](https://rustup.rs/) 安装）
- 无其他依赖（SQLite 已内嵌）

### 构建与运行

```bash
cargo build --release
./target/release/proxy-pulse
```

> **提示：** 从 `source` 分支构建需要 Rust 开发环境，编译时间比直接使用 `main` 分支的预编译文件更长。使用 `source` 分支意味着你可以获取最新功能和修复，但需要具备一定的 Rust 开发知识。

---

## 许可证

本项目采用 [MIT 许可证](LICENSE)。

Copyright (c) 2026 OpenInfra Labs.
