"""Bounded synthetic supervisor proof, including a separate child process group."""
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import threading
import time
import unittest

import redact_repaired_stage as stage


class SupervisorTests(unittest.TestCase):
    def check_cleanup(self, interrupt):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "detached.pid"
            child = ("import os,signal,time; from pathlib import Path; "
                     "signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                     f"Path({str(pidfile)!r}).write_text(str(os.getpid())); time.sleep(30)")
            parent = ("import signal,subprocess,sys,time; "
                      "signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                      f"subprocess.Popen([sys.executable,'-c',{child!r}],start_new_session=True); "
                      "time.sleep(30)")
            timer = None
            if interrupt:
                timer = threading.Timer(0.7, lambda: os.kill(os.getpid(), signal.SIGINT))
                timer.start()
            try:
                with (root / "output.log").open("wb") as log:
                    result = stage.supervise([sys.executable, "-c", parent], cwd=root,
                                             env=os.environ.copy(), log=log,
                                             deadline=time.time() + 6)
            finally:
                if timer:
                    timer.cancel()
                    timer.join()
            with (root / "receipt.json").open("x") as handle:
                json.dump(result, handle)
            self.assertEqual(result["status"], "interrupted" if interrupt else "timeout")
            self.assertNotEqual(result["exit_code"], 0)
            self.assertEqual(result["owned_remaining"], 0)
            self.assertLess(result["seconds"], 6)
            self.assertTrue(pidfile.exists())
            self.assertNotIn(int(pidfile.read_text()), stage.process_snapshot())
            self.assertEqual(json.loads((root / "receipt.json").read_text()), result)

    def test_deadline_kills_detached_owned_child_and_retains_receipt(self):
        self.check_cleanup(False)

    def test_interrupt_kills_detached_owned_child_and_retains_receipt(self):
        self.check_cleanup(True)


if __name__ == "__main__":
    unittest.main()
