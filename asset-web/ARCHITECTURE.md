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

- `Resource`、`ResourceMetadata`、`ResourceAction`、`ResourceKind`。
- 登录用户、受管用户和目录授权。
- 插件 view 与 action output 的联合类型。
- 资源草稿、目录规范化、面包屑等无副作用规则。

领域层只使用 TypeScript，不允许导入 React、OpenAPI 或请求库。字段使用前端统一的
camelCase；snake_case 被限制在 HTTP 适配器中。

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
- `renderers` 支持 text、Markdown、HTML、JSON、table、media、binary、form 和 iframe。
- `frame-protocol` 校验 iframe 消息，iframe 只能调用当前资源已经暴露的 action。

Markdown、媒体播放器和 JSON Schema 表单均按需加载，不进入基础首屏包。

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
| 当前目录、搜索、kind、tag、分页、选中资源 | URL search params |
| 资源、目录、kind、用户、授权、session | TanStack Query server cache |
| 创建、编辑、上传、插件表单 | React Hook Form/local component state |
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

编辑 summary 时只发送 `metadata.summary`。后端 patch 语义负责保留未修改的
`kind_metadata`；kind metadata 当前在详情中只读展示，等待每个 kind 的独立 schema 和编辑器。

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
| `resource_detail_panel` | 详情事实区域下方，只读自动执行 |
| `resource_detail_aside` | 核心编辑器上方，只读自动执行 |

插件只要在 manifest 中声明已有 slot，并返回已有 view kind，就不需要修改前端。完全自定义
界面通过 `plugin_frame` 加载插件自己的 Web 资源。

只有以下变化属于前端宿主协议升级：

- 增加一种全新的 view kind。
- 增加具有新布局语义的宿主 slot。
- 升级 iframe message protocol 的主版本。

## 6. 必须维持的边界

- Feature 不允许导入 `infrastructure/http/generated.ts`。
- OpenAPI DTO 不允许穿过 `OpenApiAssetGateway`。
- 具体插件 id、kind 或 action id 不允许硬编码进宿主组件。
- 自动 slot 不允许执行 write action。
- iframe action 必须先在当前 `Resource.actions` 中验证。
- 外部 URL 不允许作为插件媒体或 iframe 地址加载。
- 新的后端请求能力先加入 `AssetGateway`，再实现 HTTP adapter，最后由 feature 使用。
