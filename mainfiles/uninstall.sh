#!/system/bin/sh

#
# Copyright (C) 2026-2027 Zexshia
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#

q() {
    pkill -TERM -f "$1" 2>/dev/null
}

q9() {
    pkill -9 -f "$1" 2>/dev/null
}

# Stop the daemons BEFORE deleting any state: a still-running daemon
# recreates /data/adb/.config/AZenith (current_profile, gameinfo,
# daemon_state) and would resurrect files after the rm -rf below.
# TERM first so the daemon can restore the balanced profile on exit;
# -9 is only a fallback for a daemon that did not exit in time.
q sys.azenith-rianixiathermalcore
q sys.azenith-service
q sys.azenith-appmonitoring
q sys.azenith-fpsclient
sleep 1
q9 sys.azenith-rianixiathermalcore
q9 sys.azenith-service
q9 sys.azenith-appmonitoring
q9 sys.azenith-fpsclient

resetprop | awk -F'[][]' '/persist\.sys\.azenith/ {print $2}' | while read -r prop; do
    resetprop -p --delete "$prop"
done

for prop in persist.sys.rynx.learning_enabled persist.sys.rynx.thermalcore-bigdata.path; do
    resetprop -p --delete "$prop"
done

rm -rf \
    "/data/adb/.config/AZenith" \
    "/data/AZenith" \
    "/data/data/zx.azenith"
: > "/data/adb/modules/AZenith/remove"

pm uninstall zx.azenith >/dev/null 2>&1 &

for dir in "/data/adb/ap/bin" "/data/adb/ksu/bin"; do
    [ -d "$dir/zx" ] && rm -rf "$dir/zx"
done

for dir in "/data/adb/ap/bin" "/data/adb/ksu/bin"; do
    [ -d "$dir" ] && find "$dir" -name "sys.azenith-*" -exec rm -f {} +
done

