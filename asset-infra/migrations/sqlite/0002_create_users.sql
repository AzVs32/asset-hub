-- 用户表：保存可登录用户的身份、凭证摘要、角色、状态和工作目录。
CREATE TABLE users (
    -- UUID v7 用户标识。
    id TEXT PRIMARY KEY NOT NULL,
    -- 唯一登录用户名。
    username TEXT NOT NULL UNIQUE,
    -- 密码哈希；不保存明文密码。
    password_hash TEXT NOT NULL,
    -- 用户角色，只允许管理员或普通成员。
    role TEXT NOT NULL CHECK (role IN ('administrator', 'member')),
    -- 用户状态；禁用用户不能通过认证。
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    -- 普通用户唯一的访问边界；用户可完整访问该目录及其全部后代目录。
    -- 管理员不受该字段限制，默认使用根目录。
    workspace_directory TEXT NOT NULL,
    -- 用户创建时间，使用 RFC 3339 文本表示。
    created_at TEXT NOT NULL,
    -- 用户最后更新时间，使用 RFC 3339 文本表示。
    updated_at TEXT NOT NULL
);
