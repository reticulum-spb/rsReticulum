"""Decode with the read-only reference codec; no RNS runtime or sockets."""
import runpy
import sys
from pathlib import Path

codec = runpy.run_path(str(Path(sys.argv[1]) / "RNS/vendor/umsgpack.py"))
stats = codec["unpackb"](bytes.fromhex(sys.argv[2]))
assert stats["interfaces"] == []
assert stats["rxqt"] == 30
assert [stats[key] for key in ("rxqd", "rxqa", "rxqp", "rxqil")] == [2, 4, 8, 16]
assert stats["rxqtd"] == 10
assert [stats[key] for key in ("rxqdd", "rxqad", "rxqpd", "rxqild")] == [1, 2, 3, 4]
assert all(stats[key] == 1.0 for key in ("tqpressure", "dqpressure", "aqpressure", "pqpressure", "ilqpressure"))
