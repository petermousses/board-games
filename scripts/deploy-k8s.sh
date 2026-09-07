#!/usr/bin/env bash
set -euo pipefail

namespace="board-games"
migration_job="board-games-migrate"
wait_timeout="11m"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

existing_job="$(kubectl -n "$namespace" get job "$migration_job" --ignore-not-found -o name)"
if [[ -n "$existing_job" ]]; then
  completed="$(kubectl -n "$namespace" get job "$migration_job" -o jsonpath='{.status.conditions[?(@.type=="Complete")].status}')"
  active="$(kubectl -n "$namespace" get job "$migration_job" -o jsonpath='{.status.active}')"
  failed="$(kubectl -n "$namespace" get job "$migration_job" -o jsonpath='{.status.conditions[?(@.type=="Failed")].status}')"

  if [[ "$active" != "" && "$active" != "0" ]] || [[ "$failed" == *"True"* ]]; then
    printf 'refusing deployment: prior migration Job %s is active or failed\n' "$migration_job" >&2
    exit 1
  elif [[ "$completed" == *"True"* ]]; then
    kubectl -n "$namespace" delete job "$migration_job" --wait=false
    kubectl -n "$namespace" wait --for=delete "job/$migration_job" --timeout="$wait_timeout"
  else
    printf 'refusing deployment: prior migration Job %s is not Complete\n' "$migration_job" >&2
    exit 1
  fi
fi

existing_api_deployment="$(kubectl -n "$namespace" get deployment board-games-api --ignore-not-found -o name)"
if [[ -n "$existing_api_deployment" ]]; then
  kubectl -n "$namespace" rollout pause deployment/board-games-api
fi

kubectl apply -k "$repo_root/deploy/k8s"
kubectl -n "$namespace" wait --for=condition=Complete "job/$migration_job" --timeout="$wait_timeout"
kubectl -n "$namespace" rollout resume deployment/board-games-api
kubectl -n "$namespace" rollout status deployment/board-games-api --timeout="$wait_timeout"
kubectl -n "$namespace" rollout status deployment/board-games-web --timeout="$wait_timeout"
