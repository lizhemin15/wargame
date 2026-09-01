# Wargame — 兵棋推演系统（Rust 内核 + Lua 规则即插件）

轻量、跨平台、稳定的兵棋推演引擎。Rust 内核 + Lua 规则插件 + 事件溯源确定性回放。

## 快速开始

### 一键安装（下载 release 二进制）

从 [Releases](../../releases/latest) 下载对应平台的最新包：

| 平台 | 包名 |
|---|---|
| Linux x64 | `wargame-linux-x86_64` |
| macOS Intel | `wargame-macos-x86_64` |
| macOS Apple Silicon | `wargame-macos-aarch64` |
| Windows x64 | `wargame-windows-x86_64.exe` |

```bash
# Linux / macOS
chmod +x wargame-linux-x86_64
./wargame-linux-x86_64

# Windows（PowerShell）
.\wargame-windows-x86_64.exe
```

### 从源码构建

```bash
cargo run --release
```

## M1 能做什么

- **规则即插件**：走法规则在 `plugins/*.lua`，改规则无需改内核
- **热插拔**：运行中改 Lua 文件 → `hot_reload` → 立即生效，内核不重启
- **确定性事件溯源**：命令 → 事件日志 → 状态折叠，可回放、可哈希校验

```
M1 验证：马走日通过 / 横一格拒绝 / 原地拒绝 / 热插拔实时生效 ✅
```

## 架构

见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 开发

```bash
cargo run               # 运行 M1 演示
cargo test              # 运行测试
cargo build --release   # 发布构建
```

## License

MIT