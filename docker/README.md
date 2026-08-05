# Asset Hub Docker 部署

本目录提供两类镜像，均由同一个 `Dockerfile` 构建：

- `api`：Rust HTTP 服务、SQLite migration、内置及示例插件。
- `web`：React 管理端和 Nginx；将 `/api`、`/plugins` 转发到 API。

Compose 会同时启动两者，对外只暴露 Web 端口。浏览器和 API 保持同源，Session Cookie
不需要额外的跨域配置。

## 前置要求

- Docker Engine 24 或更高版本。
- Docker Compose v2（使用 `docker compose` 命令）。
- 首次构建需要访问 crates.io、npm registry 和 Debian/Alpine 镜像仓库。
- 建议至少预留 4 GB 内存和 10 GB 构建磁盘空间；Rust release 构建需要一些时间。

运行容器不需要本机安装 Rust、Node.js、SQLite 或 Nginx。

## 快速启动

在项目根目录执行：

```bash
cp docker/.env.example docker/.env
```

然后启动：

```bash
cd docker
docker compose up -d --build
```

首次登录前，在 API 容器中使用本地管理 CLI 创建管理员：

```bash
docker compose exec api asset --config /conf/config.toml user --create admin --admin
```

密码由终端隐藏读取并要求二次确认，不会出现在命令行、Compose 环境变量或 shell 历史中。
HTTP 服务允许在零用户状态下启动，但创建管理员之前无法登录。

查看状态和日志：

```bash
docker compose ps
docker compose logs -f api
```

打开：

- 管理端：`http://127.0.0.1:8080`
- 经 Nginx 转发的 API：`http://127.0.0.1:8080/api`
- Swagger：默认关闭；启用后访问 `http://127.0.0.1:8080/swagger-ui/`

停止服务但保留数据：

```bash
docker compose down
```

## 首次创建管理员

API 镜像同时包含 `asset-http` 和 `asset`。API 镜像默认以
`asset-http --config /conf/config.toml` 启动；管理命令也必须显式传入同一路径，因此两个
可执行程序操作同一个 `/data` 数据域。

```bash
docker compose exec api asset --config /conf/config.toml user --create admin --admin
```

管理员默认拥有根工作区 `/`。不带 `--admin` 时创建普通成员，其默认工作区为
`users/<username>`。创建操作会写入 `auth.user.create` 安全审计事件。

`docker/.env` 仅保存 Compose 与 HTTP 运行参数，不包含用户凭据。该文件已被 `.gitignore`
排除，不应提交环境专属配置。

## 可选环境变量

| 配置 | 默认值 | 说明 |
| --- | --- | --- |
| `ASSET_HUB_PORT` | `8080` | 宿主机对外提供管理端的端口 |
| `ASSET_HUB_IMAGE_TAG` | `local` | Compose 生成镜像的标签 |
| `ASSET_HTTP_ENABLE_SWAGGER` | `false` | 是否开放 Swagger/OpenAPI |
| `ASSET_HTTP_ENABLE_PURGE` | `false` | 是否允许物理删除资源及文件 |
| `ASSET_HTTP_REQUEST_TIMEOUT_SECS` | `30` | 普通 API 请求总超时秒数；流式上传不受该总时长限制 |
| `ASSET_HTTP_COOKIE_SECURE` | `false` | 公网入口为 HTTPS 时必须设为 `true` |
| `ASSET_HTTP_SESSION_INACTIVITY_SECS` | `43200` | Session 非活动过期秒数 |
| `RUST_LOG` | `asset_http=info,tower_http=info` | Rust 日志过滤规则 |

布尔环境变量只接受 `true` 或 `false`。

容器内部已经固定以下运行参数，通常无需修改：

| 配置 | 容器值 | 说明 |
| --- | --- | --- |
| `ASSET_HTTP_ADDR` | `0.0.0.0:8080` | API 容器监听地址 |

API 镜像的默认启动参数为 `--config /conf/config.toml`。该路径不是环境变量；覆盖容器
`command` 时必须保留或显式替换这个参数。

## 数据和配置文件

Compose 创建两个命名卷：

```text
asset-hub-conf  -> /conf  配置文件
asset-hub-data  -> /data  SQLite、系统内部数据和上传文件内容
```

默认容器配置位于 [config.toml](config.toml)，首次创建 `asset-hub-conf` 卷时会复制到
`/conf/config.toml`：

```toml
[database]
backend = "sqlite"

[database.sqlite]
max_connections = 5

[resource_edit]
max_text_bytes = 4194304

[blob]
backend = "local"

[blob.local]
root = "/data"

[blob.local.sync]
enabled = true
debounce_milliseconds = 1000
reconcile_interval_seconds = 1800
```

SQLite 路径不能单独配置，使用本地 Blob 后端时始终为
`<blob.local.root>/.asset-hub/asset-hub.sqlite`。`/data` 是用户文件根目录；`.asset-hub`
是系统保留目录，保存 SQLite 和 action 临时区，
扫描导入会跳过它。不要把 `/data/.asset-hub` 与 `/data` 里的用户文件拆开备份，两者共同
组成一次完整的 Asset Hub 状态。

查看卷位置：

```bash
docker volume inspect asset-hub-conf
docker volume inspect asset-hub-data
```

删除容器和全部数据是破坏性操作：

```bash
docker compose down -v
```

除非确认不再需要数据，否则不要加 `-v`。

## 插件配置

默认镜像启用：

- `azvs-markdown`
- `azvs-epub`

规范插件包位于容器 `/data/.asset-hub/plugins/azvs.markdown` 和
`/data/.asset-hub/plugins/azvs.epub`，服务启动时自动发现，不需要在配置文件中逐项声明。
已有持久化卷不会自动获得镜像中新增的初始文件；升级旧部署时需要把新版规范包同步到该目录。

Markdown 和 EPUB 的浏览器资源会在 Docker 构建期间通过 `npm ci && npm run build` 生成。两个
Wasm 插件也会从源码重新构建并与仓库产物比对。镜像构建会显式生成并验证每个规范包的
`manifest.lock.json`；容器启动只执行只读完整性验证，不会修改插件包。

## 单独构建镜像

不使用 Compose 时，可从项目根目录构建指定 target：

```bash
docker build -f docker/Dockerfile --target api -t asset-hub-api:local .
docker build -f docker/Dockerfile --target web -t asset-hub-web:local .
```

API 单容器运行示例：

```bash
docker volume create asset-hub-conf
docker volume create asset-hub-data
docker run --rm \
  --name asset-hub-api \
  -d \
  -p 8080:8080 \
  -v asset-hub-conf:/conf \
  -v asset-hub-data:/data \
  asset-hub-api:local
docker exec -it asset-hub-api asset --config /conf/config.toml user --create admin --admin
```

此方式只提供 API，不包含管理端。完整部署建议使用 Compose。

## 更新与数据库迁移

应用启动时自动执行尚未应用的 SQLx migration，不需要删除原数据库。更新步骤：

```bash
cd docker
docker compose down
docker compose build --pull
docker compose up -d
docker compose logs -f api
```

生产环境更新前先备份数据。不要修改已经发布并执行过的 migration；数据库结构变化应
始终新增 migration 文件。

## 备份与恢复

一致性要求最高的简单备份方式是短暂停止服务：

```bash
cd docker
docker compose stop api
docker run --rm \
  -v asset-hub-conf:/conf:ro \
  -v asset-hub-data:/data:ro \
  -v "$PWD/backup:/backup" \
  alpine:3.21 \
  tar czf /backup/asset-hub-$(date +%Y%m%d-%H%M%S).tar.gz -C / conf data
docker compose start api
```

恢复到空卷：

```bash
docker compose down
docker volume rm asset-hub-conf
docker volume rm asset-hub-data
docker volume create asset-hub-conf
docker volume create asset-hub-data
docker run --rm \
  -v asset-hub-conf:/conf \
  -v asset-hub-data:/data \
  -v "$PWD/backup:/backup:ro" \
  alpine:3.21 \
  tar xzf /backup/你的备份文件.tar.gz -C /
docker compose up -d
```

## 生产环境建议

- 保持 `ASSET_HTTP_ENABLE_PURGE=false`，除非明确需要不可恢复的物理删除。
- 保持 `ASSET_HTTP_ENABLE_SWAGGER=false`，或仅在受信网络开放。
- 在 Nginx、Caddy、Traefik 或云负载均衡器上配置 HTTPS。
- 不要把 API 容器的 `8080` 端口直接发布到公网；Compose 默认只对 Web 容器暴露端口。
- 使用防火墙限制管理端来源。
- 定期同时备份 `/conf` 和 `/data`。
- `/data/.asset-hub/asset-hub.sqlite` 中包含 `security_audit_events` 安全审计表，备份与恢复时
  必须与 `/data` 中的业务文件和 `/conf` 配置一并处理。
- 只在受信终端中执行 `asset --config /conf/config.toml user --create <用户名> --admin`，并
  限制 Docker socket 和容器执行权限；密码由 CLI 隐藏读取，不应写入环境变量或编排文件。
- 设置日志收集和磁盘/卷容量监控。安全审计事件当前不会自动清理，应按合规要求定期归档
  或删除历史记录，并监控 SQLite 文件增长。
- 使用 `GET /auth/audit-events?page=1&limit=100`（仅管理员）接入安全事件巡检；重点告警连续
  登录失败、登录限流、权限拒绝及管理操作。
- 将 `GET /health` 用作 readiness 探针。该接口同时检查 SQLite 和对象存储，任一依赖异常
  都会返回 HTTP 503；它不等同于只判断进程存活的 liveness 探针。

## 常见问题

### API 不健康或不断重启

```bash
docker compose logs --tail=200 api
```

常见原因包括：数据卷不可写、配置文件不可读、覆盖容器 `command` 时遗漏 `--config`、
SQLite migration 失败、对象存储不可访问或插件 Manifest/WASM 不匹配。`GET /health`
的响应会分别给出 `database` 和 `blob_storage` 状态。

### 登录后接口返回 403

管理员不受目录 ACL 限制。普通用户必须被授予目标目录的 `read`、`write` 或 `manage`
权限；父目录授权会向子目录继承。

### 上传返回 413

镜像中的 Nginx 已设置 `client_max_body_size 4g`，与 API 上限一致。若外层还有反向代理，
也需要同步调整其请求体上限。

### 修改了 `.env` 但容器配置未变化

重建容器以应用环境变量：

```bash
docker compose up -d --force-recreate
```
