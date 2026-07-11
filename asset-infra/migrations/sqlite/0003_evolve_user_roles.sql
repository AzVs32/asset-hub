ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'member'
    CHECK (role IN ('administrator', 'member'));
ALTER TABLE users ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'disabled'));
UPDATE users SET role = CASE WHEN is_admin = 1 THEN 'administrator' ELSE 'member' END;
