# prex

跨平台预编译程序管理器：每个软件一份 YAML，不编译源码。

## 安装

支持 Linux、macOS、Windows 的 amd64 / arm64。脚本自动选择架构并校验 SHA-256，
默认安装到 `~/.prex/bin`；建议先阅读下载的脚本。

**Linux / macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/kongdd/prex/main/scripts/install.sh -o install-prex.sh
bash install-prex.sh
export PATH="$HOME/.prex/bin:$PATH"
```

将 PATH 配置加入 `~/.bashrc` 或 `~/.zshrc`。

**Windows（PowerShell 5.1+）**

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/kongdd/prex/main/scripts/install.ps1 -UseBasicParsing -OutFile install-prex.ps1
powershell -ExecutionPolicy Bypass -File .\install-prex.ps1 -AddToPath
```

安装后重新打开终端。也可从 [Releases](https://github.com/kongdd/prex/releases) 下载：
直接提供可执行文件，无需解压。

## 使用

```bash
prex init
prex install lazygit yazi gh
prex list
prex update                 # 只刷新 YAML 版本
prex upgrade                # 按 YAML 升级已安装程序
prex update node            # 只刷新指定软件
prex upgrade node
prex pin gh                 # 锁定版本；unpin 解锁
prex rollback gh            # 切回本地保留版本，并同步 YAML
prex doctor
prex remove gh
```

内置清单：lazygit、yazi、gh、herdr、bun、node、codebase-memory-mcp、fs。
各软件的平台支持以清单为准。

## 配置

- 数据目录：`~/.prex`，用 `PREX_ROOT` 或 `--root` 修改。
- 自定义清单：设置 `PREX_MANIFESTS` 或 `--manifests`；不会注入内置清单。
- 新增软件：在清单目录添加同名 YAML，无需改代码。
- 旧 binpick 数据：设置 `PREX_ROOT="$HOME/.binpick"`，不要直接移动旧目录。

清单格式、平台要求、回滚与开发说明见 [详细文档](docs/guide.md)。
