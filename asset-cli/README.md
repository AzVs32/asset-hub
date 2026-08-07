# Asset CLI

`asset` 是 Asset Hub 的本地管理命令行入口。面向管理员、运维人员和插件开发者。

## 运行方式

在项目根目录运行：

```bash
cargo run -p asset-cli --bin asset -- --help
```

安装或发布可执行文件后，可以直接使用：

```bash
asset --help
```

## 命令结构

当前 CLI 已建立以下命令组：

```text
asset [--config <PATH>]
├── config
│   ├── --check
│   └── --show
├── system
├── user
│   ├── --list
│   ├── --create <USERNAME> [--admin]
│   ├── --password <USERNAME>
│   ├── --enable <USERNAME>
│   ├── --disable <USERNAME>
│   └── --show <USERNAME>
└── plugin
    ├── --generate-lock <MANIFEST>
    └── --verify <MANIFEST>
```

| 命令组 | 用途 |
| --- | --- |
| `asset config` | 检查和管理 Asset Hub 配置 |
| `asset system` | 检查和维护本地 Asset Hub 系统 |
| `asset user` | 管理 Asset Hub 用户 |
| `asset plugin` | 封装和校验 Asset Hub 插件产物 |

需要读取 Asset Hub 配置的命令统一通过顶层 `--config <PATH>` 指定文件。未指定时尝试读取
当前目录的 `config.toml`；文件不存在时使用内置默认配置。

## 查看帮助

查看顶层帮助：

```bash
asset --help
```

查看指定命令组的帮助：

```bash
asset config --help
asset system --help
asset user --help
asset plugin --help
```

## 命令文档约定

后续增加具体命令时，应在本文档对应命令组下记录：

1. 完整命令格式。
2. 命令用途和适用场景。
3. 参数、选项及默认值。
4. 是否读取配置或初始化运行时。
5. 是否修改数据库、对象存储或文件系统。
6. 至少一个可直接运行的示例。

建议使用以下格式：

````markdown
### `asset <group> <command>`

用途：说明命令解决的问题。

用法：

```bash
asset <group> <command> [OPTIONS]
```

参数：说明参数、默认值和约束。

副作用：说明会修改哪些数据。
````

# `asset config` 命令

## `asset [--config <PATH>] config --check`

读取并校验 Asset Hub 配置，同时完成路径归一化，但不会初始化数据库、对象存储或应用
运行时。配置有效时输出 `configuration is valid`，配置无效时输出错误并以非零状态退出。

```bash
asset config --check
asset --config config.toml config --check
```

省略 `--config` 时尝试读取当前目录的 `config.toml`；文件不存在时校验内置默认配置。
显式提供路径时，该文件必须存在。

## `asset [--config <PATH>] config --show`

读取并校验配置，然后以 TOML 输出填充默认值且完成路径归一化后的完整配置。该命令
同样不会初始化数据库、对象存储或应用运行时。

```bash
asset config --show
asset --config config.toml config --show
```

`--check` 与 `--show` 互斥，并且执行 `asset config` 时必须选择其中一个。

数据库和 Blob 存储分别通过 `database.backend` 与 `blob.backend` 选择后端；当前支持并
默认使用 `sqlite` 与 `local`。SQLite 文件路径不属于可配置项，使用本地 Blob 后端时
始终由 `blob.local.root` 派生为 `<blob.local.root>/.asset-hub/asset-hub.sqlite`。插件同样从
`<blob.local.root>/.asset-hub/plugins/<plugin-id>` 自动发现，不存在 `[kind]` 插件路径配置。

# `asset system` 命令

## `asset system --scan-resource`

完整遍历 Blob 存储、重新计算每个文件的 SHA-256，并将最终状态协调到资源数据库。该操作
用于显式完整校验，不属于 HTTP 服务的日常启动流程。

```bash
asset system --scan-resource
asset --config config.toml system --scan-resource
```

HTTP 服务启动和周期同步只比较物理文件修改时间与 `ResourceContent.size`；只有新增或发生
变化的文件才重新计算 SHA-256。

# `asset user` 命令

`asset user` 使用顶层 `--config` 指定的配置文件；未指定时读取当前目录的 `config.toml`
（不存在时使用内置默认配置），然后初始化本地运行时。每次必须且只能选择一个操作。

## `asset user --list`

按用户名列出全部用户，以表格展示用户名、角色、状态、工作目录和用户 ID。该命令不会输出
密码哈希。

```bash
asset user --list
```

## `asset user --create <USERNAME> [--admin]`

默认创建启用状态的普通成员；增加 `--admin` 时创建管理员。普通成员的默认工作目录为
`users/<username>`，管理员的工作目录为根目录 `/`。初始密码通过终端隐藏输入并要求二次
确认，长度不得少于 4 个字符。

```bash
asset user --create alice
asset user --create admin --admin
asset --config config.toml user --create admin --admin
```

`--admin` 只能与 `--create` 一起使用。首次部署应先用它创建至少一个管理员，再登录 Web。
该命令会创建用户数据库记录及其工作目录。

## `asset user --password <USERNAME>`

重置指定用户的密码；原密码存在时直接覆盖。新密码通过终端隐藏输入并要求二次确认，长度
不得少于 4 个字符。

```bash
asset user --password alice
```

该命令会更新用户数据库记录。密码不会作为命令参数传递，因此不会进入 shell 历史或进程
参数列表。密码更新会改变会话认证哈希，使已有会话在后续校验时失效。

## `asset user --enable <USERNAME>`

启用指定用户，使其可以重新登录。用户不存在时命令以非零状态退出。

```bash
asset user --enable alice
```

该命令会更新用户状态。

## `asset user --disable <USERNAME>`

禁用指定用户。禁用后该用户不能登录，已有会话会在后续用户状态校验时失效。用户不存在时
命令以非零状态退出。

```bash
asset user --disable alice
```

该命令会更新用户状态。

## `asset user --show <USERNAME>`

展示指定用户的用户名、ID、角色、状态、工作目录及创建和更新时间。该命令不会输出任何密码
信息。用户不存在时命令以非零状态退出。

```bash
asset user --show alice
```

# `asset plugin` 命令

`asset plugin` 只处理显式给出的插件文件，不读取 Asset Hub 运行配置；同时提供顶层
`--config` 会报错，以避免参数看似生效但实际被忽略。

插件包必须在启动 Asset Hub 前显式生成 `manifest.lock.json`。生成 lock 与验证/加载是两个独立
操作：生成操作只接受尚未存在 lock 的包；CLI 验证和 Runtime 加载均为只读，不会创建或覆盖
lock。更新包内产物时，应删除旧 lock、重新生成并再次验证。

CLI 和 Runtime 共同调用 `asset-infra` 中唯一的包验证实现，因此目录遍历、符号链接规则、
SHA-256、文件集合和大小限制完全一致：Manifest 最大 1 MiB、lock 最大 4 MiB、Wasm 最大
64 MiB，Web 资源合计最大 64 MiB。

## `asset plugin --generate-lock <MANIFEST>`

校验插件契约、规范包目录和全部产物规则后，为尚未封装的插件包原子创建
`manifest.lock.json`。如果 lock 已存在则失败，避免无意中接受被修改的产物。

```bash
asset plugin --generate-lock .asset-hub/plugins/example.tools/manifest.json
```

## `asset plugin --verify <MANIFEST>`

读取插件 Manifest 和同目录下已有的 `manifest.lock.json`，校验插件契约及全部包内产物的
完整性，但不修改任何文件。该操作与 Runtime 启动使用同一验证/加载路径，适用于 CI、镜像
构建和发布验收。

```bash
asset plugin --verify .asset-hub/plugins/example.tools/manifest.json
```
