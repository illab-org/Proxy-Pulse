# 更新日志

Proxy Pulse 的所有重要变更记录。

[English Version](CHANGELOG.md) | [返回](../README.md)

## [未发布]

### 修复
- Docker 镜像标签现在使用小写仓库所有者名称
- 版本号获取使用 `cache: no-cache` 并输出错误日志便于调试
- 数据库导入后现在自动重启服务，无需手动重启

### 变更
- 更新日志移至 `docs/` 目录

## [1.2.1] - 2026-03-10

### 新增
- 数据库导出/导入接口（`GET/POST /api/v1/admin/db`）
- 系统设置页面新增导出/导入数据库按钮
- 管理页面底部新增版本号显示

### 修复
- 代理 API 下拉菜单 z-index 问题 —— `charts-row` 层叠上下文覆盖评分分布卡片

## [1.2.0] - 2026-03-10

### 新增
- 自动更新系统 —— 自动检查 GitHub Releases，下载并替换二进制文件
- 系统设置管理页面（自动更新开关、默认语言/时区/主题）
- CI 发布流水线新增 Docker 多架构镜像构建（linux/amd64, linux/arm64）

## [1.1.3] - 2026-03-10

### 修复
- Token 选择器移至国家筛选器下方，优化使用体验
- 下拉菜单被评分分布卡片遮挡的 z-index 问题

## [1.1.2] - 2026-03-10

### 新增
- 代理 API 导出卡片新增可搜索的国家筛选器
- 评分分布图拆分：80–100 区间细分为 80–90 和 90–100
- 图表垂直居中显示

### 修复
- Token 创建选项定位在下拉菜单内
- Token/国家下拉菜单 z-index 裁剪问题
- CI 获取完整 git 历史以准确检测标签

## [1.1.1] - 2026-03-10

### 变更
- **重大变更：** 评分系统重写 —— 检查器配置从 `config.yaml` 迁移到数据库
- 移除 `config.yaml`；所有设置通过管理界面管理

### 新增
- `run.ps1`（Windows）与 `run`（Unix）启动脚本同步
- 文档整理至 `docs/` 目录
- CI 智能版本号递增逻辑

## [1.0.9] - 2026-03-10

### 新增
- 用户设置页面新增时区个性化选项

## [1.0.8] - 2026-03-10

### 修复
- API 链接卡片溢出超出网格列边界

## [1.0.7] - 2026-03-10

### 新增
- 自动将 GitHub blob URL 转换为 raw URL（订阅源）
- 启动脚本支持 `restart` 命令

## [1.0.6] - 2026-03-10

### 修复
- 页脚版本号改为从 `/api/v1/health` 接口动态获取
- Token 下拉菜单溢出裁剪

## [1.0.5] - 2026-03-10

### 修复
- 登录页面空白 —— 因未加载 `i18n.js` 导致 `opacity: 0` 隐藏内容

## [1.0.4] - 2026-03-10

### 性能
- 为 `check_logs` 表的 `checked_at` 字段添加索引，加速查询
- 清理后执行 VACUUM 回收磁盘空间

## [1.0.3] - 2026-03-10

### 性能
- 每日日志轮转，保留 7 天
- 内存监控间隔从 1 秒调整为 60 秒
- 发布二进制优化：opt-level 3、thin-LTO、符号剥离、单代码生成单元

### 新增
- CI 在编译文件变更时自动递增补丁版本号并创建发布

## [1.0.2] - 2026-03-10

### 新增
- Docker 支持，多阶段 Dockerfile
- 使用预编译二进制的 Docker 镜像（无需容器内编译）

## [1.0.1] - 2026-03-10

### 新增
- Windows 平台支持
- 启动时自动打开浏览器

## [1.0.0] - 2026-03-10

### 新增
- **代理引擎核心** —— Rust/axum/SQLite 技术栈，静态资源内嵌
- **代理检查器** —— 并发健康检查，自适应重试退避，3 分钟全量检查周期
- **评分系统** —— 基于延迟、成功率和在线时间的综合评分
- **订阅源** —— 从 URL 导入代理，支持每源独立的自动同步间隔和优先级调度
- **管理面板** —— 管理代理、订阅源、用户和系统设置
- **多用户系统** —— 基于角色的访问控制（管理员/用户）、头像、个人设置
- **认证系统** —— 登录页面、密码修改、API 密钥管理（支持过期时间）
- **API 接口** —— JSON、TXT、CSV 导出格式，基于 Token 的访问控制
- **代理 API 卡片** —— Token 选择器、国家筛选、格式/排序/协议选项
- **仪表盘** —— 实时统计、评分分布图、延迟分布图、协议占比图
- **国际化** —— 英语、简体中文、繁体中文、日语
- **明暗主题** —— 跟随系统的主题切换，支持用户偏好
- **演示模式** —— 公开展示的只读模式
- **性能优化** —— jemalloc 内存分配器、优化的 SQLite 连接池（8 连接）、内存监控
- **部署** —— 自动下载启动脚本、`rust-embed` 单二进制分发
- **CI/CD** —— GitHub Actions 发布工作流，交叉编译（Linux amd64/arm64、macOS amd64/arm64、Windows）

[未发布]: https://github.com/OpenInfra-Labs/Proxy-Pulse/compare/v1.2.1...HEAD
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
