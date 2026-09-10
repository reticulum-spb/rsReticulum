"""Python discovery contracts for S12. Fixed stamp=32 zero bytes, time omitted.

Uses the reference suite's announcer setup and real Python discovery receiver.
Only packed maps and stable receiver outputs enter the generated fixture.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('--suite', type=Path, default=root.parent / 'reticulum-e2e-tests')
p.add_argument('--output', type=Path, default=root / 'crates/rns-transport/tests/fixtures/discovery-reference.json')
p.add_argument('--check', action='store_true')
a = p.parse_args()
spec = importlib.util.spec_from_file_location('reference_vectors', a.suite / 'vectors/rust/generate_reference_vectors.py')
ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ref)
RNS = ref.RNS
section = ref.discovery_section()
base = RNS.vendor.umsgpack.unpackb(bytes.fromhex(section['cases'][0]['packed']))
handler = RNS.Discovery.InterfaceAnnounceHandler(required_value=0)
identity = ref.Identity.from_bytes(ref.PEER_PRIVATE_KEY)
destination_hash = bytes.fromhex(section['destination_hash'])
rows = []

def check(name, values, accepted):
    packed = RNS.vendor.umsgpack.packb(values)
    received = []
    handler.callback = received.append
    # Zero cost isolates map decoding; stamp validation still uses the real stamper.
    handler.received_announce(destination_hash, identity, b'\0' + packed + bytes(32))
    assert bool(received) == accepted, (name, received)
    row = {'case': name, 'packed': packed.hex(), 'accepted': accepted}
    if received:
        row.update({key: received[0][key] for key in ['name', 'latitude', 'longitude', 'height']})
        row['operator_address'] = received[0].get('operator_lxmf_address')
    rows.append(row)

for case in section['cases']:
    check(case['name'], RNS.vendor.umsgpack.unpackb(bytes.fromhex(case['packed'])), True)
for field, key in [('name', RNS.Discovery.NAME), ('latitude', RNS.Discovery.LATITUDE), ('longitude', RNS.Discovery.LONGITUDE), ('height', RNS.Discovery.HEIGHT)]:
    missing = dict(base)
    del missing[key]
    check(field + '-missing', missing, False)
    check(field + '-nil', dict(base, **{}) | {key: None}, True)
    check(field + '-valid', base | {key: 'Configured' if field == 'name' else 12.5}, True)
    check(field + '-wrong-type', base | {key: 123}, False)
# Python's name sanitizer maps false values to its fallback before calling encode.
for value in ['', False, 0]:
    check('name-false-' + repr(value), base | {RNS.Discovery.NAME: value}, True)
for size in [0, 15, 16, 17]:
    check('operator-length-' + str(size), base | {RNS.Discovery.OP_ADDR: bytes(range(size))}, True)
check('operator-nil', base | {RNS.Discovery.OP_ADDR: None}, True)
for value in ['0' * 16, 0, False]:
    check('operator-wrong-type-' + repr(value), base | {RNS.Discovery.OP_ADDR: value}, False)
text = json.dumps(rows, indent=2) + '\n'
if a.check:
    assert a.output.read_text() == text, 'regenerate the discovery fixture'
else:
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(text)
print(f'{len(rows)} Python discovery contracts passed')
