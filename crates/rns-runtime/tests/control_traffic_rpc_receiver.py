"""Compare Rust RPC counters with the reference Interface methods, without RNS startup."""
import ast
from collections import deque
from pathlib import Path
import runpy
import sys
import time

root = Path(sys.argv[1])
source = ast.parse((root / "RNS/Interfaces/Interface.py").read_text())
interface = next(n for n in source.body if isinstance(n, ast.ClassDef) and n.name == "Interface")
methods = ["received_announce", "sent_announce", "received_path_request", "sent_path_request"]
selected = [n for n in interface.body if isinstance(n, ast.FunctionDef) and n.name in methods]
assert len(selected) == 4
namespace = {"time": time}
for node in selected:
    exec(compile(ast.Module(body=[node], type_ignores=[]), "Interface.py", "exec"), namespace)
Reference = type("Reference", (), {name: namespace[name] for name in methods})
reference = Reference()
keys = ["arxb", "atxb", "arxc", "atxc", "prxb", "ptxb", "prxc", "ptxc"]
for key in keys:
    setattr(reference, key, 0)
for key in ["ia_freq_deque", "oa_freq_deque", "ip_freq_deque", "op_freq_deque"]:
    setattr(reference, key, deque(maxlen=16))
reference.parent_interface = None
for index, method in enumerate(methods):
    for size in [0, 51 + index, 500 + index]:
        getattr(reference, method)(size=size)
codec = runpy.run_path(str(root / "RNS/vendor/umsgpack.py"))
stats = codec["unpackb"](bytes.fromhex(sys.argv[2]))["interfaces"][0]
assert {key: stats[key] for key in keys} == {key: getattr(reference, key) for key in keys}
