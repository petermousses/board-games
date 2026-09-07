#!/usr/bin/env bash
set -euo pipefail

namespace="board-games"
migration_job="board-games-migrate"
migration_wait_timeout="21m"
# Job activeDeadlineSeconds (1200) plus its 30s termination grace period and 30s of observation slack.
migration_wait_seconds=1260
migration_poll_seconds=5
rollout_wait_timeout="11m"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

current_epoch_seconds() {
  date +%s
}

wait_for_migration() {
  local deadline=$(( $(current_epoch_seconds) + migration_wait_seconds ))
  local conditions
  local remaining_seconds
  local poll_seconds

  while :; do
    conditions="$(kubectl -n "$namespace" get job "$migration_job" -o jsonpath='{range .status.conditions[*]}{.type}={.status}{"\n"}{end}')"

    if [[ "$conditions" == *"Failed=True"* || "$conditions" == *"FailureTarget=True"* ]]; then
      printf 'migration Job %s failed\n' "$migration_job" >&2
      return 1
    fi

    if [[ "$conditions" == *"Complete=True"* ]]; then
      return 0
    fi

    remaining_seconds=$((deadline - $(current_epoch_seconds)))
    if (( remaining_seconds <= 0 )); then
      printf 'migration Job %s did not complete within %s\n' "$migration_job" "$migration_wait_timeout" >&2
      return 1
    fi

    poll_seconds="$migration_poll_seconds"
    if (( remaining_seconds < poll_seconds )); then
      poll_seconds="$remaining_seconds"
    fi

    kubectl -n "$namespace" wait --for=condition=Complete "job/$migration_job" --timeout="${poll_seconds}s" >/dev/null 2>&1 || true
  done
}

api_restore_patch=""
api_restore_armed=false
api_previously_paused=false

restore_api_rollout() {
  local failure_status=$?

  trap - EXIT

  if [[ "$api_restore_armed" == true ]]; then
    printf 'migration did not complete; restoring the prior API Deployment spec\n' >&2

    if ! kubectl -n "$namespace" patch deployment/board-games-api --type=json --patch="$api_restore_patch"; then
      printf 'failed to restore the prior API Deployment spec\n' >&2
    elif [[ "$api_previously_paused" != "true" ]]; then
      if ! kubectl -n "$namespace" rollout status deployment/board-games-api --timeout="$rollout_wait_timeout"; then
        printf 'restored API Deployment did not become available\n' >&2
      fi
    fi
  fi

  exit "$failure_status"
}

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
    kubectl -n "$namespace" wait --for=delete "job/$migration_job" --timeout="$migration_wait_timeout"
  else
    printf 'refusing deployment: prior migration Job %s is not Complete\n' "$migration_job" >&2
    exit 1
  fi
fi

existing_api_deployment="$(kubectl -n "$namespace" get deployment board-games-api --ignore-not-found -o name)"
if [[ -n "$existing_api_deployment" ]]; then
  if ! command -v jq >/dev/null; then
    printf 'jq is required to restore the API Deployment after a migration failure\n' >&2
    exit 1
  fi

  api_deployment_json="$(kubectl -n "$namespace" get deployment board-games-api -o json)"
  api_restore_spec="$(jq -ce '.spec | select(type == "object")' <<<"$api_deployment_json")"
  api_restore_patch="$(jq -cn --argjson spec "$api_restore_spec" '[{op: "replace", path: "/spec", value: $spec}]')"
  api_previously_paused="$(jq -er 'if .spec.paused == true then "true" else "false" end' <<<"$api_deployment_json")"

  api_restore_armed=true
  trap restore_api_rollout EXIT
  kubectl -n "$namespace" rollout pause deployment/board-games-api
fi

kubectl apply -k "$repo_root/deploy/k8s"
wait_for_migration
trap - EXIT

if [[ "$api_previously_paused" != "true" ]]; then
  kubectl -n "$namespace" rollout resume deployment/board-games-api
  kubectl -n "$namespace" rollout status deployment/board-games-api --timeout="$rollout_wait_timeout"
else
  printf 'API Deployment was already paused; leaving it paused after migration\n' >&2
fi

kubectl -n "$namespace" rollout status deployment/board-games-web --timeout="$rollout_wait_timeout"
