#!/usr/bin/env bash
# Set up and start a local Minecraft Java server for bot runs (spec §22):
# the pinned version, offline mode, RCON on localhost for scripts.
#
#   ACCEPT_EULA=1 scripts/mc-server.sh [DIR=runs/server] [DIFFICULTY=normal]
#
# MC_PORT (25565), RCON_PORT (25575) and RCON_PASSWORD (localtest) override
# the defaults. DIFFICULTY is peaceful, easy, normal or hard. Teach skills on peaceful or
# easy first; keep normal nights as the survival check (Michael, 14 Nov
# 2026). A running server changes with: scripts/rcon.py "difficulty easy".
#
# Running the server means accepting Mojang's EULA
# (https://aka.ms/MinecraftEULA). The script writes eula=true only when
# ACCEPT_EULA=1 is set. Needs Java 21, curl, python3; downloads the jar from
# piston-meta/piston-data.mojang.com and checks its sha1.
set -euo pipefail

version=1.21.11
dir=${1:-runs/server}
difficulty=${2:-normal}
case $difficulty in peaceful | easy | normal | hard) ;; *)
  echo "DIFFICULTY must be peaceful, easy, normal or hard" >&2
  exit 2
  ;;
esac
if [[ ${ACCEPT_EULA:-} != 1 ]]; then
  echo "Set ACCEPT_EULA=1 to accept Mojang's EULA (https://aka.ms/MinecraftEULA)." >&2
  exit 2
fi
mkdir -p "$dir"
cd "$dir"

if [[ ! -f server.jar ]]; then
  manifest=https://piston-meta.mojang.com/mc/game/version_manifest_v2.json
  meta=$(curl -fsS "$manifest" | python3 -I -c "
import json, sys
m = json.load(sys.stdin)
print(next(v['url'] for v in m['versions'] if v['id'] == '$version'))")
  read -r url sha1 < <(curl -fsS "$meta" | python3 -I -c "
import json, sys
s = json.load(sys.stdin)['downloads']['server']
print(s['url'], s['sha1'])")
  curl -fsS -o server.jar.part "$url"
  echo "$sha1  server.jar.part" | sha1sum -c --quiet
  mv server.jar.part server.jar
fi

echo "eula=true" >eula.txt
cat >server.properties <<EOF
server-port=${MC_PORT:-25565}
online-mode=false
difficulty=$difficulty
gamemode=survival
spawn-protection=0
max-players=10
view-distance=6
simulation-distance=6
motd=curiosity test ($difficulty)
enable-rcon=true
rcon.port=${RCON_PORT:-25575}
rcon.password=${RCON_PASSWORD:-localtest}
EOF
echo "starting Minecraft $version ($difficulty) in $dir; stop with Ctrl-C"
exec java -Xmx3G -jar server.jar nogui
