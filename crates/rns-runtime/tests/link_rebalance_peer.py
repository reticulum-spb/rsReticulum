"""Isolated Python 1.5.2 Link peer; packets travel over stdin/stdout, no sockets.

Uses the real Python announce, Link ID, ECDH, proof signing and encryption.
Hop bytes simulate a shorter proof path and are not part of signatures.
"""
import json
import sys
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
import RNS
from RNS.vendor import umsgpack

RNS.loglevel = RNS.LOG_NONE
RNS.Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False)
identity = RNS.Identity()
destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE,
                              "test", "rebalance")
announce = destination.announce(send=False)
announce.pack()
print(json.dumps({"public_key": identity.get_public_key().hex(),
                  "destination": destination.hash.hex(), "announce": announce.raw.hex()}), flush=True)

def receive():
    packet = RNS.Packet(None, bytes.fromhex(input()))
    assert packet.unpack()
    return packet

request = receive()
assert request.packet_type == RNS.Packet.LINKREQUEST
link = RNS.Link(owner=destination, peer_pub_bytes=request.data[:32],
                peer_sig_pub_bytes=request.data[32:64])
link.set_link_id(request)
link.mode = RNS.Link.mode_from_lr_packet(request)
link.mtu = RNS.Link.mtu_from_lr_packet(request)
link.handshake()
signalling = RNS.Link.signalling_bytes(link.mtu, link.mode)
if sys.argv[2] == "legacy":
    signalling = b""
signature = identity.sign(link.link_id + link.pub_bytes + link.sig_pub_bytes + signalling)
proof = RNS.Packet(link, signature + link.pub_bytes + signalling,
                   packet_type=RNS.Packet.PROOF, context=RNS.Packet.LRPROOF)
proof.hops = 1
proof.pack()
print(json.dumps({"proof": proof.raw.hex()}), flush=True)

rtt = receive()
assert rtt.context == RNS.Packet.LRRTT
assert isinstance(umsgpack.unpackb(link.decrypt(rtt.data)), float)
link.status = RNS.Link.ACTIVE
print(json.dumps({"rtt_ok": True}), flush=True)

data = receive()
assert link.decrypt(data.data) == b"still on the established interface"
reply = RNS.Packet(link, b"python reply after gravity change")
reply.pack()
print(json.dumps({"reply": reply.raw.hex()}), flush=True)
