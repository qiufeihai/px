#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage:" >&2
  echo "  $0 start <helper> <dns_helper> <socks> <device> <interface> <mtu> <loglevel> <tun_ipv4> <gateway> <server_ip> <log_path> <pid_path>" >&2
  echo "  $0 stop <device> <tun_ipv4> <gateway> <server_ip> <pid_path> <pid_hint>" >&2
  exit 1
}

is_running() {
  local pid="$1"
  [[ -n "$pid" ]] && ps -p "$pid" -o pid= >/dev/null 2>&1
}

delete_routes_best_effort() {
  local device_name="$1"
  local tun_ipv4="$2"
  local primary_gateway="$3"
  local server_ip="$4"

  route -n delete -host "$server_ip" "$primary_gateway" >/dev/null 2>&1 || true
  for cidr in \
    1.0.0.0/8 \
    2.0.0.0/7 \
    4.0.0.0/6 \
    8.0.0.0/5 \
    16.0.0.0/4 \
    32.0.0.0/3 \
    64.0.0.0/2 \
    128.0.0.0/1 \
    198.18.0.0/15; do
    route -n delete -net "$cidr" "$tun_ipv4" >/dev/null 2>&1 || true
  done
  ifconfig "$device_name" down >/dev/null 2>&1 || true
}

kill_pid_best_effort() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  if is_running "$pid"; then
    kill "$pid" >/dev/null 2>&1 || true
    for _ in {1..20}; do
      if ! is_running "$pid"; then
        break
      fi
      sleep 0.1
    done
    if is_running "$pid"; then
      kill -9 "$pid" >/dev/null 2>&1 || true
      for _ in {1..10}; do
        if ! is_running "$pid"; then
          break
        fi
        sleep 0.1
      done
    fi
  fi
  return 0
}

kill_matching_processes() {
  local pattern="$1"
  local pid=""
  while IFS= read -r pid; do
    [[ -n "$pid" ]] || continue
    kill_pid_best_effort "$pid"
  done < <(pgrep -f "$pattern" 2>/dev/null || true)
}

add_routes() {
  local device_name="$1"
  local tun_ipv4="$2"
  local primary_gateway="$3"
  local server_ip="$4"

  ifconfig "$device_name" "$tun_ipv4" "$tun_ipv4" up
  route -n add -host "$server_ip" "$primary_gateway"
  for cidr in \
    1.0.0.0/8 \
    2.0.0.0/7 \
    4.0.0.0/6 \
    8.0.0.0/5 \
    16.0.0.0/4 \
    32.0.0.0/3 \
    64.0.0.0/2 \
    128.0.0.0/1 \
    198.18.0.0/15; do
    route -n add -net "$cidr" "$tun_ipv4"
  done
}

resolve_network_service() {
  local primary_interface="$1"
  networksetup -listnetworkserviceorder | awk -v dev="$primary_interface" '
    /^\([0-9]+\)/ {
      service = $0
      sub(/^\([0-9]+\) /, "", service)
      sub(/^\*/, "", service)
      next
    }
    $0 ~ "Device: " dev "\\)" {
      print service
      exit
    }
  '
}

configure_dns() {
  local primary_interface="$1"
  local dns_state_path="$2"
  local helper_log_path="$3"
  local service

  service="$(resolve_network_service "$primary_interface")"
  if [[ -z "$service" ]]; then
    echo "[launcher] failed to resolve network service for interface $primary_interface" >>"$helper_log_path"
    return 1
  fi

  {
    printf '%s\n' "$service"
    networksetup -getdnsservers "$service" 2>&1 || true
  } >"$dns_state_path"

  networksetup -setdnsservers "$service" 127.0.0.1
  echo "[launcher] dns service $service -> 127.0.0.1" >>"$helper_log_path"
}

read_dns_state_lines() {
  local service="$1"
  local line=""
  local current_lines=()

  while IFS= read -r line || [[ -n "$line" ]]; do
    current_lines+=("$line")
  done < <(networksetup -getdnsservers "$service" 2>&1 || true)

  printf '%s\n' "${current_lines[@]}"
}

dns_is_automatic() {
  local service="$1"
  local line=""

  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == "There aren't any DNS Servers set on $service."* ]]; then
      return 0
    fi
  done < <(read_dns_state_lines "$service")

  return 1
}

dns_matches_servers() {
  local service="$1"
  shift
  local expected=("$@")
  local actual=()
  local line=""
  local index=0

  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -n "$line" ]] && actual+=("$line")
  done < <(read_dns_state_lines "$service")

  [[ ${#actual[@]} -eq ${#expected[@]} ]] || return 1
  for index in "${!expected[@]}"; do
    [[ "${actual[$index]}" == "${expected[$index]}" ]] || return 1
  done
  return 0
}

restore_dns() {
  local dns_state_path="$1"
  local helper_log_path="$2"
  local line=""

  [[ -f "$dns_state_path" ]] || return 0

  local state_lines=()
  while IFS= read -r line || [[ -n "$line" ]]; do
    state_lines+=("$line")
  done <"$dns_state_path"
  local service="${state_lines[0]:-}"
  if [[ -z "$service" ]]; then
    echo "[launcher] dns restore failed: missing service name in $dns_state_path" >>"$helper_log_path"
    return 1
  fi

  if [[ ${#state_lines[@]} -le 1 ]] || [[ "${state_lines[1]:-}" == "There aren't any DNS Servers set on $service."* ]]; then
    if ! networksetup -setdnsservers "$service" Empty >/dev/null 2>&1; then
      echo "[launcher] dns restore failed: unable to reset $service to automatic" >>"$helper_log_path"
      return 1
    fi
    if ! dns_is_automatic "$service"; then
      echo "[launcher] dns restore failed: $service is still not automatic" >>"$helper_log_path"
      return 1
    fi
    echo "[launcher] dns service $service restored to automatic" >>"$helper_log_path"
    rm -f "$dns_state_path"
    return 0
  fi

  local dns_servers=()
  for line in "${state_lines[@]:1}"; do
    [[ -n "$line" ]] && dns_servers+=("$line")
  done

  if [[ ${#dns_servers[@]} -eq 0 ]]; then
    if ! networksetup -setdnsservers "$service" Empty >/dev/null 2>&1; then
      echo "[launcher] dns restore failed: unable to reset $service to automatic" >>"$helper_log_path"
      return 1
    fi
    if ! dns_is_automatic "$service"; then
      echo "[launcher] dns restore failed: $service is still not automatic" >>"$helper_log_path"
      return 1
    fi
    echo "[launcher] dns service $service restored to automatic" >>"$helper_log_path"
  else
    if ! networksetup -setdnsservers "$service" "${dns_servers[@]}" >/dev/null 2>&1; then
      echo "[launcher] dns restore failed: unable to restore $service DNS servers" >>"$helper_log_path"
      return 1
    fi
    if ! dns_matches_servers "$service" "${dns_servers[@]}"; then
      echo "[launcher] dns restore failed: $service DNS servers did not match expected values" >>"$helper_log_path"
      return 1
    fi
    echo "[launcher] dns service $service restored" >>"$helper_log_path"
  fi

  rm -f "$dns_state_path"
}

start_cmd() {
  [[ "$#" -eq 12 ]] || usage
  local helper_path="$1"
  local dns_helper_path="$2"
  local socks_addr="$3"
  local device_name="$4"
  local primary_interface="$5"
  local mtu="$6"
  local log_level="$7"
  local tun_ipv4="$8"
  local primary_gateway="$9"
  local server_ip="${10}"
  local helper_log_path="${11}"
  local pid_path="${12}"
  local helper_pid=""
  local dns_helper_pid=""
  local state_dir=""
  local dns_pid_path=""
  local dns_state_path=""
  local existing_pid=""
  local existing_dns_pid=""
  local helper_name=""
  local dns_helper_name=""

  mkdir -p "$(dirname "$helper_log_path")"
  : > "$helper_log_path"
  state_dir="$(dirname "$pid_path")"
  dns_pid_path="$state_dir/dns-helper.pid"
  dns_state_path="$state_dir/dns-servers.txt"
  helper_name="$(basename "$helper_path")"
  dns_helper_name="$(basename "$dns_helper_path")"

  if [[ -f "$pid_path" ]]; then
    existing_pid="$(cat "$pid_path" 2>/dev/null || true)"
  fi
  if [[ -f "$dns_pid_path" ]]; then
    existing_dns_pid="$(cat "$dns_pid_path" 2>/dev/null || true)"
  fi

  cleanup_failed_start() {
    if [[ -n "$helper_pid" ]] && is_running "$helper_pid"; then
      kill "$helper_pid" >/dev/null 2>&1 || true
      wait "$helper_pid" 2>/dev/null || true
    fi
    if [[ -n "$dns_helper_pid" ]] && is_running "$dns_helper_pid"; then
      kill "$dns_helper_pid" >/dev/null 2>&1 || true
      wait "$dns_helper_pid" 2>/dev/null || true
    fi
    restore_dns "$dns_state_path" "$helper_log_path" || true
    rm -f "$dns_pid_path"
    delete_routes_best_effort "$device_name" "$tun_ipv4" "$primary_gateway" "$server_ip"
    rm -f "$pid_path"
  }

  echo "[launcher] start uid=$(id -u) user=$(id -un)" >>"$helper_log_path"
  if [[ -n "$existing_pid" ]] || [[ -n "$existing_dns_pid" ]]; then
    echo "[launcher] cleaning previous tun state pid=$existing_pid dns_pid=$existing_dns_pid" >>"$helper_log_path"
  fi
  kill_pid_best_effort "$existing_pid" || true
  kill_pid_best_effort "$existing_dns_pid" || true
  kill_matching_processes "$helper_name.*-device $device_name" || true
  kill_matching_processes "$dns_helper_name.*--listen 127.0.0.1:53" || true
  restore_dns "$dns_state_path" "$helper_log_path" || true
  delete_routes_best_effort "$device_name" "$tun_ipv4" "$primary_gateway" "$server_ip" || true
  rm -f "$dns_pid_path" "$pid_path"

  "$dns_helper_path" \
    --listen 127.0.0.1:53 \
    --socks "$socks_addr" \
    >>"$helper_log_path" 2>&1 < /dev/null &
  dns_helper_pid="$!"
  echo "[launcher] started dns helper pid=$dns_helper_pid" >>"$helper_log_path"

  sleep 1
  if ! is_running "$dns_helper_pid"; then
    echo "[launcher] dns helper exited before dns setup" >>"$helper_log_path"
    cleanup_failed_start
    exit 1
  fi

  if ! configure_dns "$primary_interface" "$dns_state_path" "$helper_log_path"; then
    echo "[launcher] dns setup failed" >>"$helper_log_path"
    cleanup_failed_start
    exit 1
  fi

  "$helper_path" \
    -device "$device_name" \
    -proxy "socks5://$socks_addr" \
    -interface "$primary_interface" \
    -mtu "$mtu" \
    -loglevel "$log_level" \
    >>"$helper_log_path" 2>&1 < /dev/null &
  helper_pid="$!"
  echo "[launcher] started tun2socks pid=$helper_pid" >>"$helper_log_path"

  sleep 1
  if ! is_running "$helper_pid"; then
    echo "[launcher] tun2socks exited before route setup" >>"$helper_log_path"
    cleanup_failed_start
    exit 1
  fi

  if ! add_routes "$device_name" "$tun_ipv4" "$primary_gateway" "$server_ip" >>"$helper_log_path" 2>&1; then
    echo "[launcher] route setup failed" >>"$helper_log_path"
    cleanup_failed_start
    exit 1
  fi

  echo "$dns_helper_pid" > "$dns_pid_path"
  echo "$helper_pid" > "$pid_path"
  echo "[launcher] routes installed" >>"$helper_log_path"
  printf '%s' "$helper_pid"
}

stop_cmd() {
  [[ "$#" -eq 6 ]] || usage
  local device_name="$1"
  local tun_ipv4="$2"
  local primary_gateway="$3"
  local server_ip="$4"
  local pid_path="$5"
  local pid_hint="$6"
  local pid=""
  local state_dir=""
  local dns_pid_path=""
  local dns_state_path=""
  local dns_pid=""
  local helper_log_path=""
  local helper_name="tun2socks"
  local dns_helper_name="px-dns-helper"
  local restore_status=0

  state_dir="$(dirname "$pid_path")"
  dns_pid_path="$state_dir/dns-helper.pid"
  dns_state_path="$state_dir/dns-servers.txt"
  helper_log_path="$state_dir/tun-helper.log"

  if [[ -f "$pid_path" ]]; then
    pid="$(cat "$pid_path" 2>/dev/null || true)"
  fi
  if [[ -z "$pid" ]]; then
    pid="$pid_hint"
  fi

  if is_running "$pid"; then
    kill_pid_best_effort "$pid"
  fi

  if [[ -f "$dns_pid_path" ]]; then
    dns_pid="$(cat "$dns_pid_path" 2>/dev/null || true)"
  fi
  kill_pid_best_effort "$dns_pid"

  if ! restore_dns "$dns_state_path" "$helper_log_path"; then
    restore_status=1
  fi
  delete_routes_best_effort "$device_name" "$tun_ipv4" "$primary_gateway" "$server_ip"
  kill_matching_processes "$helper_name.*-device $device_name"
  kill_matching_processes "$dns_helper_name.*--listen 127.0.0.1:53"
  rm -f "$dns_pid_path"
  rm -f "$pid_path"
  return "$restore_status"
}

main() {
  local cmd="${1:-}"
  shift || true

  case "$cmd" in
    start)
      start_cmd "$@"
      ;;
    stop)
      stop_cmd "$@"
      ;;
    *)
      usage
      ;;
  esac
}

main "$@"
