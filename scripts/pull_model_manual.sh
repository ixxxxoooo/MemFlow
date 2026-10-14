#!/bin/bash
# 手动拉取 Ollama 模型(绕过 ollama 对 fake-ip(198.18.x.x) 重定向的拦截)
# 用法: scripts/pull_model_manual.sh bge-m3 latest
#       scripts/pull_model_manual.sh qwen2.5 7b
# 原理: 用 curl 下载 manifest 与各 blob(支持断点续传),校验 sha256 后写入 ~/.ollama/models
# @author ygw
set -euo pipefail

NAME="${1:?缺少模型名}"
TAG="${2:-latest}"
REG="https://registry.ollama.ai/v2/library/${NAME}"
ROOT="${OLLAMA_MODELS:-$HOME/.ollama/models}"
MF_DIR="$ROOT/manifests/registry.ollama.ai/library/${NAME}"
mkdir -p "$ROOT/blobs" "$MF_DIR"

MANIFEST="$(curl -fsS --retry 5 -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' "$REG/manifests/$TAG")"
# 取出 config 与全部 layer 的 digest
DIGESTS="$(printf '%s' "$MANIFEST" | python3 -c '
import sys, json
m = json.load(sys.stdin)
print(m["config"]["digest"])
for l in m["layers"]:
    print(l["digest"])
')"

for D in $DIGESTS; do
  FILE="$ROOT/blobs/${D/:/-}"
  HEX="${D#sha256:}"
  if [ -f "$FILE" ] && [ "$(shasum -a 256 "$FILE" | cut -d' ' -f1)" = "$HEX" ]; then
    echo "已存在且校验通过: $D"
    continue
  fi
  echo "下载 $D ..."
  # 下载到 .part 并断点续传,失败自动重试
  for i in 1 2 3 4 5 6 7 8 9 10; do
    if curl -fL --retry 5 --retry-delay 3 -C - -o "$FILE.part" "$REG/blobs/$D"; then break; fi
    echo "第 $i 次中断,重试..."; sleep 3
  done
  GOT="$(shasum -a 256 "$FILE.part" | cut -d' ' -f1)"
  if [ "$GOT" != "$HEX" ]; then echo "校验失败: $D (实际 $GOT)"; rm -f "$FILE.part"; exit 1; fi
  mv "$FILE.part" "$FILE"
done

printf '%s' "$MANIFEST" > "$MF_DIR/$TAG"
echo "完成: ${NAME}:${TAG}"
