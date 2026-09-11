"""Run the actual local Python evaluator, with socket/log side effects stubbed."""
import ast
import json
from pathlib import Path
import sys
from types import SimpleNamespace

root = Path(sys.argv[1])
source = root / "RNS/Interfaces/BackboneInterface.py"
tree = ast.parse(source.read_text())
cls = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "BackboneInterface")
selected = []
for node in cls.body:
    if isinstance(node, ast.Assign) and any(
        isinstance(t, ast.Name) and t.id.startswith("DP_EC_") for t in node.targets
    ):
        selected.append(node)
    elif isinstance(node, ast.FunctionDef) and node.name == "_dp_ec_evaluate":
        selected.append(node)
cls.bases = []
cls.decorator_list = []
cls.body = selected
namespace = {
    "RNS": SimpleNamespace(
        log=lambda *args: None, sl=lambda *args: False,
        prettyshorttime=lambda *args, **kwargs: "", LOG_NOTICE=1, LOG_DEBUG=2, LOG_ERROR=3,
    )
}
exec(compile(ast.fix_missing_locations(ast.Module(body=[cls], type_ignores=[])), str(source), "exec"), namespace)
backbone = namespace["BackboneInterface"]


class Buffer:
    def __len__(self):
        return self.buffered


results = []
for sequence in json.load(sys.stdin):
    buffer = Buffer()
    interface = SimpleNamespace(
        transmit_buffer=buffer, _dp_ec_prev_sent=0, _dp_ec_last_drain=0,
        _dp_ec_zero_ticks=0, tx_stalled=False, socket=None, receive=lambda data: None,
    )
    states = []
    for now, buffered, sendable, sent in sequence:
        buffer.buffered, buffer.sendable, buffer._tx_sent = buffered, sendable, sent
        dead = backbone._dp_ec_evaluate(interface, now)
        states.append([
            "Disconnect" if dead else "Gated" if interface.tx_stalled else "Open",
            interface.tx_stalled, interface._dp_ec_zero_ticks,
            interface._dp_ec_last_drain, interface._dp_ec_prev_sent,
        ])
    results.append(states)
json.dump(results, sys.stdout)
