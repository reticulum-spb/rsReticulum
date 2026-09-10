"""NET-1: real requests cross the Python/Rust boundary in both directions.

The packet boundary uses Python's MDU and msgpack encoder. Timestamp is fixed
at 1700000000.0 for calculating framing overhead; runtime timestamps are f64 too.
Run in the prepared parity Docker stack alongside the endpoint suite.
"""
import hashlib
import pytest
import RNS
from RNS.vendor import umsgpack
from test_rsreticulum_interop import command, endpoints, expand_payload


def packet_body_limit():
    path_hash = RNS.Identity.truncated_hash(b'/echo')
    def packed_length(size):
        return len(umsgpack.packb([1700000000.0, path_hash, b'a' * size]))
    size = RNS.Link.MDU
    while packed_length(size) > RNS.Link.MDU:
        size -= 1
    assert packed_length(size) == RNS.Link.MDU
    assert packed_length(size + 1) == RNS.Link.MDU + 1
    return size


@pytest.mark.parametrize(('sender', 'destination_key'), [
    ('rust-rns-client', 'python_link'), ('python-rns-client', 'rust_link'),
])
@pytest.mark.parametrize('size', [packet_body_limit(), packet_body_limit() + 1, 100_000])
def test_resource_request(endpoints, sender, destination_key, size):
    seed = 'resource-request-fixed-seed'
    content = expand_payload(seed, size)
    expected = ('echo:' + content).encode()
    response = command(sender, {
        'type': 'link_request', 'destination': endpoints[destination_key],
        'payload_seed': seed, 'payload_size': size,
    }, timeout=100)
    assert response['success'], response
    assert response['response_length'] == len(expected)
    assert response['response_sha256'] == hashlib.sha256(expected).hexdigest()
