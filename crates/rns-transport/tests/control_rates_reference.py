"""Evaluate actual Python transport speed expressions without starting RNS."""
import ast
from pathlib import Path
import sys

root = Path(sys.argv[1])
tree = ast.parse((root / "RNS/Transport.py").read_text())
loop = next(n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == "count_traffic_loop")
names = ["carxs", "catxs", "cprxs", "cptxs"]
expressions = {}
for node in ast.walk(loop):
    if isinstance(node, ast.Assign) and len(node.targets) == 1:
        target = node.targets[0]
        if isinstance(target, ast.Name) and target.id in names:
            assert target.id not in expressions
            expressions[target.id] = compile(ast.Expression(node.value), "Transport.py", "eval")
assert len(expressions) == 4
for sample in sys.argv[2:]:
    seconds, *deltas = map(float, sample.split(","))
    namespace = dict(zip(["arx_diff", "atx_diff", "prx_diff", "ptx_diff"], deltas))
    namespace["ts_diff"] = seconds
    print(",".join(str(eval(expressions[name], namespace)) for name in names))
