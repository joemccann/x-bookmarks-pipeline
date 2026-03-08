#!/usr/bin/env bash
#
# service_ctl.sh — Install / uninstall / manage the x-bookmarks-pipeline launchd daemon
#
# Usage:
#   ./service_ctl.sh install     Copy plist to ~/Library/LaunchAgents/ and load it
#   ./service_ctl.sh uninstall   Unload and remove plist
#   ./service_ctl.sh start       Start the daemon
#   ./service_ctl.sh stop        Stop the daemon
#   ./service_ctl.sh restart     Stop then start
#   ./service_ctl.sh status      Show daemon status
#   ./service_ctl.sh logs        Tail the log file
#   ./service_ctl.sh logs-all    Tail all log files (app + stdout + stderr)
#

set -euo pipefail

LABEL="com.joemccann.x-bookmarks-pipeline"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLIST_SRC="${SCRIPT_DIR}/${LABEL}.plist"
PLIST_DST="${HOME}/Library/LaunchAgents/${LABEL}.plist"
LOG_DIR="${HOME}/.local/log"
LOG_FILE="${LOG_DIR}/x-bookmarks-pipeline.log"
STDOUT_LOG="${LOG_DIR}/x-bookmarks-pipeline.stdout.log"
STDERR_LOG="${LOG_DIR}/x-bookmarks-pipeline.stderr.log"

_green()  { printf '\033[0;32m%s\033[0m\n' "$*"; }
_yellow() { printf '\033[0;33m%s\033[0m\n' "$*"; }
_red()    { printf '\033[0;31m%s\033[0m\n' "$*"; }

_ensure_log_dir() {
    mkdir -p "${LOG_DIR}"
}

cmd_install() {
    _ensure_log_dir

    if [ ! -f "${PLIST_SRC}" ]; then
        _red "ERROR: Plist not found at ${PLIST_SRC}"
        exit 1
    fi

    # Create LaunchAgents directory if it doesn't exist
    mkdir -p "${HOME}/Library/LaunchAgents"

    # Copy plist
    cp "${PLIST_SRC}" "${PLIST_DST}"
    _green "Copied plist to ${PLIST_DST}"

    # Load the agent
    launchctl load "${PLIST_DST}" 2>/dev/null || true
    _green "Loaded ${LABEL}"

    cmd_status
}

cmd_uninstall() {
    # Unload first
    if launchctl list "${LABEL}" &>/dev/null; then
        launchctl unload "${PLIST_DST}" 2>/dev/null || true
        _yellow "Unloaded ${LABEL}"
    fi

    # Remove plist
    if [ -f "${PLIST_DST}" ]; then
        rm "${PLIST_DST}"
        _yellow "Removed ${PLIST_DST}"
    else
        _yellow "Plist not found at ${PLIST_DST} (already removed?)"
    fi

    _green "Uninstalled ${LABEL}"
}

cmd_start() {
    _ensure_log_dir

    if ! [ -f "${PLIST_DST}" ]; then
        _red "ERROR: Not installed. Run './service_ctl.sh install' first."
        exit 1
    fi

    launchctl start "${LABEL}"
    _green "Started ${LABEL}"
    sleep 1
    cmd_status
}

cmd_stop() {
    if launchctl list "${LABEL}" &>/dev/null; then
        launchctl stop "${LABEL}"
        _green "Stopped ${LABEL}"
    else
        _yellow "${LABEL} is not running"
    fi
}

cmd_restart() {
    cmd_stop
    sleep 1
    cmd_start
}

cmd_status() {
    echo "=== ${LABEL} ==="
    echo ""

    if launchctl list "${LABEL}" &>/dev/null; then
        launchctl list "${LABEL}"
        echo ""
        _green "Status: LOADED"
    else
        _yellow "Status: NOT LOADED"
    fi

    echo ""

    # Check for PID
    local pid
    pid=$(launchctl list "${LABEL}" 2>/dev/null | grep -oE '"PID"\s*=\s*[0-9]+' | grep -oE '[0-9]+' || true)
    if [ -n "${pid}" ]; then
        _green "PID: ${pid}"
    fi

    # Check plist
    if [ -f "${PLIST_DST}" ]; then
        echo "Plist: ${PLIST_DST}"
    else
        _yellow "Plist: not installed"
    fi

    # Check log file
    if [ -f "${LOG_FILE}" ]; then
        local size
        size=$(du -h "${LOG_FILE}" | cut -f1)
        local last
        last=$(tail -1 "${LOG_FILE}" 2>/dev/null || echo "(empty)")
        echo "Log:   ${LOG_FILE} (${size})"
        echo "Last:  ${last}"
    else
        echo "Log:   ${LOG_FILE} (not created yet)"
    fi
}

cmd_logs() {
    _ensure_log_dir
    if [ ! -f "${LOG_FILE}" ]; then
        _yellow "Log file does not exist yet: ${LOG_FILE}"
        _yellow "Waiting for first output..."
    fi
    tail -f "${LOG_FILE}"
}

cmd_logs_all() {
    _ensure_log_dir
    echo "Tailing: ${LOG_FILE}"
    echo "         ${STDOUT_LOG}"
    echo "         ${STDERR_LOG}"
    echo ""
    tail -f "${LOG_FILE}" "${STDOUT_LOG}" "${STDERR_LOG}" 2>/dev/null
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

case "${1:-}" in
    install)    cmd_install ;;
    uninstall)  cmd_uninstall ;;
    start)      cmd_start ;;
    stop)       cmd_stop ;;
    restart)    cmd_restart ;;
    status)     cmd_status ;;
    logs)       cmd_logs ;;
    logs-all)   cmd_logs_all ;;
    *)
        echo "Usage: $0 {install|uninstall|start|stop|restart|status|logs|logs-all}"
        exit 1
        ;;
esac
