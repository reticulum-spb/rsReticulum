"""Execute the actual reference admission prefix without starting RNS runtime."""
import ast
import sys
import time
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, sys.argv[1])
import RNS

RNS.sl = lambda level: False
source = ast.parse((Path(sys.argv[1]) / "RNS/Transport.py").read_text())
transport = next(node for node in source.body if isinstance(node, ast.ClassDef) and node.name == "Transport")
packet_filter = next(node for node in transport.body if isinstance(node, ast.FunctionDef) and node.name == "packet_filter")
preprocess = next(node for node in transport.body if isinstance(node, ast.FunctionDef) and node.name == "preprocess_inbound")
# Retain all original admission code up to the announce MTU check. Stop before
# signature validation/queue processing, exercised separately by Rust tests.
prefix = []
found = False
for statement in preprocess.body:
    prefix.append(statement)
    if isinstance(statement, ast.If) and ast.unparse(statement.test) == "packet.packet_type == RNS.Packet.ANNOUNCE":
        assert "len(raw) > RNS.Reticulum.MTU" in ast.unparse(statement.body[0])
        statement.body = statement.body[:1]
        statement.orelse = []
        found = True
        break
assert found, "reference admission structure changed"
preprocess.body = prefix + [ast.Return(value=ast.Constant(value=True))]
transport.body = [packet_filter, preprocess]
exec(compile(ast.fix_missing_locations(ast.Module(body=[transport], type_ignores=[])), "reference-admission", "exec"))
Transport.ready = True
Transport.identity = SimpleNamespace(hash=bytes(16))
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
Transport.TC_DATA = 0
Transport.rx_packets = 0

class Interface:
    ifac_identity = None
    ifac_size = 0
    def __init__(self, mtu):
        self.HW_MTU = mtu
        self.protocol = 0
        self.filters = 0
    def protocol_violation(self, description=None):
        self.protocol += 1
    def packet_filter_hit(self):
        self.filters += 1
    def __str__(self):
        return "oracle"

for line in sys.stdin:
    raw, client, mtu = line.split()
    interface = Interface(int(mtu))
    Transport.owner = SimpleNamespace(is_connected_to_shared_instance=client == "1")
    accepted = Transport.preprocess_inbound(bytes.fromhex(raw), interface) is True
    print(f"{int(accepted)} {interface.protocol} {interface.filters}")
