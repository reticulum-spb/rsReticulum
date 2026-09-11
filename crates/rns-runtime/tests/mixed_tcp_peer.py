"""Isolated RNS packet producer over real HDLC/TCP, without a Reticulum daemon."""
import json
import socket
import sys
import threading
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
import RNS
from RNS.Interfaces.TCPInterface import HDLC

RNS.loglevel = RNS.LOG_NONE
RNS.Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False)
identity = RNS.Identity()
ann_dest = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, "test", "mixed", "announce")
data_dest = RNS.Destination(None, RNS.Destination.OUT, RNS.Destination.PLAIN, "test", "mixed", "data")
pr_dest = RNS.Destination(None, RNS.Destination.OUT, RNS.Destination.PLAIN, "rnstransport", "path", "request")
announce = ann_dest.announce(send=False)
announce.pack()

def frame(packet):
    if packet.raw is None:
        packet.pack()
    return b"\x7e" + HDLC.escape(packet.raw) + b"\x7e"

listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(2)
listener.settimeout(15)
print(json.dumps({"port": listener.getsockname()[1], "data": data_dest.hash.hex(), "announce": ann_dest.hash.hex()}), flush=True)
regular, _ = listener.accept()
limited, _ = listener.accept()
listener.close()
for connection in [regular, limited]:
    connection.settimeout(5)
assert input() == "start"
stop = threading.Event()
counts = [0, 0]
errors = []

def send(connection, index):
    sequence = 0
    try:
        # Bound the kernel/driver backlog independently of the actor's queues.
        # This is a lifecycle test, not an unbounded throughput benchmark.
        while not stop.is_set() and sequence < 256:
            tag = sequence.to_bytes(8, "big") + bytes([index + 1]) * 8
            pr = RNS.Packet(pr_dest, tag + tag)
            batch = frame(pr)
            if index == 0:
                batch += frame(RNS.Packet(data_dest, b"mixed~}" + sequence.to_bytes(8, "big")))
                batch += frame(announce)
            connection.sendall(batch)
            counts[index] += 1
            sequence += 1
            stop.wait(0.001)
        stop.wait(15)
    except OSError as error:
        if not stop.is_set():
            errors.append(str(error))

threads = [threading.Thread(target=send, args=(connection, index), daemon=True)
           for index, connection in enumerate([regular, limited])]
for thread in threads:
    thread.start()
assert input() == "disconnect"
stop.set()
for connection in [regular, limited]:
    connection.shutdown(socket.SHUT_RDWR)
    connection.close()
for thread in threads:
    thread.join(5)
assert all(not thread.is_alive() for thread in threads)
assert not errors, errors
assert all(count > 0 for count in counts), counts
print(json.dumps({"closed": True, "batches": counts}), flush=True)
