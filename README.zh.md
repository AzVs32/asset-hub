# Asset Hub

Asset Hub 是一个以本地为主的资源管理系统，支持多个用户账户，并提供 Web 页面和本地管理命令。

## 入门指南

安装 Web 应用依赖：

```bash
npm --prefix asset-web ci
```

创建首位管理员：

```bash
cargo run -p asset-cli --bin asset -- user --create admin --admin
```

启动 API：

```bash
cargo run -p asset-http --bin asset-http
```

默认监听 `http://127.0.0.1:8080`。在另一个终端启动 Web 页面：

```bash
cd asset-web
npm run dev
```

打开 `http://127.0.0.1:5173`。
