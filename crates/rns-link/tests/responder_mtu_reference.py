"""Check Rust LRPROOF signing/MTU/MDU using read-only Python RNS primitives."""
import sys
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
import RNS

for line in sys.stdin:
    request, proof, destination, public, mtu, mdu = line.split()
    request, proof = bytes.fromhex(request), bytes.fromhex(proof)
    destination, public = bytes.fromhex(destination), bytes.fromhex(public)
    packet = RNS.Packet(None, b"\x02\x00" + destination + b"\x00" + request)
    assert packet.unpack()
    offered = RNS.Link.mtu_from_lr_packet(packet) or RNS.Reticulum.MTU
    # Rust's current local implementation still caps links at the base MTU;
    # this oracle does not claim interface MTU upgrade support.
    effective = min(offered, RNS.Reticulum.MTU)
    assert int(mtu) == effective
    assert RNS.Link.mtu_from_lp_packet(SimpleNamespace(data=proof)) == effective
    mode = RNS.Link.mode_from_lr_packet(packet)
    signalling = RNS.Link.signalling_bytes(effective, mode)
    assert proof[96:] == signalling
    link_id = RNS.Link.link_id_from_lr_packet(packet)
    signed = link_id + proof[64:96] + public + signalling
    RNS.Cryptography.Ed25519PublicKey.from_public_bytes(public).verify(proof[:64], signed)
    link = SimpleNamespace(mtu=effective)
    RNS.Link.update_mdu(link)
    # Negative Python MDU means no payload capacity. Rust represents it as 0.
    assert int(mdu) == max(0, link.mdu)
