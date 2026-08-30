# asset-web 架构

## 1. 定位与原则

`asset-web` 是 Asset Hub 的浏览器宿主。它保留与后端一致的契约边界，但按前端 feature 和
运行时职责组织代码，不机械复制后端 crate：

- 领域对象不感知 HTTP、React 或后端 DTO。
- Feature 只依赖自己需要的窄 Gateway，不直接调用 `fetch`。
- OpenAPI 类型和 snake_case DTO 只存在于 `infra/http`。
- 服务端状态、URL 状态、表单状态和插件注册状态分别拥有明确归属。
- 插件只能通过受版本约束的 action、view、slot 和 Frame Host capability 进入宿主。
- `main.tsx` 是唯一的前端 composition root。

依赖方向：

```text
infra/http ──implements──▶ shared/api ──uses──▶ domain
                                        ▲
                                        │
                                    features
                                      │   │
                                      ▼   ▼
                                   kernel plugins

app/main 只负责创建外层实现并连接 Provider、路由和功能入口。
```

## 2. 模块职责

### `domain`

定义前端内部稳定的业务语言和无副作用规则：

- `Resource`、`Directory`、Kind、Action、用户与授权模型。
- 插件 view、diagnostic 和 action output 类型。
- 资源草稿、目录规范化、父目录和面包屑规则。

Domain 字段统一使用 camelCase。Action 只包含前端发现和展示所需的声明，不包含后端 executor、
handler binding 或传输 DTO。

### `shared`

`shared/api/gateways.ts` 按消费者定义四个接口：

- `AuthGateway`：Session、登录和退出。
- `AssetWorkspaceGateway`：目录浏览、资源命令和上传。
- `PluginHostGateway`：Action 执行、插件资源 URL 和受约束内容替换。
- `UserAdministrationGateway`：管理员用户操作。

`gateway-context.tsx` 为每个接口提供独立 React Hook，避免 Feature 获得全能 API 对象。
`query-keys.ts` 统一服务端状态身份，`errors.ts` 定义 Feature 可理解的错误，
`routing/paths.ts` 保存跨 Feature 的 URL 编解码规则。Feature 不反向依赖 `app`。

### `infra/http`

`createOpenApiGateways` 让所有 HTTP Gateway 共享一个 OpenAPI client。实现按职责拆为认证、资产
工作区、用户管理、DTO 映射和可恢复上传模块，负责：

- 调用 `asset-http` 并将 DTO 映射为 Domain 对象。
- 把 HTTP 错误转换为前端错误，例如认证失效和 revision 冲突。
- 校验不可信插件 view JSON、capability 和 effect 枚举。
- 限制插件媒体与 iframe URL 只能指向允许的后端路径。
- 处理文件哈希、分块上传、校验重试、断点恢复和发布轮询。

替换 HTTP、引入本地 mock 或增加另一种传输时，只替换受影响 Feature 的 Gateway 实现。

### `features`

- `auth`：加载 Session、登录、退出和 Session Context。
- `asset-workspace`：目录浏览、URL 筛选、资源详情、命令、上传和插件 Action。
- `users`：管理员用户列表、创建和状态更新。

资产工作区将读流程放在 `use-asset-workspace-listing`，写流程放在
`use-asset-workspace-commands`；组件主要负责展示、交互和组合。

### `kernel` 与 `plugins`

Kernel 保存宿主级插件选择规则：

- Resource view kind 到 React renderer 的注册表。
- Resource/Directory Action 到 Core slot 的选择、排序和回退。
- Directory kind 的最近 `workspace` provider 选择。

`plugins` 实现通用 Action 对话框、diagnostic、view renderer、sandboxed iframe 和
Resource/Directory Frame Host Bridge。Kernel 与 plugins 不包含具体插件 ID 或具体文件格式实现。

### `app`、主题与样式

`app` 只定义 Provider 组合和路由。路由及大型 Feature 使用 lazy import，认证边界位于业务路由
外层。

Material UI 主题集中管理宿主 palette、typography、shape、表面和共享组件默认样式。Feature
只保留布局相关样式；插件文本和 Markdown 的少量宿主排版位于 `styles.css`。

## 3. 组装与状态归属

启动顺序：

```text
main.tsx
  ├─ createOpenApiGateways
  ├─ PluginKernel + 通用 renderer
  ├─ TanStack QueryClient
  └─ AppProviders
       ├─ ThemeProvider / CssBaseline
       ├─ QueryClientProvider
       ├─ GatewayProvider
       └─ PluginKernelProvider
            └─ AuthBoundary
                 └─ RouterProvider
                      ├─ AssetWorkspace
                      └─ StandaloneResourcePluginView
```

状态只有一个主要所有者：

| 状态 | 归属 |
| --- | --- |
| 当前目录、搜索、kind、分页、选中 Resource/Directory | URL path 与 search params |
| Resource、Directory、Kind、用户、授权、Session | TanStack Query server cache |
| 创建、编辑、上传表单 | React Hook Form 或局部组件状态 |
| 当前登录用户读取 | Session Context |
| view renderer、slot 和 provider 规则 | PluginKernel |

因此不使用一个同时承载服务端数据、表单和 UI 状态的全局 Store。刷新页面可以恢复导航和筛选；
写操作完成后更新必要快照并失效相关 Query，以后端状态为最终事实。

## 4. 运行时流程

### 认证与路由

`AuthBoundary` 首先通过 `AuthGateway.currentUser` 加载 Session：

- 未认证用户被送到 `/login`。
- 登录成功后清理旧业务缓存并写入新的 Session。
- 已认证用户访问 `/login` 时返回根目录。
- 退出成功后清除业务缓存并回到登录页。

### 浏览与查询

```text
URL path / search params
  → useAssetWorkspaceListing
  → AssetWorkspaceGateway
  → OpenAPI HTTP module
  → asset-http
  → DTO 映射为 Domain 对象
  → TanStack Query cache
  → React 重新渲染
```

Resource Kind、Directory Kind 和目录 listing 使用稳定 Query Key。存在内容验证状态为 `pending`
的 Resource 时，listing 临时轮询；验证结束后停止。

Directory 在 Domain 和 Gateway 中始终使用稳定 UUID 标识，path 只用于导航和显示。切换目录时
清除分页与选中项，Resource 和 Directory 选中状态互斥。

### 命令、并发与上传

资源更新、恢复、目录创建、Directory kind 更新和 Action 执行统一通过 React Query mutation。
写入成功后更新必要 Resource 快照并失效目录或详情 Query。稳定错误码
`concurrency.revision_conflict` 会触发刷新并提示用户已加载最新版本。

Directory 更新和 write Action 携带当前 revision；read Action 读取最新授权快照，避免预览缓存
产生无意义冲突。根 Directory 的 kind 与其他根元数据一样不可修改。

上传流程：

```text
计算文件 SHA-256
  → 创建或恢复上传会话
  → 分块上传并校验每个 chunk
  → checksum mismatch 时有限重试
  → 完成会话
  → 轮询后台验证与发布
  → Resource 就绪后刷新缓存
```

恢复指纹包含文件 SHA-256，避免同名、同大小但内容不同的文件错误复用上传会话。

## 5. 插件宿主模型

### Action 与 Provider

后端返回实际适用的 Resource/Directory Action 扁平数组。每个 Action 声明：

- builtin 或 plugin `origin`。
- `read` 或 `write` access。
- 可能产生的 `output.views` 和 `output.effects`。
- UI location、排序、破坏性确认和 capability `provides`。

后端在返回前解析 singleton capability provider。Provider 可以匹配 Kind、MIME 或扩展名；同一
Kind 层级冲突由 Host 启动校验处理。前端只执行已经解析出的 Action，不内置具体格式 Kind、读取器
或编辑器。

### Host 交接点

稳定的宿主 location：

| Location | 所有者与行为 |
| --- | --- |
| `directory_workspace` | 顶层交接点；最近 Kind 的只读 `workspace` provider 完整替换 Core 内容区 |
| `directory_context_menu` | Core Directory 行菜单 |
| `directory_thumbnail` | Core Directory 行和详情标题缩略图 |
| `resource_context_menu` | Core Resource 行菜单 |
| `resource_thumbnail` | Core Resource 行和详情标题缩略图 |

没有 `workspace` provider 时挂载 `CoreDirectoryWorkspace`。存在 provider 时 Core 工作区不挂载，
其四个内部 location 也不存在，插件 iframe 完全拥有内容画布。路径面包屑和当前 Directory kind
编辑器属于外层 Host Shell，因此始终保留。

未声明 location 或只声明当前 Host 不认识的 location 时，Resource/Directory Action 分别回退到
对应 context menu。自动 thumbnail 只接受解析后的只读 `thumbnail` provider；没有 provider 时
显示本地 File 或 Folder 图标，不执行 Action。

### View 与渲染

支持 `text`、`markdown`、`html`、`json`、`media`、`download` 和 `plugin_frame`。Resource 与
Directory 共用通用 renderer；HTML 注入禁止网络访问的 CSP。Markdown 和媒体 renderer 按需加载，
不进入基础首屏包。

只有 `plugin_frame` 使用聚合专属 Bridge。插件资源 URL 必须是验证后的 `/plugins/<id>/...` 路径；
iframe 使用 `sandbox="allow-scripts"`，在 opaque origin 下运行。

### Frame Host capability

Frame 协议版本、channel、Host method、view/effect、Action output 和 capability ID 统一来自
`@asset-hub/asset-web-sdk/contract`，宿主不维护字符串副本。Frame Action 输入在进入 Gateway 前
必须是有界 JSON 对象：最多 32 层、10,000 个值，并拒绝循环引用、非有限数字和非 JSON 值。

Resource Frame：

- 只能执行当前 Resource 已暴露的 Action。
- destructive Action 必须先由 Host 确认。
- 只有产生当前 Frame 的 write `edit` provider 可以替换当前 Resource 文本。
- 保存成功后立即推进 Bridge 快照，并同步详情、列表和 Action 对话框缓存。

Directory Frame：

- 只能执行绑定 Directory 已暴露的 Action、刷新当前 Directory 或请求 Host 导航。
- 只能通过不透明 ID 访问绑定 Directory 的直属 Resource。
- Host 重新读取 Resource，并自行解析当前只读 `view` 或 write `edit` provider；插件不能指定
  provider Action ID。
- 嵌套 Resource 读取 Frame 只中继起始只读 provider，Directory Frame 不能直接替换 Resource
  内容。

两个 Bridge 始终绑定初始聚合 ID，只接受同 ID 且 revision 不回退的快照。导航到另一 Directory
或 Host 主动提升 instance version 时重新挂载 iframe，既有连接不会改绑到另一聚合。

### 控制面与数据面

Action JSON 只承载控制信息。Resource 原始内容走流式数据面：文本替换通过
`PUT /resources/{id}/content`，使用 `Content-SHA256` 校验完整性，并通过 `If-Match` 携带打开编辑器
时的 revision。插件 Manifest 必须显式申请 `resource.content.replace`，超出
`resource_edit.max_text_bytes` 的 Resource 不暴露 `edit` provider。

## 6. 维护边界与扩展规则

必须维持：

- Feature 不导入 `infra/http/generated.ts` 或直接调用传输实现。
- OpenAPI DTO 不穿过 `infra/http`。
- 新请求能力只加入实际消费它的 Feature Gateway，不扩张成全局 API 接口。
- 宿主组件不硬编码具体插件 ID、Kind 或 Action ID。
- 自动 slot 不执行 write Action。
- Frame Action 必须重新验证当前聚合、Action、access、capability 和必要的 Host 确认。
- 外部 URL 不作为插件媒体或 iframe 地址加载。
- 行为、公共协议、配置或运行流程变化时，同步更新本文档。

以下变化属于前端宿主协议升级，需要同步 Web SDK、Host、测试和文档：

- 增加新的 view kind。
- 增加具有新布局语义的 Host slot。
- 增加或改变 Frame Host method。
- 升级 Frame 协议主版本。

添加只使用既有 capability、slot 和 view kind 的插件不需要修改或重新构建 `asset-web`。
