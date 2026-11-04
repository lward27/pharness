#!/bin/sh
# Generates spike credentials in-cluster. Values are piped straight into kubectl and never echoed.
set -eu
NS=campus-litellm-spike
if kubectl -n "$NS" get secret spike-litellm >/dev/null 2>&1; then
  echo "secrets already exist in $NS"
  exit 0
fi
PG_PASSWORD="$(openssl rand -hex 24)"
kubectl -n "$NS" create secret generic spike-postgres \
  --from-literal=password="$PG_PASSWORD" \
  --from-literal=database-url="postgresql://litellm:$PG_PASSWORD@postgres:5432/litellm" >/dev/null
unset PG_PASSWORD
kubectl -n "$NS" create secret generic spike-redis --from-literal=password="$(openssl rand -hex 24)" >/dev/null
kubectl -n "$NS" create secret generic spike-litellm --from-literal=master-key="sk-$(openssl rand -hex 24)" >/dev/null
kubectl -n "$NS" label secret spike-postgres spike-redis spike-litellm pharness.dev/purpose=luc-115-litellm-spike >/dev/null
echo "created spike-postgres, spike-redis, spike-litellm"
