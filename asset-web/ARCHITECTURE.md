# asset-web 架构

## 1. 总体原则

`asset-web` 使用与 Rust 后端相同的边界思想，但不机械模仿 crate：

- 领域对象不感知 HTTP、React 或后端 DTO。
- 功能代码面向 `AssetGateway` 端口，不直接调用 `fetch`。
- OpenAPI 类型只存在于 HTTP 基础设施适配器内。
- 插件通过资源 action、稳定插槽和通用 view 协议进入宿主。
- `main.tsx` 是唯一组装具体实现的 composition root。

依赖方向如下：

```text
                              ┌──────────────────────┐
                              │ infrastructure/http  │
                              │ OpenAPI adapter      │
                              └──────────┬───────────┘
                                         │ implements
┌──────────┐    uses     ┌────────────────▼─┐    uses    ┌───────────────────┐
│ features │ ──────────▶ │ application/ports │ ─────────▶ │ domain            │
└────┬─────┘             └──────────────────┘             └───────────────────┘
     │
     ├────────▶ kernel / plugins
     └────────▶ shared/ui

app/main 负责创建并连接以上所有外层实现。
```

## 2. 目录职责

### `domain`

定义前端内部稳定的业务语言：

- `Resource`、`ResourceAction`、`ResourceKind`。
- 登录用户、受管用户和目录授权。
- 插件 view 与 action output 的联合类型。
- 资源草稿、目录规范化、面包屑等无副作用规则。

领域层只使用 TypeScript，不允许导入 React、OpenAPI 或请求库。字段使用前端统一的
camelCase；snake_case 被限制在 HTTP 适配器中。
Action 领域模型只包含发现和展示所需的声明，不包含后端私有的 executor 或 handler
binding。

### `application`

`application/ports/asset-gateway.ts` 是前端面对后端的 SPI，声明认证、资源、目录、插件
action 和用户授权能力。Feature 只知道这个接口。

`application/queries/keys.ts` 定义服务端状态的统一身份，避免每个页面自行发明缓存 key。
`gateway-context.tsx` 只负责将端口实例注入 React 树，不包含业务规则。
`application/errors.ts` 定义 feature 可以理解的应用错误；例如 HTTP 401 会在 adapter 中转换为
`AuthenticationRequiredError`，认证界面无需认识 HTTP 状态码。

### `infrastructure/http`

`OpenApiAssetGateway` 是 `AssetGateway` 的当前实现：

1. 使用 `openapi-fetch` 调用后端。
2. 将 OpenAPI snake_case DTO 转换为 domain camelCase 对象。
3. 将资源草稿转换为请求 DTO。
4. 统一把错误转换成 `HttpError`。
5. 用 Zod 校验不可信的插件 view JSON。
6. 限制插件媒体和 iframe 只能访问后端同源路径。

以后改成 Tauri command、本地 mock 或另一个 HTTP 协议时，只需提供新的端口实现。

### `kernel`

微内核只管理两件事：

- view kind 到 React renderer 的注册表。
- resource action 到稳定宿主 slot 的选择、排序和回退。

内核不认识 Markdown、EPUB、视频等具体插件。未知 slot 的 action 会回退到
`resource_detail`，避免插件 action 因宿主版本较旧而完全不可访问。

### `plugins`

这是通用插件宿主，而不是具体插件实现：

- `automatic-slot` 自动执行明确放入自动插槽的只读 action。
- `plugin-action-dialog` 承载用户触发的 action 结果。
- `plugin-output` 统一展示 diagnostics 并进入 view renderer。
- `renderers` 支持 text、Markdown、HTML、JSON、media、download 和 iframe。
- `frame-protocol` 校验 iframe 消息；iframe 只能调用当前资源已经暴露的 action，且只有由
  当前 `text_edit` provider 打开的读写 frame 才能请求替换当前资源文本。

Markdown 和媒体播放器均按需加载，不进入基础首屏包。

### `features`

Feature 是用户用例与界面的组合边界：

- `auth`：加载 session、登录和 session context。
- `resources`：目录列表、URL 筛选、资源命令、详情、上传和插件 action。
- `users`：管理员、用户状态和目录授权。

资源查询与命令分别放在 `use-resource-listing` 和 `use-resource-commands` 中，组件主要负责
展示与事件绑定。

### `shared/ui`

只放无业务语义的宿主 UI 原语，例如 Button、Dialog、Field 和状态提示。复杂交互使用
Radix 提供焦点管理、键盘行为和 overlay 语义。

### `app` 与 `main.tsx`

启动顺序：

```text
main.tsx
  ├─ 创建 OpenApiAssetGateway
  ├─ 创建 PluginKernel 并注册通用 renderer
  ├─ 创建 TanStack QueryClient
  └─ AppProviders
       └─ AuthBoundary
            └─ RouterProvider
                 ├─ ResourceWorkspace
                 └─ StandalonePluginView
```

路由和大功能使用 lazy import。认证在路由外层，因此所有业务路由默认受保护。

## 3. 状态归属

不同状态有明确的唯一归属：

| 状态 | 归属 |
| --- | --- |
| 当前目录、搜索、kind、分页、选中资源 | URL search params |
| 资源、目录、kind、用户、授权、session | TanStack Query server cache |
| 创建、编辑、上传表单 | React Hook Form/local component state |
| 当前登录用户读取 | Session context |
| view renderer 与 slot 规则 | PluginKernel |

因此不需要一个同时承载服务端数据、表单和 UI 状态的全局 store。刷新页面可以恢复导航和
筛选状态，写操作成功后通过 query invalidation 获取后端真实状态。

## 4. 资源请求链路

```text
ResourceWorkspace
  → useResourceListing / useResourceCommands
  → AssetGateway port
  → OpenApiAssetGateway
  → asset-http
  → DTO 映射为 Resource domain model
  → TanStack Query 更新缓存
  → React 重新渲染
```

Resource 与 Directory action 都是扁平数组，使用 `read` / `write` access 和
`output.views`；Kind 与 Action 的 `origin` 明确区分 Host 内建定义和插件定义。
Directory 在 Domain 和 Gateway 中始终以稳定 UUID 标识，path 仅用于导航与显示。目录写入
以及 `write` Action 执行携带当前 revision，过期页面不能覆盖已经提交的并发修改；`read`
Action 默认读取最新授权快照，不会因为缩略图或预览缓存较旧而产生无意义的 409。稳定错误码
`concurrency.revision_conflict` 会触发相关 query invalidation，并提示用户已加载最新版本。

## 5. 插件执行链路

```text
后端 Resource.actions
  → PluginKernel 按 ui.locations 放入 slot
  → 用户触发或 AutomaticSlot 自动触发只读 action
  → AssetGateway.executeAction
  → Zod 校验 PluginView
  → PluginViewHost 查询 renderer registry
  → 通用 renderer 或 sandboxed plugin_frame
```

当前稳定插槽：

| slot | 行为 |
| --- | --- |
| `resource_detail` | 详情操作按钮，用户触发 |
| `context_menu` | 列表行菜单，用户触发 |
| `resource_list_thumbnail` | 列表缩略图，只读自动执行 |
| `directory_list_thumbnail` | 目录列表缩略图，只读自动执行 |
| `resource_detail_panel` | 详情事实区域下方，只读自动执行 |
| `resource_detail_aside` | 核心编辑器上方，只读自动执行 |

插件只要在 manifest 中声明已有 slot，并返回已有 view kind，就不需要修改前端。完全自定义
界面通过 `plugin_frame` 加载插件自己的 Web 资源。
后端会在实际适用性过滤后解析单例能力 provider；例如 EPUB 的
`azvs.epub.thumbnail` 为 EPUB 提供作用于 Resource Action 的 `thumbnail`。Resource 与
Directory Action 注册表分别限定该能力的作用域。Host 对图片也遵循相同边界：
`core.image.thumbnail` 仅适用于 `core:image`；通用 provider 本身始终保持 kind-neutral，
不根据 MIME 特判图片。相同能力选择 Kind 谱系中最近的 provider，同层冲突会导致 Host
启动失败，前端只执行后端已经解析出的 provider。文本能力同样按此规则解析：
`core.text.read` 和 `core.text.edit` 是 `core:text` 的纯文本回退 provider，而 Markdown
插件分别以 `azvs.markdown.read` 和 `azvs.markdown.edit` 提供 `text_read` 与 `text_edit`，
从而在 `azvs:markdown` 上取代 Host 的纯文本界面。
Host 内建纯文本编辑器只绑定稳定 ID `core.text.edit`，不会假定第三方 `text_edit`
provider 接受相同输入。Action 只负责能力发现和返回初始文本；保存不再把完整文本塞入
Action JSON，而是通过 `AssetGateway.replaceResourceText` 将 UTF-8 原始字节流提交到
`PUT /resources/{id}/content`。请求使用 `Content-SHA256` 做端到端完整性校验，并将打开
编辑器时的 `Resource.revision` 放入 `If-Match`；Host 检测到资源或其目录位置已经变化时
返回冲突并恢复原 Blob。`resource_edit.max_text_bytes` 由 Core 同时用于能力发现和执行，
超限资源不会暴露 `text_edit`。

因此 Action JSON 属于控制面，资源原始内容属于流式数据面。HTTP Action 的 1 MiB 请求
限制不会再限制文本保存，Blob 数据也不需要经过 JSON 转义或 Base64 膨胀。
插件 iframe 通过 `asset-hub:replace-resource-text` 消息进入同一个 Gateway；宿主同时校验
frame 对应的原始 action 是当前资源解析出的读写 `text_edit` provider。保存成功后 frame
持有响应中的新 revision，以支持同一编辑窗口连续保存。

只有以下变化属于前端宿主协议升级：

- 增加一种全新的 view kind。
- 增加具有新布局语义的宿主 slot。
- 升级 iframe message protocol 的主版本。

## 6. 必须维持的边界

- Feature 不允许导入 `infrastructure/http/generated.ts`。
- OpenAPI DTO 不允许穿过 `OpenApiAssetGateway`。
- 具体插件 id、kind 或 action id 不允许硬编码进宿主组件。
- 自动 slot 不允许执行 write action。
- iframe action 必须先在当前 `Resource.actions` 中验证；文本替换还必须绑定产生当前 frame
  的 `write` `text_edit` action。
- 外部 URL 不允许作为插件媒体或 iframe 地址加载。
- 新的后端请求能力先加入 `AssetGateway`，再实现 HTTP adapter，最后由 feature 使用。
