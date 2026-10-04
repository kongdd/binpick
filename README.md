# prex

用 Rust 实现的跨平台预编译可执行文件管理器。不编译源码、不安装系统依赖。
每个软件一份 YAML；支持 GitHub Release 和直接 URL 来源。
`update` 只刷新 YAML；`upgrade` 才按 YAML 中记录的版本升级已安装软件。
GitHub 来源自动查询最新稳定 Release；URL 来源通过可选的 `checkver` 查询版本。

## 安装预编译版本

从 [Releases](https://github.com/kongdd/prex/releases) 下载，不需要 Rust。
首个公开 prex 版本为 `v0.1.1`。

### Linux / macOS（Bash）

先下载并阅读脚本，再执行；脚本选择当前架构并校验发行包 SHA-256，默认安装到
`~/.prex/bin`（设置 `PREX_ROOT` 时安装到该目录的 `bin/`）：

```bash
curl -fsSL https://raw.githubusercontent.com/kongdd/prex/main/scripts/install.sh -o install-prex.sh
# 可先阅读 install-prex.sh
bash install-prex.sh
export PATH="$HOME/.prex/bin:$PATH"  # 建议也写入 ~/.bashrc 或 ~/.zshrc
prex --version
prex init
```

指定版本、位置或 GNU 构建：

```bash
bash install-prex.sh --version v0.1.1
bash install-prex.sh --version v0.1.1 --dir "$HOME/.local/bin"
bash install-prex.sh --platform linux-amd64-gnu
```

Linux 默认使用完全静态的 musl 构建；在 GNU 系统上运行时，prex 仍根据宿主 libc
选择软件包，不会将 glibc 软件误当成 musl 软件。macOS 自动选择 Intel / Apple Silicon。
脚本只安装 prex，不运行软件清单或其他安装脚本，也不自动改写 shell 配置文件。

### Windows（PowerShell）

脚本支持 PowerShell 5.1+，自动选择 amd64 / arm64，校验 SHA-256 后安装到
`$HOME\.prex\bin`。先阅读下载的脚本；以下执行策略只影响此次子进程，不更改全局策略：

```powershell
Invoke-WebRequest -Uri https://raw.githubusercontent.com/kongdd/prex/main/scripts/install.ps1 -OutFile install-prex.ps1
# 可先用 Get-Content .\install-prex.ps1 阅读脚本
powershell -ExecutionPolicy Bypass -File .\install-prex.ps1 -AddToPath
# 重新打开终端后：
prex --version
prex init
```

`-AddToPath` 将安装目录加入用户 PATH，不修改系统 PATH；省略则自行配置 PATH。
指定版本或位置：

```powershell
powershell -ExecutionPolicy Bypass -File .\install-prex.ps1 -Version v0.1.1 -InstallDir "$HOME\tools\prex" -AddToPath
```

Windows 中正在运行的 prex 可能阻止替换，先退出相关进程再重试。

### 手动下载

Release 直接提供可执行文件，不使用压缩包。

| 平台 | 文件名后缀 | 说明 |
|---|---|---|
| Linux amd64 GNU | `linux-amd64-gnu` | glibc >= 2.35 |
| Linux arm64 GNU | `linux-arm64-gnu` | glibc >= 2.39 |
| Linux amd64 / arm64 musl | `linux-<架构>-musl` | 完全静态，GNU/Alpine 均可用，推荐 |
| macOS Intel / Apple Silicon | `darwin-amd64` / `darwin-arm64` | 构建最低目标 macOS 11，测试于 macOS 15 |
| Windows amd64 / arm64 | `windows-<架构>.exe` | 静态链接 MSVC C 运行库 |

例如 `prex-0.1.1-linux-amd64-musl`。从同一 Release 下载 `SHA256SUMS.txt` 校验；
Linux/macOS 使用 `chmod +x <文件>`，重命名为 `prex` 并放入 PATH；Windows 重命名为
`prex.exe`。Release 另提供 `LICENSE`、`install.sh`、`install.ps1`，无需解压。
当前预编译版本已包含初始 YAML 快照，支持通过 `PREX_MANIFESTS` 使用独立清单目录。

## 从源码安装及快速开始

```bash
git clone https://github.com/kongdd/prex.git
cd prex
cargo install --path . --locked
prex init
# 将 init 输出的 bin 目录加入 PATH
prex install lazygit yazi gh herdr
prex list
prex update                 # 刷新全部 YAML，不升级程序
prex update lazygit         # 只刷新 lazygit 的 YAML
prex update --dry-run       # 预览版本变化
prex upgrade                # 按 YAML 升级所有已安装的软件
prex upgrade lazygit        # 只升级已安装的 lazygit
prex remove lazygit
```

`install` 安装 YAML 中指定的版本，不隐式获取最新版。
`update [names...]` 默认处理全部 YAML，无论软件是否已安装：
- 只查询上游版本并原子写回 YAML，不下载程序、不修改已安装状态。
- `--dry-run` 只预览版本变化，不写入版本变更。
- GitHub 来源查询最新稳定 Release；URL 来源按 `checkver` 查询。
- URL 来源没有 `checkver` 时明确提示并跳过；也可手动编辑 `version`。
- 查询失败、版本无效或已是最新版时保留原 YAML。

`upgrade [names...]` 默认只处理已安装软件：
- 使用 YAML 中的版本，不重新查询最新版、不改写 YAML。
- 指定尚未安装的软件时提示并跳过；首次安装请使用 `install`。
- 下载、校验、解压或切换失败时尝试恢复旧程序和状态。
- 已安装版本与 YAML 相同则跳过；`pinned` 软件在 `update` / `upgrade` 时均跳过。

通常执行 `prex update && prex upgrade`。两步分离后，升级失败不会撤销已经更新的
清单版本；修复问题后重新执行 `upgrade` 即可。

默认数据目录为用户主目录下的 `~/.prex`，可执行文件入口为 `~/.prex/bin`。
`prex init` 会输出实际路径；Windows 使用对应用户主目录。
可用 `--root` / `PREX_ROOT` 指定其他位置。

```text
root/
  manifests/          # 每个软件一份可修改的 YAML
  bin/                # Unix 符号链接；Windows 可执行文件副本
  packages/           # 安装的各代可执行文件
  state/              # 实际已安装版本记录
  .lock               # 防止并发修改
```

默认清单目录在启动时补充二进制内置清单，不覆盖已有同名 YAML。目前提供
lazygit/yazi/gh/herdr/bun/node/codebase-memory-mcp/fs。
显式指定 `--manifests` / `PREX_MANIFESTS` 时，只使用该目录，不注入内置清单。
codebase-memory-mcp 的 Linux 平台统一使用上游 fully-static portable 包，兼容 GNU 和 musl；
macOS/Windows 使用对应架构原生包，所有平台均校验 `checksums.txt`。
`prex install codebase-memory-mcp` 仅安装程序，不自动注册 MCP 客户端或运行上游安装脚本。
更新使用 `prex update codebase-memory-mcp && prex upgrade codebase-memory-mcp`，
不依赖未被提取的上游安装脚本。
Node 的 GNU 构建要求 glibc >= 2.28；x64 musl 使用独立发行包，ARM64 musl 暂不支持。
fs 使用上游 Linux 静态包，支持 GNU/musl 的 amd64/arm64；Windows 支持 amd64/arm64，
macOS 仅提供 Apple Silicon 包且上游要求 macOS 26+，不提供 Intel Mac 包。
所有 fs 发行包均校验 `SHA256SUMS.txt`；安装只提取 `fs`，不执行附带脚本。
升级 prex 不会覆盖已经初始化的旧版 `node.yaml`；如需修正旧清单，请手动同步仓库版本。
构建脚本自动扫描仓库的 `manifests/*.yaml` 并生成内置目录；新增、删除或修改清单
会触发重新构建，无需在 Rust 代码中逐个注册软件，测试也会自动检查全部内置清单。
Herdr 的 Linux 资源为静态 musl 构建，GNU 系统也可自动选择；上游目前未提供
Windows ARM64 发行资源或独立 checksum 文件，缺少 checksum 时会明确警告。
要直接管理本仓库的 YAML：

```bash
prex --manifests ./manifests update --dry-run
prex --manifests ./manifests update
prex --manifests ./manifests upgrade lazygit
prex --manifests ./manifests install lazygit
```

`--manifests` 也可通过 `PREX_MANIFESTS` 设置。
更新会重新序列化 YAML，保留配置字段但不保留注释/原始排版。

## 独立清单仓库

`manifests/` 目前仍在本仓库中，后续可以直接迁入独立仓库；运行时只依赖目录中的
`*.yaml`，不要求与 prex 源码同处一个仓库，也不需要重新编译 prex。
独立清单仓库应将 YAML 放在选定目录的顶层（仓库顶层或其 `manifests/` 子目录均可）：

```bash
# 独立仓库创建后，将占位 URL 替换为实际地址
git clone <清单仓库URL> "$HOME/prex-manifests"
export PREX_MANIFESTS="$HOME/prex-manifests"
prex list
prex update --dry-run
prex update && prex upgrade
```

prex 不自动执行 `git pull`、提交或推送。`update` 修改版本字段；`pin` / `rollback`
也可能修改清单。同步仓库前需要自行处理本地变更。默认内置清单暂时作为离线初始快照，
不会写入显式指定的外部目录；仓库迁移及自动同步将在确定仓库地址后另行处理。

### 从旧名称迁移

命令与 Cargo 包名改为 `prex`，默认目录为 `~/.prex`，环境变量为 `PREX_*`。
现有 `~/.binpick` 不会自动移动或删除。可以继续使用原安装状态：

```bash
export PREX_ROOT="$HOME/.binpick"
prex list
prex doctor
```

prex 兼容原 `.binpick-generation.json` 元数据；新安装代使用 `.prex-generation.json`。
旧 Unix 符号链接可能包含绝对路径，不要直接重命名旧数据目录。
GitHub 仓库为 `kongdd/prex`；旧命令不会自动卸载。

## 回滚与维护

```bash
prex history gh                    # 查看保留版本与当前版本
prex rollback gh                   # 离线切换到最近保留的不同版本
prex rollback gh --version v2.101.0 # 指定本地已保留的 tag
prex pin gh                        # 锁定，update / upgrade 跳过该软件
prex unpin gh
prex gc --keep 2 --dry-run          # 预览；保留当前代 + 最近一个其他安装代
prex gc --keep 2                    # 执行清理
prex gc gh --keep 1                 # 只保留 gh 当前安装代
prex doctor                        # 检查活动文件、命令入口和状态
```

回滚同步写回 YAML 和已安装状态，不修改 pinned 设置。回滚后若不想在下次 update / upgrade
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

- 加入内置清单：在仓库的 `manifests/` 中添加 `mytool.yaml`，重新构建或安装 prex。
- 本机立即使用：放入 `~/.prex/manifests/mytool.yaml`，无需改代码或重新编译。
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
- `pinned`：可选布尔值，默认 false；true 时 update / upgrade 跳过。
- `{tag}`：完整 tag；`{version}`：去掉一个前导 v。
- `file`：发行文件的精确名称模板，用于 GitHub asset 查找及校验文件匹配。
- `format`：可选，支持 `zip`、`tar.gz`、`tar.xz`、`raw`；默认按文件名推断。
- `checksum`：可选，Release 内的校验文件名称模板，支持 sha256sum 格式。
  URL 来源同时配置 `checksum_url` 指定校验文件 URL 模板；它支持 `{tag}` / `{version}`。
  缺少 `checksum` 时明确警告，不声称已校验；配置了校验但无法解析地址时安装失败。
- `executables`：压缩包中的可执行文件 basename，支持嵌套目录和多个程序。
  Windows 自动补 `.exe`；同名文件出现多次会报错，不猜选哪一个。
- 未知字段报错，避免拼写错误被静默忽略。

### URL 来源的版本查询（checkver）

借鉴 Scoop 的 `checkver` 思路，将版本查询与下载模板分开。例如 Node：

```yaml
source:
  url: https://nodejs.org/dist/{tag}/node-{tag}-linux-x64.tar.xz
checkver:
  url: https://nodejs.org/dist/index.json
  json_pointer: /0/version
```

`checkver.url` 必须是 HTTP(S) 地址，不发送 `GITHUB_TOKEN`。
`json_pointer` 是 RFC 6901 JSON Pointer（不是 Scoop 的 JSONPath），选择值必须为字符串；
省略该字段时将响应作为纯文本版本并去掉首尾空白。返回版本保留前导 v，
不得包含路径分隔符等不安全字符。GitHub 来源无需也不接受 `checkver`。
下载及校验地址仍使用原有 `{tag}` / `{version}` 模板，在安装时按新版本渲染；
`update` 不自动修复上游改名或平台增减，也不验证发行包是否已经发布完整。

平台键：
`linux-amd64-gnu`、`linux-arm64-gnu`、
`linux-amd64-musl`、`linux-arm64-musl`、
`darwin-amd64`、`darwin-arm64`、
`windows-amd64`、`windows-arm64`。

Linux 通过 `getconf GNU_LIBC_VERSION` 探测宿主 glibc；即使 prex 使用静态 musl 构建，
在能检测到 glibc 的系统上也选择 GNU 软件包。探测不到时按 prex 的编译目标回退。
GNU 资源不存在，或不满足清单中的
`min_glibc: '2.39'` 时，自动选择同架构 musl 资源；没有可用资源则明确报错。
未设置 `min_glibc` 时不检查最低 glibc 版本，也不从二进制推断要求。
不会模拟其他 CPU；musl 系统请使用 prex 的 musl 构建。
Yazi 的部分功能仍需要系统提供外部工具，本项目不自动安装这些依赖。

## 网络与安全

- 使用 HTTPS GitHub API / 上游下载地址；遵循系统代理环境变量。
- 可设置 `GITHUB_TOKEN` 避免匿名 API 速率限制；token 仅发送给 API，不发送给下载地址。
- `PREX_GITHUB_API` 可替换 API 基址，用于测试或可信代理；不要指向不可信服务器。
- 仅提取声明的普通可执行文件，拒绝路径穿越，不执行安装脚本。
- 避免覆盖其他软件或非本工具管理的同名命令。
- 下载、校验、提取均在发布前完成；单个命令入口原子替换。
  多命令入口/state 切换前会保存快照；回滚还保护 YAML。普通错误会尝试恢复旧状态，恢复失败会明确报错。
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
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
bash -n scripts/install.sh
```

端到端测试使用本地 HTTP fixture，不依赖 GitHub，覆盖安装、更新、卸载、
仅更新 YAML 不改变已安装程序、升级失败保持旧程序、命令冲突、离线回滚、清理保护、
版本锁定、旧状态迁移、篡改检测及切换失败后的快照恢复。
Node 回归测试使用严格的本地发行路径，覆盖版本模板、校验 URL、嵌套归档提取、
校验失败不发布及 URL 升级失败保留旧安装；平台测试拒绝不支持的 ARM64 musl。
版本查询测试覆盖 JSON Pointer / 纯文本、无效版本保护、dry-run、批处理失败隔离及
update 不读取/迁移安装状态；upgrade 不改写清单、不重新发现版本。
内置清单来自实现时核对过的上游发行文件；后续上游改名需要调整 YAML。

## 发布

`.github/workflows/release.yml` 在推送 `v*` 标签时构建八个原生目标，并对每个目标执行
测试、Clippy、release 构建及版本烟雾检查；musl 二进制额外检查不存在动态加载器。
标签必须与 `Cargo.toml` 版本一致。全部目标成功后才汇总原始可执行文件、生成 `SHA256SUMS.txt`
并公开 Release，不会把缺少平台的部分构建当作完整发行版。

```bash
# 先提交并推送代码，确认 CI 成功，再创建对应版本标签
git tag v0.1.1
git push origin v0.1.1
```

工作流也支持手动输入已经存在的版本标签，重试同一版本的构建和上传。
发布脚本、安装脚本位于 `scripts/`；清单仍可独立于发布二进制更新。
