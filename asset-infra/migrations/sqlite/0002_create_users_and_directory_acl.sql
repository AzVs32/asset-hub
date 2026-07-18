-- 用户表：保存可登录用户的身份、凭证摘要、角色、状态和默认工作目录。
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
    -- 登录后的默认入口目录；该字段本身不授予访问权限。
    workspace_directory TEXT NOT NULL,
    -- 用户创建时间，使用 RFC 3339 文本表示。
    created_at TEXT NOT NULL,
    -- 用户最后更新时间，使用 RFC 3339 文本表示。
    updated_at TEXT NOT NULL
);

-- 目录访问控制表：保存用户在逻辑目录上的显式授权。
CREATE TABLE directory_acl (
    -- 被授权目录的规范化路径；授权可应用于该目录及其后代目录。
    directory_path TEXT NOT NULL,
    -- 获得授权的用户标识。
    user_id TEXT NOT NULL,
    -- 目录权限级别：只读、读写或完全控制。
    permission TEXT NOT NULL CHECK (permission IN ('read', 'write', 'full')),
    -- 授权创建时间，使用 RFC 3339 文本表示。
    created_at TEXT NOT NULL,
    -- 同一用户在同一目录上最多拥有一条显式授权。
    PRIMARY KEY (directory_path, user_id),
    -- 删除用户时级联删除该用户的全部目录授权。
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- 加速按用户和目录路径查询直接授权及祖先目录授权。
CREATE INDEX idx_directory_acl_user_path
ON directory_acl(user_id, directory_path);
