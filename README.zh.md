# Asset Hub

Asset Hub 是一个以本地为主的资源管理系统，支持多个用户账户。
它提供一个 Web 页面和本地管理命令，并且可以通过插件进行拓展。

## 入门指南

安装捆绑插件和 Web 应用所需的依赖：

```bash
rustup target add wasm32-unknown-unknown
npm --prefix asset-plugin-sdk/web ci
npm --prefix plugins/resource-text/web ci
npm --prefix plugins/azvs-epub/web ci
npm --prefix plugins/directory-games/web ci
npm --prefix asset-web ci
```

## 1. 构建并安装捆绑插件

从仓库根目录构建插件：

```bash
plugins/resource-text/build.sh
plugins/resource-image/build.sh
plugins/azvs-epub/build.sh
plugins/directory-games/build.sh
```

然后将其安装到 Asset Hub：

```bash
cargo run -p asset-cli --bin asset -- plugin --install plugins/resource-text/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/resource-image/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/azvs-epub/asset-plugin-target
cargo run -p asset-cli --bin asset -- plugin --install plugins/directory-games/asset-plugin-target
```

### 创建首位管理员

```bash
cargo run -p asset-cli --bin asset -- user --create admin --admin
```

在系统提示时输入管理员密码。

### 3. 启动 API

```bash
cargo run -p asset-http --bin asset-http
```

默认情况下，API 在 `http://127.0.0.1:8080` 上进行监听。

### 4. 启动 Web 页面

在另一个终端中：

```bash
cd asset-web
npm run dev
```

打开 `http://127.0.0.1:5173`。