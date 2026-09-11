"""Decode a live Rust announce using the local Python 1.5.2 receiver.

Invoked by the ignored Rust discovery_python_receiver test with Python 3.11+.
Does not write to either reference repository or connect to external peers.
"""
import json
import os
import sys
import tempfile
from pathlib import Path

sys.path[:0] = [
    os.environ.get("RNS_REFERENCE", "/home/room/src/Reticulum"),
    os.environ.get("LXMF_REFERENCE", "/home/room/src/LXMF"),
]
import RNS
from RNS.Discovery import InterfaceAnnounceHandler

raw = bytes.fromhex(sys.stdin.read().strip())
with tempfile.TemporaryDirectory(prefix="rns-discovery-python-") as directory:
    identity_path = sys.argv[1] if len(sys.argv) > 1 else ""
    Path(directory, "config").write_text(
        "[reticulum]\nshare_instance = no\nenable_transport = no\n"
        + (f"network_identity = {identity_path}\n" if identity_path else "")
        +
        "[logging]\nloglevel = 0\n[interfaces]\n"
    )
    runtime = RNS.Reticulum(configdir=directory)
    packet = RNS.Packet(None, raw)
    assert packet.unpack(), "Python could not unpack the Rust packet"
    assert packet.packet_type == RNS.Packet.ANNOUNCE
    assert RNS.Identity.validate_announce(packet), "invalid announce signature"
    identity = RNS.Identity.recall(packet.destination_hash)
    app_data = RNS.Identity.recall_app_data(packet.destination_hash)
    assert bool(app_data[0] & InterfaceAnnounceHandler.FLAG_ENCRYPTED) == bool(identity_path)
    received = []
    receiver = InterfaceAnnounceHandler(required_value=8, callback=received.append)
    receiver.received_announce(packet.destination_hash, identity, app_data)
    assert len(received) == 1, "Python rejected the Rust discovery announce"
    info = received[0]
    assert info["name"] == "Interop relay", info
    assert info["reachable_on"] == "127.0.0.1", info
    assert info["latitude"] == 55.75 and info["longitude"] == 37.6, info
    assert info["height"] == 150.0, info
    assert info["operator_lxmf_address"] == "0123456789abcdef0123456789abcdef", info
    assert info["value"] >= 8, info
    assert info["ifac_netname"] == "public-net", info
    assert info["ifac_netkey"] == "public-passphrase", info
    print(json.dumps({"accepted": True, "stamp_value": info["value"], "name": info["name"]}))
