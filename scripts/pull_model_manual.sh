#!/bin/bash
# 手动拉取 Ollama 模型(绕过 ollama 对 fake-ip(198.18.x.x) 重定向的拦截)
#
# 用法:
#   scripts/pull_model_manual.sh bge-m3 latest
#   scripts/pull_model_manual.sh qwen2.5 7b
#   scripts/pull_model_manual.sh qwen2.5:7b            # 也支持 名称:标签 写法
#   scripts/pull_model_manual.sh someone/model:tag     # 支持 命名空间/名称
#
# 环境变量:
#   OLLAMA_REGISTRY  模型仓库地址,默认 https://registry.ollama.ai,可改为可访问的镜像
#   OLLAMA_MODELS    本地模型目录,默认 ~/.ollama/models
#
# 原理: 用 curl 下载 manifest 与各 blob(断点续传),校验 sha256 后写入 ~/.ollama/models。
# 只依赖 macOS 自带的 curl / shasum / grep,不需要 python。
# 应用内设置页的「直连下载」按钮使用相同逻辑,一般无需手动运行本脚本。
# @author ixxxxoooo
set -euo pipefail

ARG1="${1:?缺少模型名,例如: $0 qwen2.5:7b}"
ARG2="${2:-}"

# 解析 名称[:标签] 与可选命名空间
if [ -z "$ARG2" ] && [[ "$ARG1" == *:* ]]; then
  FULL="${ARG1%%:*}"
  TAG="${ARG1#*:}"
else
  FULL="$ARG1"
  TAG="${ARG2:-latest}"
fi
if [[ "$FULL" == */* ]]; then
  NS="${FULL%%/*}"
  NAME="${FULL#*/}"
else
  NS="library"
  NAME="$FULL"
fi

# 只允许安全字符,避免路径穿越;不支持 hf.co 等第三方仓库
for V in "$NS" "$NAME" "$TAG"; do
  if ! [[ "$V" =~ ^[A-Za-z0-9._-]+$ ]] || [ "$V" = "." ] || [ "$V" = ".." ]; then
    echo "模型名不合法: $V" >&2; exit 2
  fi
done
if [[ "$NAME" == */* ]] || [[ "$NS" == *.* && "$NS" != "library" ]]; then
  echo "只支持 Ollama 官方仓库的模型,例如 qwen2.5:7b" >&2; exit 2
fi

REGISTRY="${OLLAMA_REGISTRY:-https://registry.ollama.ai}"
REGISTRY="${REGISTRY%/}"
REG="$REGISTRY/v2/${NS}/${NAME}"
ROOT="${OLLAMA_MODELS:-$HOME/.ollama/models}"
# Ollama 在本地固定读取 registry.ollama.ai 目录,与实际下载自哪个镜像无关
MF_DIR="$ROOT/manifests/registry.ollama.ai/${NS}/${NAME}"
mkdir -p "$ROOT/blobs" "$MF_DIR"

echo "获取模型清单: ${NS}/${NAME}:${TAG} ($REGISTRY)"
MANIFEST="$(curl -fsS --retry 5 --connect-timeout 15 \
  -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' \
  "$REG/manifests/$TAG")"

# 取出 config 与全部 layer 的 digest(去重)
DIGESTS="$(printf '%s' "$MANIFEST" | grep -oE 'sha256:[0-9a-f]{64}' | sort -u)"
if [ -z "$DIGESTS" ]; then echo "清单中没有找到任何 blob,请检查模型名" >&2; exit 1; fi

sha_of() { shasum -a 256 "$1" | cut -d' ' -f1; }

for D in $DIGESTS; do
  FILE="$ROOT/blobs/${D/:/-}"
  HEX="${D#sha256:}"
  if [ -f "$FILE" ] && [ "$(sha_of "$FILE")" = "$HEX" ]; then
    echo "已存在且校验通过: $D"
    continue
  fi
  # 上次可能已下载完整但没来得及改名:先校验半成品,避免续传时 416 报错
  if [ -f "$FILE.part" ] && [ "$(sha_of "$FILE.part")" = "$HEX" ]; then
    mv "$FILE.part" "$FILE"
    echo "半成品已完整且校验通过: $D"
    continue
  fi
  echo "下载 $D ..."
  # 下载到 .part 并断点续传,失败自动重试
  OK=0
  for i in 1 2 3 4 5 6 7 8 9 10; do
    if curl -fL --retry 5 --retry-delay 3 --retry-all-errors \
         --connect-timeout 15 --max-time 0 \
         --speed-time 60 --speed-limit 1024 \
         -C - --progress-bar -o "$FILE.part" "$REG/blobs/$D"; then OK=1; break; fi
    # 续传位置已是文件末尾时服务端会返回 416:此时若校验通过也视为成功
    if [ -f "$FILE.part" ] && [ "$(sha_of "$FILE.part")" = "$HEX" ]; then OK=1; break; fi
    echo "第 $i 次中断,2 秒后重试..."; sleep 2
  done
  if [ "$OK" != 1 ]; then echo "下载失败: $D" >&2; exit 1; fi
  GOT="$(sha_of "$FILE.part")"
  if [ "$GOT" != "$HEX" ]; then echo "校验失败: $D (实际 $GOT)" >&2; rm -f "$FILE.part"; exit 1; fi
  mv "$FILE.part" "$FILE"
done

# 所有 blob 就绪后才写 manifest,保证 Ollama 看到的模型总是完整的
printf '%s' "$MANIFEST" > "$MF_DIR/$TAG.tmp"
mv "$MF_DIR/$TAG.tmp" "$MF_DIR/$TAG"
echo "完成: ${NS}/${NAME}:${TAG}"
