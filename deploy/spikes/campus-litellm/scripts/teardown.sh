#!/bin/sh
# Removes every workload, service, configmap, secret and policy but keeps the namespace for review.
set -eu
NS=campus-litellm-spike
kubectl -n "$NS" delete deployment,job,pod,service,configmap,secret,networkpolicy -l pharness.dev/purpose=luc-115-litellm-spike --ignore-not-found
kubectl -n "$NS" delete deployment --all --ignore-not-found
kubectl -n "$NS" get all
