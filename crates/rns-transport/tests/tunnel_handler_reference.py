"""Execute the real tunnel handler with isolated tunnel installation tracking."""
import ast
from pathlib import Path
import sys
from types import SimpleNamespace

root = Path(sys.argv[1])
sys.path.insert(0, str(root))
import RNS

tree = ast.parse((root / "RNS/Transport.py").read_text())
handler = next(n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == "tunnel_synthesize_handler")
handler.decorator_list = []
installed = []
namespace = {"RNS": RNS, "Transport": SimpleNamespace(handle_tunnel=lambda *args: installed.append(args))}
exec(compile(ast.Module(body=[handler], type_ignores=[]), "Transport.py", "exec"), namespace)

class Interface:
    def __init__(self):
        self.violations = 0
    def protocol_violation(self, *args):
        self.violations += 1

for line in sys.stdin:
    installed.clear()
    interface = Interface()
    payload = b"" if line.strip() == "-" else bytes.fromhex(line.strip())
    namespace["tunnel_synthesize_handler"](payload, SimpleNamespace(receiving_interface=interface))
    print(f"{len(installed)} {interface.violations}")
