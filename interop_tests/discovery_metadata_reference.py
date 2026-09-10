"""S10: the real Python receiver consumes maps emitted by the Rust codec.

Stamp is fixed to 32 zero bytes with explicit cost zero to isolate metadata.
"""
import importlib.util
import json
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('reference_vectors', root.parent / 'reticulum-e2e-tests/vectors/rust/generate_reference_vectors.py')
ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ref)
RNS = ref.RNS
section = ref.discovery_section()
identity = ref.Identity.from_bytes(ref.PEER_PRIVATE_KEY)
destination = bytes.fromhex(section['destination_hash'])
request = json.load(sys.stdin)
for case in request['cases']:
    packed = bytes.fromhex(case['packed'])
    info = RNS.vendor.umsgpack.unpackb(packed)
    assert info[RNS.Discovery.TRANSPORT_IMPL] == 'rsReticulum', info
    assert info[RNS.Discovery.TRANSPORT_VERS] == request['version'], info
    received = []
    handler = RNS.Discovery.InterfaceAnnounceHandler(required_value=0, callback=received.append)
    handler.received_announce(destination, identity, b'\0' + packed + bytes(32))
    assert len(received) == 1, case
    assert received[0].get('operator_lxmf_address') == case['operator_address'], case
print(f"{len(request['cases'])} Rust discovery maps accepted by Python with matching operator addresses")
