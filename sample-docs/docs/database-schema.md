# 数据库 Schema — OrbitBoard

PostgreSQL 16,所有表使用 `uuid` 主键 + `timestamptz` 时间戳。逻辑删除字段 `deleted_at`。

## 1. 用户与成员

```sql
CREATE TABLE users (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  email         citext UNIQUE NOT NULL,
  name          text NOT NULL,
  avatar_url    text,
  password_hash text NOT NULL,
  created_at    timestamptz NOT NULL DEFAULT now(),
  updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspaces (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name        text NOT NULL,
  slug        citext UNIQUE NOT NULL,
  owner_id    uuid NOT NULL REFERENCES users(id),
  plan        text NOT NULL DEFAULT 'free',  -- free/pro/enterprise
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
  user_id      uuid REFERENCES users(id) ON DELETE CASCADE,
  workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
  role         text NOT NULL CHECK (role IN ('owner','admin','member','guest')),
  joined_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, workspace_id)
);
```

## 2. 项目与看板

```sql
CREATE TABLE projects (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name         text NOT NULL,
  description  text,
  color        text,
  icon         text,
  archived_at  timestamptz,
  created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE boards (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name        text NOT NULL,
  template    text NOT NULL DEFAULT 'blank',
  position    int  NOT NULL DEFAULT 0,
  created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE columns (
  id       uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  name     text NOT NULL,
  position int  NOT NULL DEFAULT 0,
  wip_limit int,
  color    text
);
```

## 3. 卡片

```sql
CREATE TABLE cards (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  board_id     uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  column_id    uuid NOT NULL REFERENCES columns(id),
  number       int  NOT NULL,            -- 项目内自增编号,如 ORB-124
  title        text NOT NULL,
  description  text,                     -- Markdown
  assignee_id  uuid REFERENCES users(id),
  priority     text CHECK (priority IN ('low','medium','high','urgent')),
  due_at       timestamptz,
  estimate     int,                      -- 分钟
  version      int  NOT NULL DEFAULT 1,   -- 乐观锁
  position     text NOT NULL,            -- Fractional indexing 字符串
  created_by   uuid NOT NULL REFERENCES users(id),
  created_at   timestamptz NOT NULL DEFAULT now(),
  updated_at   timestamptz NOT NULL DEFAULT now(),
  deleted_at   timestamptz
);

CREATE INDEX cards_board_idx   ON cards(board_id) WHERE deleted_at IS NULL;
CREATE INDEX cards_column_idx  ON cards(column_id) WHERE deleted_at IS NULL;
CREATE INDEX cards_assignee_idx ON cards(assignee_id) WHERE deleted_at IS NULL;
```

### Fractional indexing

卡片在同一列内的顺序用字符串 `position` 表示(如 `a0`, `a1`, `m`)。移动只更新被移动卡片的 position,避免重排整列。库:`fractional_indexing` rust crate。

## 4. 评论与活动

```sql
CREATE TABLE comments (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  author_id  uuid NOT NULL REFERENCES users(id),
  body       text NOT NULL,            -- Markdown,支持 @mention
  created_at timestamptz NOT NULL DEFAULT now(),
  edited_at  timestamptz,
  deleted_at timestamptz
);

CREATE TABLE activities (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  card_id    uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  actor_id   uuid REFERENCES users(id),
  kind       text NOT NULL,            -- created/moved/assigned/commented/...
  payload    jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX activities_card_idx ON activities(card_id, created_at DESC);
```

## 5. 标签与附件

```sql
CREATE TABLE labels (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name        text NOT NULL,
  color       text NOT NULL,
  UNIQUE(project_id, name)
);

CREATE TABLE card_labels (
  card_id  uuid REFERENCES cards(id) ON DELETE CASCADE,
  label_id uuid REFERENCES labels(id) ON DELETE CASCADE,
  PRIMARY KEY (card_id, label_id)
);

CREATE TABLE attachments (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  card_id     uuid NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
  uploader_id uuid NOT NULL REFERENCES users(id),
  filename    text NOT NULL,
  mime_type   text NOT NULL,
  size_bytes  bigint NOT NULL,
  storage_key text NOT NULL,          -- S3 object key
  thumb_key   text,                   -- 缩略图 key
  created_at  timestamptz NOT NULL DEFAULT now()
);
```

## 6. 实时与持久化

```sql
CREATE TABLE board_snapshots (
  board_id   uuid PRIMARY KEY REFERENCES boards(id) ON DELETE CASCADE,
  doc_bytes  bytea NOT NULL,          -- Automerge doc 二进制
  seq        bigint NOT NULL,         -- 已持久化的最大 change seq
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE board_changes (
  board_id  uuid REFERENCES boards(id) ON DELETE CASCADE,
  seq       bigint NOT NULL,
  change    bytea NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (board_id, seq)
);
-- 保留最近 1000 条 change,更早的合并到 snapshot
```

## 7. 索引与搜索

- `pg_trgm` 在 `cards.title`、`comments.body` 上建 GIN 索引用于模糊搜索
- Meilisearch 异步索引 `cards` + `comments`,支持高亮与分面
- 全文检索回退:数据库 `tsvector` (simple 配置,中文用 jieba 预分词写入)

## 8. 迁移策略

- 工具:`sqlx migrate`(纯 SQL 文件)
- 命名:`migrations/0001_init.sql`, `0002_add_labels.sql`, ...
- 部署前自动备份:`pg_dump` 到 S3,失败回滚
- 破坏性变更分两步:先加列/表 → 代码切换 → 后删旧(Expand-Contract)