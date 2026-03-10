# Proxy Pulse

**开源代理池管理与网络质量监控系统**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)]()
[![Axum](https://img.shields.io/badge/web-axum%200.7-blue.svg)]()
[![SQLite](https://img.shields.io/badge/database-SQLite-003B57.svg)]()

> **[English Documentation](README.md)** | **[法律免责声明](DISCLAIMER_CN.md)** | **[使用条款](TERMS_OF_USE_CN.md)**

---

## 概述

Proxy Pulse 是一款开源的代理池生命周期管理工具，专为**合法的网络基础设施监控、服务质量（QoS）测试以及企业代理资源管理**而设计。它帮助运维工程师、网络管理员和质量保障（QA）团队高效地管理、验证和监控其基础设施中代理服务器的健康状态。

基于 **Rust** 构建，具备高性能与低资源占用，内嵌 SQLite 数据库，实现零外部依赖部署。

## 合规使用场景

本项目仅为**合法合规**目的而构建，适用场景包括但不限于：

| 使用场景 | 说明 |
|---|---|
| **企业代理管理** | 管理和监控企业内部代理服务器池，用于内部网络路由和负载均衡。 |
| **网络质量监控** | 持续测试代理延迟、可用性和吞吐量，确保满足服务等级协议（SLA）要求。 |
| **API 与 Web 服务测试** | 验证 Web 服务在通过不同网络路径访问时是否正确响应。 |
| **学术与安全研究** | 研究网络性能、代理协议行为和连接模式，用于科研目的。 |
| **CDN 与边缘节点测试** | 验证内容分发和边缘节点在分布式基础设施中的可访问性。 |
| **自动化 QA 流水线** | 将代理健康检查集成到 CI/CD 工作流中，确保测试环境的可达性。 |

> **⚠️ 本软件不得用于、也非设计用于绕过网络安全控制、规避互联网审查、访问受限内容或任何违反适用法律法规的活动。** 详见 [DISCLAIMER_CN.md](DISCLAIMER_CN.md) 和 [TERMS_OF_USE_CN.md](TERMS_OF_USE_CN.md)。

---

## 部署与运行

### 环境要求

- **Rust** 1.75+（通过 [rustup](https://rustup.rs/) 安装）
- 无其他外部依赖（SQLite 已内嵌）

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse

# 构建 Release 版本
cargo build --release

# 二进制文件位于 target/release/proxy-pulse
```

### 配置

复制示例配置文件并按需编辑：

```bash
cp config.example.yaml config.yaml
```

```yaml
server:
  host: "0.0.0.0"
  port: 8080

database:
  url: "sqlite://proxy_pulse.db?mode=rwc"

sources:
  sync_interval_secs: 1800       # 全局来源同步间隔（30 分钟）
  providers:
    - type: file
      path: ./proxies.txt        # 每行一个代理: ip:port
    # - type: url
    #   url: https://example.com/proxy-list.txt

checker:
  interval_secs: 60              # 检测周期间隔（1 分钟）
  timeout_secs: 10               # 单个代理超时时间
  max_concurrent: 200            # 并发检测任务数
  targets:                       # 健康检测目标 URL（每代理并行检测）
    - https://httpbin.org/ip
    - https://www.cloudflare.com/cdn-cgi/trace

scoring:
  min_score: 60                  # "健康"代理的最低评分阈值
```

### 运行

```bash
# 使用默认 config.yaml 运行
cargo run --release

# 指定配置文件路径
cargo run --release -- /path/to/config.yaml

# 以演示模式运行（所有写入 API 返回 403）
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
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

---

## 功能特性

### 1. 订阅源管理
通过管理面板从多个可配置来源聚合代理：
- **URL 订阅** — GitHub 托管列表、内部注册中心、公开代理 API
- **本地文件来源** — 静态代理列表
- **独立同步间隔** — 每个订阅源可设置独立的同步频率（5 分钟至 24 小时）

支持在管理界面中添加、删除、启用/禁用和手动同步来源。

### 2. 智能代理去重
使用 `ip:port` 作为唯一标识自动去重，消除所有来源间的冗余健康检查和资源浪费。

### 3. 持续可用性检测
计划性代理健康验证，包括：
- TCP 连接测试
- 通过代理的 HTTP 往返验证
- 响应时间测量
- **并行**多目标检测（`httpbin.org`、`cloudflare.com`）— 每代理并发检测所有目标
- 可在 **3 分钟**内完成所有存活代理的轮训检测（200 并发 × 并行目标）

### 4. 自适应退避机制
智能退避机制，减少对失败代理的无效检测。成功检测的代理每 **3 分钟**重新检查一次：

| 连续失败次数 | 下次检测间隔 |
|---|---|
| 1 | 3 分钟 |
| 2 | 10 分钟 |
| 3 | 30 分钟 |
| 4 | 1 小时 |
| 5 | 3 小时 |
| 6 | 6 小时 |
| 7 | 12 小时 |
| 8 | 24 小时 |
| 9+ | 48 小时 |

### 5. 代理健康评分
每个代理获得综合健康评分（0-100），基于四个加权组件：

| 评分组件 | 最高分 | 计算方式 |
|---|---|---|
| **成功率** | 50 分 | `(成功次数 / 总检测次数) × 50` |
| **成功计数** | 10 分 | `min(成功次数, 10)` |
| **国家检测** | 10 分 | 已识别国家 = 10，未知 = 0 |
| **延迟** | 30 分 | ≤50ms = 30，≥10000ms = 0，线性插值 |

### 6. 代理元数据检测
自动检测代理元数据信息：
- **国家/地区** — 通过 ip-api.com → ipinfo.io → ipwho.is 级联查询
- **协议类型** — HTTP、HTTPS、SOCKS4、SOCKS5
- **匿名等级** — 透明、匿名、高匿

### 7. REST API
完整的 REST API，用于代理获取和管理：

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
GET  /api/v1/admin/proxy/list      # 列出所有代理（含管理详情）
POST /api/v1/admin/proxy/import    # 批量导入代理
POST /api/v1/admin/proxy/purge-dead # 清除失效代理
POST /api/v1/admin/proxy/delete/:id # 删除指定代理
GET  /api/v1/admin/source/list     # 列出订阅源
POST /api/v1/admin/source/add      # 添加订阅源
POST /api/v1/admin/source/delete/:id # 删除订阅源
POST /api/v1/admin/source/:id/toggle # 启用/禁用订阅源
POST /api/v1/admin/source/sync     # 触发手动同步
```

响应示例：
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

### 8. Web 仪表盘
赛博朋克风格的仪表盘，展示：
- 代理总数 / 存活数 / 失效数，平均评分与延迟
- 延迟分布图表
- 协议分布（环形图）
- 评分分布直方图
- Top 代理表格，支持一键复制
- 代理 API 卡片，支持格式切换（JSON / TXT / CSV）和排序选项

### 9. 管理面板
完整的管理界面：
- 代理列表，含状态、评分、延迟、成功/失败次数、下次检测时间
- 批量导入代理（每行一个）
- 订阅源管理，支持独立同步间隔
- 启用/禁用来源、删除代理、清除失效代理
- 演示模式指示器（使用 `--demo` 标志时显示）

### 10. 国际化（i18n）
多语言支持，内置四种语言：
- 🇺🇸 English (`en`)
- 🇨🇳 简体中文 (`zh-CN`)
- 🇹🇼 繁體中文 (`zh-TW`)
- 🇯🇵 日本語 (`ja`)

语言自动从浏览器检测，也可在界面中手动切换。

### 11. 演示模式
使用 `--demo` 标志运行以启用只读演示模式：
- 所有写入/变更 API 端点返回 `403 Forbidden`
- 管理面板显示演示模式横幅
- 适用于公开演示部署

---

## 技术栈

| 组件 | 技术 |
|---|---|
| 编程语言 | Rust 2021 Edition |
| Web 框架 | axum 0.7 |
| 数据库 | SQLite（通过 sqlx 0.7） |
| HTTP 客户端 | reqwest 0.12（支持 SOCKS） |
| 异步运行时 | tokio |
| 前端 | 原生 HTML/CSS/JS + Chart.js 4.4 |

---

## 项目结构

```
Proxy-Pulse/
├── src/
│   ├── main.rs          # 入口点、服务器启动
│   ├── api.rs           # REST API 路由与处理器
│   ├── db.rs            # 数据库操作
│   ├── models.rs        # 数据结构
│   ├── checker.rs       # 代理健康检测与评分
│   ├── scheduler.rs     # 后台任务调度器
│   ├── sources.rs       # 代理来源提供者
│   └── config.rs        # 配置加载器
├── static/
│   ├── index.html       # 仪表盘页面
│   ├── admin.html       # 管理面板页面
│   ├── css/style.css    # 赛博朋克风格样式
│   ├── js/
│   │   ├── app.js       # 仪表盘逻辑与图表
│   │   └── i18n.js      # 国际化引擎
│   └── i18n/            # 翻译文件（en、zh-CN、zh-TW、ja）
├── config.example.yaml  # 示例配置文件
├── Cargo.toml           # Rust 依赖配置
├── LICENSE              # MIT 许可证
├── DISCLAIMER.md        # 法律免责声明（英文）
├── DISCLAIMER_CN.md     # 法律免责声明（中文）
├── TERMS_OF_USE.md      # 使用条款（英文）
└── TERMS_OF_USE_CN.md   # 使用条款（中文）
```

---

## 法律合规

本项目承诺完全遵守所有适用的法律法规，包括但不限于：

- **《中华人民共和国网络安全法》**
- **《中华人民共和国数据安全法》**
- **《中华人民共和国个人信息保护法》**
- **《计算机信息系统安全保护条例》**
- **《互联网信息服务管理办法》**

### 明确禁止的行为

本软件**严禁**用于以下目的：

1. **翻越防火墙或规避网络审查**：不得使用本软件绕过中华人民共和国或任何其他国家/地区的网络访问限制。
2. **非法数据采集**：不得使用本软件非法获取、存储或传播个人信息或其他受保护数据。
3. **网络攻击**：不得使用本软件实施或协助任何形式的网络攻击、入侵或未经授权的访问。
4. **非法经营**：不得将本软件用于未经相关部门批准的互联网经营活动。
5. **传播违法信息**：不得使用本软件传播违法、有害、淫秽或其他法律禁止的信息。

### 用户责任

**用户须确保其对本软件的使用完全符合所在地区所有适用的法律法规。** 项目维护者不鼓励也不支持任何非法使用行为。详细法律条款请参见 [DISCLAIMER_CN.md](DISCLAIMER_CN.md) 和 [TERMS_OF_USE_CN.md](TERMS_OF_USE_CN.md)。

---

## 贡献

欢迎贡献！请阅读我们的贡献指南，并确保所有贡献符合本项目的法律和道德标准。

## 许可证

本项目基于 [MIT 许可证](LICENSE) 授权。

版权所有 (c) 2026 OpenInfra Labs。

---

> **免责声明：** 本软件仅供合法用途使用。作者和贡献者对任何滥用行为不承担任何责任。完整条款请参见 [DISCLAIMER_CN.md](DISCLAIMER_CN.md)。
