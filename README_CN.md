# Proxy Pulse

**开源代理池管理与网络质量监控系统**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language](https://img.shields.io/badge/language-Go-00ADD8.svg)]()

> **[English Documentation](README.md)** | **[法律免责声明](DISCLAIMER_CN.md)** | **[使用条款](TERMS_OF_USE_CN.md)**

---

## 概述

Proxy Pulse 是一款开源的代理池生命周期管理工具，专为**合法的网络基础设施监控、服务质量（QoS）测试以及企业代理资源管理**而设计。它帮助运维工程师、网络管理员和质量保障（QA）团队高效地管理、验证和监控其基础设施中代理服务器的健康状态。

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

## 功能特性

### 1. 代理来源聚合
从多个可配置来源聚合代理：
- **上游代理列表**（如 GitHub 托管列表、内部注册中心）
- **公开代理目录**（免费代理提供商 API）
- **本地代理文件**（静态配置文件）

来源按可配置的周期自动同步。

### 2. 智能代理去重
使用 `ip:port` 作为唯一标识自动去重，消除冗余健康检查和资源浪费。

### 3. 持续可用性检测
计划性代理健康验证，包括：
- TCP 连接成功率
- HTTP 往返验证
- 响应时间测量

检测间隔可配置（如 1 分钟、5 分钟）。

### 4. 自适应退避机制
智能退避机制，减少对失败代理的无效检测：

| 连续失败次数 | 检测间隔 |
|---|---|
| 1 | 1 分钟 |
| 3 | 5 分钟 |
| 5 | 15 分钟 |
| 10+ | 60 分钟 |

### 5. 代理健康评分
每个代理获得综合健康评分（0-100），评分依据：
- 成功率
- 平均响应延迟
- 运行稳定时间

```
示例：1.2.3.4:8080  score=92
```

使用方可按最低评分阈值过滤代理。

### 6. 多目标检测
支持对多个测试端点验证代理：
- `httpbin.org`
- `github.com`
- `google.com`
- `cloudflare.com`

衡量代理在不同网络条件下的能力。

### 7. 代理元数据检测
自动检测代理元数据信息：
- **国家/地区**（GeoIP）
- **协议类型**（HTTP、HTTPS、SOCKS4、SOCKS5）
- **匿名等级**（透明、匿名、高匿）

### 8. 代理历史追踪
每个代理的完整历史记录：
- 累计成功/失败次数
- 历史平均延迟
- 最后一次成功检测时间
- 稳定性趋势分析

### 9. REST API
简洁的 REST API，用于获取代理：

```
GET /api/v1/proxy/random       # 获取随机健康代理
GET /api/v1/proxy/top          # 获取评分最高的代理
GET /api/v1/proxy/country/us   # 按国家筛选
GET /api/v1/proxy/stats        # 代理池统计信息
```

响应示例：
```json
{
  "proxy": "1.2.3.4:8080",
  "protocol": "http",
  "country": "US",
  "score": 92,
  "latency_ms": 120
}
```

### 10. Web 仪表盘
轻量级 Web 仪表盘，显示：
- 代理总数
- 可用（健康）代理数量
- 国家分布图表
- 延迟分布直方图
- 实时健康状态

---

## 快速开始

```bash
# 克隆仓库
git clone https://github.com/OpenInfra-Labs/Proxy-Pulse.git
cd Proxy-Pulse

# 构建
go build -o proxy-pulse ./cmd/proxy-pulse

# 使用默认配置运行
./proxy-pulse --config config.yaml
```

## 配置

参见 [`config.example.yaml`](config.example.yaml) 获取完整配置参考。

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

## 法律合规

本项目承诺完全遵守所有适用的法律法规，包括但不限于：

- **《中华人民共和国网络安全法》**
- **《中华人民共和国数据安全法》**
- **《中华人民共和国个人信息保护法》**
- **《计算机信息系统安全保护条例》**
- **《互联网信息服务管理办法》**
- **《关于加强对利用互联网络从事经营活动管理的通知》**

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
