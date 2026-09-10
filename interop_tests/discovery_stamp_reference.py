"""S9: compare Rust settings to real Python discovery stamp acceptance.

Stamp candidates are deterministic 32-byte big-endian counters starting at zero.
Receive timestamps and freshly mined announcer stamps are not compared.
"""
import importlib.util
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
suite = root.parent / 'reticulum-e2e-tests'
spec = importlib.util.spec_from_file_location('reference_vectors', suite / 'vectors/rust/generate_reference_vectors.py')
ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ref)
RNS = ref.RNS
section = ref.discovery_section()
packed = bytes.fromhex(section['cases'][0]['packed'])
identity = ref.Identity.from_bytes(ref.PEER_PRIVATE_KEY)
destination = bytes.fromhex(section['destination_hash'])
reference_minimum = RNS.Discovery.InterfaceAnnouncer.DEFAULT_STAMP_VALUE
handler = RNS.Discovery.InterfaceAnnounceHandler(required_value=reference_minimum)
workblock = handler.stamper.stamp_workblock(RNS.Identity.full_hash(packed), expand_rounds=RNS.Discovery.InterfaceAnnouncer.WORKBLOCK_EXPAND_ROUNDS)
stamps = {}
for counter in range(10_000_000):
    stamp = counter.to_bytes(32, 'big')
    value = handler.stamper.stamp_value(workblock, stamp)
    if value in [reference_minimum - 1, reference_minimum]:
        stamps.setdefault(value, stamp)
    if len(stamps) == 2:
        break
assert len(stamps) == 2, 'deterministic stamp search exhausted'

def accepts(required, value):
    received = []
    receiver = RNS.Discovery.InterfaceAnnounceHandler(required_value=required, callback=received.append)
    receiver.received_announce(destination, identity, b'\0' + packed + stamps[value])
    return bool(received)

# Establish the Python boundary before reading or comparing the Rust defaults.
assert not accepts(reference_minimum, reference_minimum - 1)
assert accepts(reference_minimum, reference_minimum)
assert accepts(reference_minimum - 1, reference_minimum - 1)
settings = json.load(sys.stdin)
assert settings['producer'] == reference_minimum, settings
assert settings['receiver'] == reference_minimum, settings
assert settings['explicit_producer'] == reference_minimum - 1, settings
assert settings['explicit_receiver'] == reference_minimum - 1, settings
for value in stamps:
    assert accepts(settings['receiver'], value) == accepts(reference_minimum, value)
assert accepts(settings['explicit_receiver'], reference_minimum - 1)
print(json.dumps({'python_minimum': reference_minimum, 'actual_stamp_values': sorted(stamps), 'rust_settings': settings}))
