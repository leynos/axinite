# Axinite

*一个承袭 IronClaw 血统的安全型个人 AI 助手，重点是让数据留在本地，
让扩展能力掌握在你自己手里。*

[English](README.md) | [简体中文](README.zh-CN.md)

Axinite 继承了 IronClaw 相当多的运行时、CLI 入口和文档内容。这份传承
是刻意保留的：这个分支沿用了上游的安全与自动化基础，同时把项目带往自
己的方向。现阶段，编译出来的二进制、crate 名称，以及不少内部路径仍然
使用 `ironclaw`。

______________________________________________________________________

## 为什么选择 axinite？

- **默认本地优先**：数据、密钥与审计痕迹都保留在你的机器上，而不是漂
  到别人的服务里。
- **安全不是附带品**：WASM 沙箱、宿主侧密钥注入、泄露检测与网络白名
  单都是运行时的一部分。
- **它不只是聊天框**：REPL、Web 网关、Webhook、定时任务、后台作业
  与渠道集成都放在同一个助手里。
- **能力可以继续生长**：你可以安装 MCP 服务器、加入 WASM 工具，并扩
  展代理能力，而不必等供应商碰巧发布你需要的功能。

______________________________________________________________________

## 快速开始

### 安装

Axinite 是这个分支的名称；当前的 crate、可执行文件与初始化命令仍然叫
`ironclaw`。

```bash
cargo build
target/debug/ironclaw onboard --quick
```

### 基本用法

```bash
# 发送一条消息后退出
target/debug/ironclaw --message "Summarize what this machine is ready to do."

# 查看健康状态与已配置服务
target/debug/ironclaw status

# 检查工作空间记忆功能
target/debug/ironclaw memory status
```

______________________________________________________________________

## 功能特性

- 安全运行时：提供 WASM 沙箱、提示内容净化、凭据保护与网络白名单。
- 多种入口：交互式 CLI、单消息模式、Web 网关、Webhook、定时任务，以
  及系统服务支持。
- 持久化工作空间记忆：可对笔记、日志与身份文件进行混合搜索。
- 可扩展工具链：包含内建工具、MCP 服务器、注册表扩展，以及动态构建的
  WASM 工具。
- 灵活的模型提供方支持：初始化流程可配置 NEAR AI、OpenAI 兼容端点、
  Ollama、Bedrock 等后端。

______________________________________________________________________

## 延伸阅读

- [LLM 提供方指南](docs/LLM_PROVIDERS.md) — 提供方配置与环境变量说明。
- [初始化规范](src/setup/README.md) — `ironclaw onboard` 实际会配置什么。
- [工作空间与记忆](src/workspace/README.md) — 持久记忆布局与相关工具。
- [构建渠道](docs/BUILDING_CHANNELS.md) — 如何重新构建随仓库分发的渠道
  产物。
- [贡献指南](CONTRIBUTING.md) — 开发流程与审查分级。
- [变更日志](CHANGELOG.md) — 发布历史。

______________________________________________________________________

## 许可

本项目采用 MIT 或 Apache-2.0 双许可证。详情请见
[LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)。

______________________________________________________________________

## 参与贡献

欢迎贡献。开始之前请先阅读 [AGENTS.md](AGENTS.md) 与
[CONTRIBUTING.md](CONTRIBUTING.md)；这个仓库要求带门禁的提交、明确的
审查分级，以及如实的状态汇报。
