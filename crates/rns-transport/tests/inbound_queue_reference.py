"""Read-only oracle for the local Python Reticulum InboundQueues class.

Only the class AST is executed, avoiding Reticulum startup, sockets, storage,
and optional Python dependencies. Backbone throttling belongs to stage 5 and
is deliberately excluded from this queue scheduling/admission comparison.
"""

import ast
from collections import deque
from pathlib import Path
from queue import Empty, Full
import sys
from threading import Condition
import time


class Transport:
    TC_DATA = 0


source = Path(sys.argv[1]) / "RNS" / "Transport.py"
tree = ast.parse(source.read_text(), filename=str(source))
queue_class = next(
    node for node in tree.body
    if isinstance(node, ast.ClassDef) and node.name == "InboundQueues"
)
exec(compile(ast.Module(body=[queue_class], type_ignores=[]), str(source), "exec"))
queues = InboundQueues((7, 5, 3, 2), hw_mark=10000000)
for operation in sys.stdin.read().splitlines():
    fields = operation.split()
    if fields[0] == "put":
        item = int(fields[2])
        try:
            queues.put(int(fields[1]), item, block=False)
            result = "ok"
        except Full:
            result = f"full:{item}"
    else:
        try:
            result = f"item:{queues.get(block=False)}"
        except Empty:
            result = "empty"
    total, heights, dropped = queues.snapshot()
    print(f"{result}|{total}|{list(heights)}|{list(dropped)}")
