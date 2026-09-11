"""Evaluate source arithmetic only; no daemon, sockets or copied policy body."""
import ast
from pathlib import Path
import sys
from types import SimpleNamespace

root = Path(sys.argv[1])
backbone = ast.parse((root / "RNS/Interfaces/BackboneInterface.py").read_text())
cls = next(n for n in backbone.body if isinstance(n, ast.ClassDef) and n.name == "BackboneInterface")
constants = {}
for n in cls.body:
    if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name) and n.targets[0].id.startswith("DP_IC_"):
        constants[n.targets[0].id] = ast.literal_eval(n.value)
values = SimpleNamespace(**constants)


def expression(node, env):
    return eval(compile(ast.Expression(node), "<reference expression>", "eval"), env)


holds = {}
allocations = {}
for method in cls.body:
    if isinstance(method, ast.FunctionDef) and method.name in ("_throttle_immediate", "__dp_ic_job"):
        key = "i" if method.name == "_throttle_immediate" else "p"
        for n in ast.walk(method):
            if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name):
                if n.targets[0].id == "hold": holds[key] = n.value
                if n.targets[0].id == "alloc": allocations[key] = n.value

reticulum = ast.parse((root / "RNS/Reticulum.py").read_text())
marks = {}
for n in ast.walk(reticulum):
    if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Attribute):
        name = n.targets[0].attr
        if name in ("DP_IC_HIGH_WM", "DP_IC_MID_WM", "DP_IC_LOW_WM"):
            marks[name] = n.value

for line in sys.stdin.readlines():
    parts = line.split()
    if parts[0] == "w":
        env = {"BackboneInterface": SimpleNamespace(BackboneInterface=values), "dql": int(parts[1])}
        high, mid, low = [expression(marks[key], env) for key in ("DP_IC_HIGH_WM", "DP_IC_MID_WM", "DP_IC_LOW_WM")]
        print(f"w {high} {mid} {low} {max(128, high)}")
    else:
        _, kind, count, size, span = parts
        span = float(span)
        env = {"BackboneInterface": values, "interfaces": [None] * int(count), "span": span, "avail": int(size) / span}
        env["alloc"] = expression(allocations[kind], env)
        print("h", expression(holds[kind], env))
