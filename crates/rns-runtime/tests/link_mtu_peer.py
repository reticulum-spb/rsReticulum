"""Python 1.5.2 Link initiator over loopback TCP, without a Reticulum daemon.

Only daemon services (routing, outbound socket and watchdog scheduling) are
stubbed. Link construction, proof validation, ECDH, MDU and packet encryption
are the actual reference implementations.
"""
import json
import socket
import sys
from collections import deque
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
import RNS
from RNS.Interfaces.TCPInterface import HDLC

RNS.loglevel = RNS.LOG_NONE
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(10)
    print(json.dumps({"port": listener.getsockname()[1]}), flush=True)
    connection, _ = listener.accept()

with connection:
    connection.settimeout(10)
    config = json.loads(input())
    offer, expected = config["offer"], config["expected"]
    identity = RNS.Identity(create_keys=False)
    identity.load_public_key(bytes.fromhex(config["public_key"]))
    destination = RNS.Destination(identity, RNS.Destination.OUT,
                                  RNS.Destination.SINGLE, "test", "mtu")
    assert destination.hash.hex() == config["destination"]
    RNS.Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False)
    RNS.Reticulum.get_instance = staticmethod(lambda: SimpleNamespace(get_first_hop_timeout=lambda _: 1))
    RNS.Reticulum.link_mtu_discovery = staticmethod(lambda: True)
    RNS.Transport.hops_to = staticmethod(lambda _: 1)
    RNS.Transport.next_hop_interface_hw_mtu = staticmethod(lambda _: offer)
    RNS.Transport.register_link = staticmethod(lambda _: None)
    RNS.Transport.activate_link = staticmethod(lambda _: None)
    RNS.Link.start_watchdog = lambda self: None

    def outbound(packet):
        if not packet.packed:
            packet.pack()
        wire = b"\x7e" + HDLC.escape(packet.raw) + b"\x7e"
        for offset in range(0, len(wire), 997):
            connection.sendall(wire[offset:offset+997])
        return True

    RNS.Transport.outbound = staticmethod(outbound)
    pending = deque()
    buffer = bytearray()

    def receive():
        while not pending:
            chunk = connection.recv(8192)
            assert chunk, "Rust TCP connection closed before reply"
            for byte in chunk:
                if byte == HDLC.FLAG:
                    if buffer:
                        # TCPInterface.read_loop uses these replacements inline.
                        raw = bytes(buffer).replace(bytes([HDLC.ESC, HDLC.FLAG ^ HDLC.ESC_MASK]), bytes([HDLC.FLAG]))
                        raw = raw.replace(bytes([HDLC.ESC, HDLC.ESC ^ HDLC.ESC_MASK]), bytes([HDLC.ESC]))
                        assert len(raw) <= expected
                        packet = RNS.Packet(None, raw)
                        assert packet.unpack()
                        pending.append(packet)
                        buffer.clear()
                else:
                    buffer.append(byte)
                    assert len(buffer) <= 2 * expected
        return pending.popleft()

    link = RNS.Link(destination)
    assert RNS.Link.mtu_from_lr_packet(link.packet) == offer
    while True:
        proof = receive()
        if proof.destination_hash == link.link_id and proof.context == RNS.Packet.LRPROOF:
            break
    assert proof.packet_type == RNS.Packet.PROOF
    assert RNS.Link.mtu_from_lp_packet(proof) == expected
    link.validate_proof(proof)  # Also sends the encrypted LRRTT packet.
    assert link.status == RNS.Link.ACTIVE, "Rust LRPROOF failed Python validation"
    assert link.mtu == expected
    print(json.dumps({"mtu": link.mtu, "mdu": link.mdu,
                      "link_id": link.link_id.hex()}), flush=True)
    for length in [1, link.mdu]:
        payload = bytes(index % 256 for index in range(length))
        packet = RNS.Packet(link, payload)
        packet.send()
        assert len(packet.raw) <= expected
        while True:
            reply = receive()
            if reply.destination_hash == link.link_id and reply.packet_type == RNS.Packet.DATA:
                break
        assert reply.context == RNS.Packet.NONE
        assert link.decrypt(reply.data) == payload
    print(json.dumps({"complete": True}), flush=True)
