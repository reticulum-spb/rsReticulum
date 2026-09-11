"""Reference two-generation tag admission/maintenance, without RNS startup."""
import ast
from contextlib import nullcontext
from pathlib import Path
from types import SimpleNamespace
import sys

tree = ast.parse((Path(sys.argv[1]) / "RNS/Transport.py").read_text())
cls = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "Transport")
limit = next(n.value.value for n in cls.body if isinstance(n, ast.Assign)
             and any(isinstance(t, ast.Name) and t.id == "max_pr_tags" for t in n.targets))
rotation = next(n for n in ast.walk(cls) if isinstance(n, ast.If)
                and ast.unparse(n.test) == "len(Transport.discovery_pr_tags) > Transport.max_pr_tags")
admissions = [n for n in ast.walk(cls) if isinstance(n, ast.If)
              and "unique_tag" in ast.unparse(n.test)
              and "Transport.discovery_pr_tags_prev" in ast.unparse(n.test)]
assert len(admissions) == 1, "reference tag admission structure changed"
admission = admissions[0]
def compile_node(node):
    return compile(ast.fix_missing_locations(ast.Module(body=[node], type_ignores=[])), "reference-tags", "exec")
rotate = compile_node(rotation)
admit = compile_node(admission)
Transport = SimpleNamespace(discovery_pr_tags=set(), discovery_pr_tags_prev=set(),
                            max_pr_tags=limit, discovery_pr_tags_lock=nullcontext())
for line in sys.stdin:
    operation, *args = line.split()
    if operation == "add":
        unique_tag = bytes.fromhex(args[0])
        tag_valid = False
        exec(admit)
        result = str(int(tag_valid))
    else:
        assert operation == "tick"
        exec(rotate)
        result = "tick"
    print(f"{result} {len(Transport.discovery_pr_tags)} {len(Transport.discovery_pr_tags_prev)}")
