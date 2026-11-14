#!/usr/bin/env python3
"""Send commands to the local Minecraft server over RCON (spec §22).

    scripts/rcon.py "time set 12000" "difficulty easy" "time query daytime"

Host, port and password come from RCON_HOST (127.0.0.1), RCON_PORT (25575)
and RCON_PASSWORD (localtest), as scripts/mc-server.sh sets them.
"""

import os
import socket
import struct
import sys

LOGIN, COMMAND = 3, 2


def packet(req_id: int, kind: int, body: str) -> bytes:
    data = struct.pack("<ii", req_id, kind) + body.encode() + b"\0\0"
    return struct.pack("<i", len(data)) + data


def read(sock: socket.socket) -> tuple[int, str]:
    def exactly(n: int) -> bytes:
        buf = b""
        while len(buf) < n:
            chunk = sock.recv(n - len(buf))
            if not chunk:
                raise ConnectionError("server closed the connection")
            buf += chunk
        return buf

    (size,) = struct.unpack("<i", exactly(4))
    data = exactly(size)
    (req_id,) = struct.unpack("<i", data[:4])
    return req_id, data[8:-2].decode()


def main() -> int:
    host = os.environ.get("RCON_HOST", "127.0.0.1")
    port = int(os.environ.get("RCON_PORT", "25575"))
    password = os.environ.get("RCON_PASSWORD", "localtest")
    with socket.create_connection((host, port), timeout=10) as sock:
        sock.sendall(packet(1, LOGIN, password))
        if read(sock)[0] == -1:
            print("RCON login refused", file=sys.stderr)
            return 1
        for cmd in sys.argv[1:]:
            sock.sendall(packet(2, COMMAND, cmd))
            print(f"{cmd} -> {read(sock)[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
