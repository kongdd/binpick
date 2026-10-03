# binpick

用 Rust 实现的跨平台预编译可执行文件管理器。不编译源码、不安装系统依赖。
每个软件一份 YAML；支持 GitHub Release 和直接 URL 来源。
GitHub 来源的 `update` 查询最新稳定 Release；URL 来源使用 YAML 中手动指定的版本。

## 快速开始

```bash
git clone https://github.com/kongdd/binpick.git
cd binpick
cargo install --path . --locked
binpick init
# 将 init 输出的 bin 目录加入 PATH
binpick install lazygit yazi gh herdr
binpick list
binpick update
binpick update lazygit
binpick remove lazygit
```

`install` 安装 YAML 中指定的版本，不隐式获取最新版。
`update [names...]` 默认处理全部 YAML：
- 已安装：下载、校验、安装最新版本，然后写回 YAML。
- 未安装：只更新 YAML，不安装软件。
- 下载、校验、解压失败：不推进 YAML 版本，旧程序仍然可用。
- 已经是最新版：不重复下载。
- URL 来源不自动发现最新版：先手动修改 YAML 的 `version`，再执行 `update` 升级已安装软件。

默认数据目录为用户主目录下的 `~/.binpick`，可执行文件入口为 `~/.binpick/bin`。
`binpick init` 会输出实际路径；Windows 使用对应用户主目录。
可用 `--root` / `BINPICK_ROOT` 指定其他位置。

```text
root/
  manifests/          # 每个软件一份可修改的 YAML
  bin/                # Unix 符号链接；Windows 可执行文件副本
  packages/           # 安装的各代可执行文件
  state/              # 实际已安装版本记录
  .lock               # 防止并发修改
```

启动时会补充二进制中内置的清单，不覆盖已有同名 YAML。目前提供
lazygit/yazi/gh/herdr/bun/node/codebase-memory-mcp。
codebase-memory-mcp 的 Linux 平台统一使用上游 fully-static portable 包，兼容 GNU 和 musl；
macOS/Windows 使用对应架构原生包，所有平台均校验 `checksums.txt`。
`binpick install codebase-memory-mcp` 仅安装程序，不自动注册 MCP 客户端或运行上游安装脚本。
更新使用 `binpick update codebase-memory-mcp`，不依赖未被提取的上游安装脚本。
Node 的 GNU 构建要求 glibc >= 2.28；x64 musl 使用独立发行包，ARM64 musl 暂不支持。
升级 binpick 不会覆盖已经初始化的旧版 `node.yaml`；如需修正旧清单，请手动同步仓库版本。
构建脚本自动扫描仓库的 `manifests/*.yaml` 并生成内置目录；新增、删除或修改清单
会触发重新构建，无需在 Rust 代码中逐个注册软件，测试也会自动检查全部内置清单。
Herdr 的 Linux 资源为静态 musl 构建，GNU 系统也可自动选择；上游目前未提供
Windows ARM64 发行资源或独立 checksum 文件，缺少 checksum 时会明确警告。
要直接管理本仓库的 YAML：

```bash
binpick --manifests ./manifests update
binpick --manifests ./manifests install lazygit
```

`--manifests` 也可通过 `BINPICK_MANIFESTS` 设置。
更新会重新序列化 YAML，保留配置字段但不保留注释/原始排版。

## 回滚与维护

```bash
binpick history gh                    # 查看保留版本与当前版本
binpick rollback gh                   # 离线切换到最近保留的不同版本
binpick rollback gh --version v2.101.0 # 指定本地已保留的 tag
binpick pin gh                        # 锁定，update 跳过该软件
binpick unpin gh
binpick gc --keep 2 --dry-run          # 预览；保留当前代 + 最近一个其他安装代
binpick gc --keep 2                    # 执行清理
binpick gc gh --keep 1                 # 只保留 gh 当前安装代
binpick doctor                        # 检查活动文件、命令入口和状态
```

回滚同步写回 YAML 和已安装状态，不修改 pinned 设置。回滚后若不想在下次 update
再次升级，请执行 pin。`--version` 只接受本地保留版本，不会自动联网下载。
GC 永远优先保留当前安装代，即使当前版本更旧；keep 最小为 1。
清理后的版本无法离线回滚。GC 不删除缺少元数据的旧版目录或未知目录，
也不会检测 Windows 中旧程序是否仍在运行；文件被占用时可能清理失败。

每个安装代记录版本、可执行文件、创建时间和 SHA-256，用于回滚前完整性验证。
本地 hash 是安装时的完整性记录，不替代上游签名或校验。
初版状态会自动迁移当前安装代；旧的非活动目录无法可靠推断版本，保留不动。
相同版本的重复 install 会跳过，不重复下载或创建安装代。

## 新增软件

新增软件只需一份 YAML，文件名必须与 name 一致：

- 加入内置清单：在仓库的 `manifests/` 中添加 `mytool.yaml`，重新构建或安装 binpick。
- 本机立即使用：放入 `~/.binpick/manifests/mytool.yaml`，无需改代码或重新编译。
- 自定义目录：使用 `--manifests /path/to/manifests`。

例如：

```yaml
name: mytool
version: v1.2.3
source:
  github: owner/repository
assets:
  linux-amd64-gnu:
    file: mytool_{version}_linux_amd64.tar.gz
    checksum: checksums.txt
  darwin-arm64:
    file: mytool_{version}_darwin_arm64.zip
  windows-amd64:
    file: mytool_{version}_windows_amd64.zip
executables:
  - mytool
```

- `version`：完整上游版本/tag，保留前导 v。
- `source`：`github: owner/repository` 与 `url: https://...` 二选一；URL 是默认下载地址模板。
- `assets.<平台>.url`：可选，默认下载 URL 的平台专用模板覆盖。
  Node 的目录与文件名均保留 v，因此清单使用 `{tag}` 而非 `{version}`。
- URL 下载模板还支持 `{platform}` 和 `{format}`；平台命名不同的上游应配置平台专用 URL。
- `pinned`：可选布尔值，默认 false；true 时 update 跳过。
- `{tag}`：完整 tag；`{version}`：去掉一个前导 v。
- `file`：发行文件的精确名称模板，用于 GitHub asset 查找及校验文件匹配。
- `format`：可选，支持 `zip`、`tar.gz`、`tar.xz`、`raw`；默认按文件名推断。
- `checksum`：可选，Release 内的校验文件名称模板，支持 sha256sum 格式。
  URL 来源同时配置 `checksum_url` 指定校验文件 URL 模板；它支持 `{tag}` / `{version}`。
  缺少 `checksum` 时明确警告，不声称已校验；配置了校验但无法解析地址时安装失败。
- `executables`：压缩包中的可执行文件 basename，支持嵌套目录和多个程序。
  Windows 自动补 `.exe`；同名文件出现多次会报错，不猜选哪一个。
- 未知字段报错，避免拼写错误被静默忽略。

平台键：
`linux-amd64-gnu`、`linux-arm64-gnu`、
`linux-amd64-musl`、`linux-arm64-musl`、
`darwin-amd64`、`darwin-arm64`、
`windows-amd64`、`windows-arm64`。

Linux libc 类型先根据 binpick 自身的编译目标选择；GNU 构建会通过
`getconf GNU_LIBC_VERSION` 获取系统 glibc 版本。GNU 资源不存在，或不满足清单中的
`min_glibc: '2.39'` 时，自动选择同架构 musl 资源；没有可用资源则明确报错。
未设置 `min_glibc` 时不检查最低 glibc 版本，也不从二进制推断要求。
不会模拟其他 CPU；musl 系统请使用 binpick 的 musl 构建。
Yazi 的部分功能仍需要系统提供外部工具，本项目不自动安装这些依赖。

## 网络与安全

- 使用 HTTPS GitHub API / 上游下载地址；遵循系统代理环境变量。
- 可设置 `GITHUB_TOKEN` 避免匿名 API 速率限制；token 仅发送给 API，不发送给下载地址。
- `BINPICK_GITHUB_API` 可替换 API 基址，用于测试或可信代理；不要指向不可信服务器。
- 仅提取声明的普通可执行文件，拒绝路径穿越，不执行安装脚本。
- 避免覆盖其他软件或非本工具管理的同名命令。
- 下载、校验、提取均在发布前完成；单个命令入口原子替换。
  多命令入口、YAML/state 切换前会保存快照；普通错误会尝试恢复旧状态，恢复失败会明确报错。
  **尚无持久化崩溃恢复日志**，进程被强杀、断电或恢复本身失败仍可能部分更新。
- 安装代保留到 gc 或卸载，rollback 不需要重新下载。
- Windows 正在运行的程序可能阻止替换，需要退出程序后重试。

## 代码结构

按业务边界保留 6 个功能文件，避免过度拆分；模块间使用显式导入。

- `main.rs`：入口、命令参数与命令分发
- `package.rs`：本地清单/状态读取、GitHub 请求、安装、更新与卸载
- `manifest.rs`：数据模型、内置清单与初始化；`build.rs` 自动扫描 YAML
- `platform.rs`：OS/CPU/libc 适配与资源模板
- `storage.rs`：安全命名、文件发布、归档提取与校验
- `maintenance.rs`：安装代、回滚、清理、锁定与检查
- `unit_tests.rs`、`tests/`：通用单元测试和端到端测试

## 测试

```bash
cargo test --locked
cargo clippy --all-targets -- -D warnings
```

端到端测试使用本地 HTTP fixture，不依赖 GitHub，覆盖安装、更新、卸载、
未安装时仅更新 YAML、失败时保持旧版本、命令冲突、离线回滚、清理保护、
版本锁定、旧状态迁移、篡改检测及切换失败后的快照恢复。
Node 回归测试使用严格的本地发行路径，覆盖版本模板、校验 URL、嵌套归档提取、
校验失败不发布及 URL 升级失败保留旧安装；平台测试拒绝不支持的 ARM64 musl。
内置清单来自实现时核对过的上游发行文件；后续上游改名需要调整 YAML。
