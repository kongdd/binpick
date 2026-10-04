# prex manifests

软件清单目录，后续可独立为一个仓库。YAML 是 prex 的运行时输入，不依赖 Rust 代码注册。

## 使用

将这个目录（或未来独立仓库的 YAML 顶层目录）设置为清单目录：

```bash
export PREX_MANIFESTS=/path/to/prex-manifests
prex list
prex update --dry-run
prex update
prex upgrade
```

也可逐次传入 `--manifests /path/to/prex-manifests`。显式指定的目录是唯一清单来源，
prex 不会往其中补充内置清单，也不自动执行 Git 操作。

- `update` 查询上游，只修改 YAML 中的版本。
- `upgrade` 按 YAML 升级已安装程序；首次安装用 `install`。
- 清单中的 URL 和文件名可使用 `{tag}`（完整版本）与 `{version}`（去掉前导 v）。
- `update`、`pin`、`rollback` 可能产生本地 YAML 变更；Git 同步前自行提交、备份或处理冲突。

## 清单约定

每个软件一份顶层 `*.yaml`，文件名与 `name` 一致：

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
    checksum: checksums.txt
  windows-amd64:
    file: mytool_{version}_windows_amd64.zip
    checksum: checksums.txt
executables:
  - mytool
```

`source.github` 与 `source.url` 二选一。URL 来源通过可选的 `checkver.url` 查询版本；
`checkver.json_pointer` 使用 RFC 6901 JSON Pointer，省略时响应视为纯文本版本。
URL 来源的校验文件用 `checksum_url` 指定。`format` 支持 `tar.gz`、`tar.xz`、`zip`、`raw`。
`executables` 是可执行文件 basename，Windows 自动补 `.exe`；不执行上游安装脚本。
缺少 checksum 时会警告，不会声称已校验。未知字段会报错。

平台键：`linux-amd64-gnu`、`linux-arm64-gnu`、`linux-amd64-musl`、`linux-arm64-musl`、
`darwin-amd64`、`darwin-arm64`、`windows-amd64`、`windows-arm64`。
必须按实际发行包的 ABI 配置；不能把 glibc 包当作 musl 包。
静态 portable 包可同时用于 GNU 和 musl；GNU 包可声明 `min_glibc`。

新增或更新清单前，核对指定版本的发行文件名、可执行文件名、校验文件以及平台支持。
保留版本模板，不假设未来版本一定沿用文件名或提供全部平台。
