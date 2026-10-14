# 部署指南 — OrbitBoard

## 1. Docker Compose(单机)

```bash
git clone https://github.com/orbitboard/orbitboard.git
cd orbitboard
cp .env.example .env
docker compose up -d
```

`docker-compose.yml` 包含:api, worker, web, postgres, redis, minio, meilisearch, nginx(反代+TLS)。

健康检查:`docker compose ps` 全部 healthy 后访问 https://localhost。

## 2. 生产变量(.env)

```
DATABASE_URL=postgres://...
REDIS_URL=redis://...
S3_ENDPOINT=https://s3.amazonaws.com
S3_BUCKET=orbitboard-prod
S3_ACCESS_KEY=...
S3_SECRET_KEY=...
JWT_PRIVATE_KEY_PATH=/secrets/jwt.pem
SMTP_URL=smtps://...
MEILISEARCH_URL=http://meilisearch:7700
MEILISEARCH_KEY=...
```

## 3. Kubernetes(Helm)

```bash
helm repo add orbitboard https://charts.orbitboard.dev
helm install orbitboard orbitboard/orbitboard \
  -f values.prod.yaml \
  -n orbitboard --create-namespace
```

`values.yaml` 关键字段:replicaCount, postgresql.enabled(外部 DB 关闭), redis.enabled, ingress, resources。

WebSocket 需 sticky session:Ingress 注解 `nginx.ingress.kubernetes.io/affinity: cookie`。

## 4. 数据库迁移

迁移在 API pod 启动前通过 Helm pre-install Job 执行:
```yaml
hooks:
  - pre-install
  - pre-upgrade
command: ["sqlx", "migrate", "run"]
```

## 5. 备份与恢复

- PostgreSQL:pgBackRest 每日全量 + WAL 增量,保留 14 天,异步上传 S3
- MinIO/S3:版本控制 + 跨区域复制
- 恢复演练:每季度一次,记录 RTO/RPO

## 6. 升级与回滚

- 发布:Git tag `v1.2.3` → CI 构建镜像 `ghcr.io/orbitboard/api:1.2.3` → Helm upgrade
- 回滚:`helm rollback orbitboard -n orbitboard` (保留上 5 个 revision)
- 数据库回滚:若迁移有破坏性变更,需执行逆向迁移脚本(在 `migrations/down/`)

## 7. 监控

- Prometheus scrape `/metrics`
- Grafana dashboards:API 延迟、WS 连接、DB 连接池、队列深度、错误率
- 告警:API 5xx > 1% 5min、队列堆积 > 1000、DB 连接池饱和

## 8. 安全加固

- 所有 Secret 通过 Kubernetes Secret / 外部 Secret Operator(Vault/AWS SM)
- Pod Security Standard:restricted
- NetworkPolicy:仅 ingress → api, api → db/redis/minio
- 镜像扫描:Trivy,CRITICAL 阻断发布