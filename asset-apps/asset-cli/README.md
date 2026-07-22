# Asset CLI

`asset` 是 Asset Hub 的本地管理命令行入口。面向管理员、运维人员和插件开发者。

## 运行方式

在项目根目录运行：

```bash
cargo run -p asset-apps --bin asset -- --help
```

安装或发布可执行文件后，可以直接使用：

```bash
asset --help
```

## 命令结构

当前 CLI 已建立以下命令组：

```text
asset
├── config
│   ├── --check [PATH]
│   └── --show [PATH]
├── system
├── user
│   ├── --list
│   ├── --create <USERNAME>
│   ├── --password <USERNAME>
│   ├── --enable <USERNAME>
│   ├── --disable <USERNAME>
│   └── --show <USERNAME>
└── plugin
```

| 命令组 | 用途 |
| --- | --- |
| `asset config` | 检查和管理 Asset Hub 配置 |
| `asset system` | 检查和维护本地 Asset Hub 系统 |
| `asset user` | 管理 Asset Hub 用户 |
| `asset plugin` | 构建和管理 Asset Hub 插件 |

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
6. 是否写入安全审计事件。
7. 至少一个可直接运行的示例。

建议使用以下格式：

````markdown
### `asset <group> <command>`

用途：说明命令解决的问题。

用法：

```bash
asset <group> <command> [OPTIONS]
```

参数：说明参数、默认值和约束。

副作用：说明会修改哪些数据，以及是否记录安全审计事件。
````

# `asset config` 命令

## `asset config --check [PATH]`

读取并校验 Asset Hub 配置，同时完成路径归一化，但不会初始化数据库、对象存储或应用
运行时。配置有效时输出 `configuration is valid`，配置无效时输出错误并以非零状态退出。

```bash
asset config --check
asset config --check config.toml
```

省略 `PATH` 时尝试读取当前目录的 `config.toml`；文件不存在时校验内置默认配置。显式
提供 `PATH` 时，该文件必须存在。

## `asset config --show [PATH]`

读取并校验配置，然后以 TOML 输出填充默认值且完成路径归一化后的完整配置。该命令
同样不会初始化数据库、对象存储或应用运行时。

```bash
asset config --show
asset config --show config.toml
```

`--check` 与 `--show` 互斥，并且执行 `asset config` 时必须选择其中一个。

数据库和 Blob 存储分别通过 `database.backend` 与 `blob.backend` 选择后端；当前支持并
默认使用 `sqlite` 与 `local`。SQLite 文件路径不属于可配置项，使用本地 Blob 后端时
始终由 `blob.local.root` 派生为 `<blob.local.root>/.asset-hub/asset-hub.sqlite`。

# `asset system` 命令

# `asset user` 命令

`asset user` 读取当前目录的 `config.toml`（不存在时使用内置默认配置）并初始化本地运行时。
每次必须且只能选择一个操作。

## `asset user --list`

按用户名列出全部用户，以表格展示用户名、角色、状态、工作目录和用户 ID。该命令不会输出
密码哈希，也不会写入安全审计事件。

```bash
asset user --list
```

## `asset user --create <USERNAME>`

创建启用状态的普通成员。`UserService` 会将未指定的工作目录设置为
`users/<username>`。初始密码通过终端隐藏输入并要求二次确认，长度不得少于 4 个字符。

```bash
asset user --create alice
```

该命令会创建用户数据库记录及其工作目录，并写入 `auth.user.create` 安全审计事件。审计事件
只记录目标用户名，不记录密码。

## `asset user --password <USERNAME>`

重置指定用户的密码；原密码存在时直接覆盖。新密码通过终端隐藏输入并要求二次确认，长度
不得少于 4 个字符。

```bash
asset user --password alice
```

该命令会更新用户数据库记录并写入 `auth.user.password` 安全审计事件。密码不会作为命令参数
传递，因此不会进入 shell 历史、进程参数列表或审计记录。密码更新会改变会话认证哈希，使
已有会话在后续校验时失效。

## `asset user --enable <USERNAME>`

启用指定用户，使其可以重新登录。用户不存在时命令以非零状态退出。

```bash
asset user --enable alice
```

该命令会更新用户状态并写入 `auth.user.status` 安全审计事件。

## `asset user --disable <USERNAME>`

禁用指定用户。禁用后该用户不能登录，已有会话会在后续用户状态校验时失效。用户不存在时
命令以非零状态退出。

```bash
asset user --disable alice
```

该命令会更新用户状态并写入 `auth.user.status` 安全审计事件。

## `asset user --show <USERNAME>`

展示指定用户的用户名、ID、角色、状态、工作目录及创建和更新时间。该命令不会输出任何密码
信息，也不会写入安全审计事件。用户不存在时命令以非零状态退出。

```bash
asset user --show alice
```

# `asset plugin` 命令
