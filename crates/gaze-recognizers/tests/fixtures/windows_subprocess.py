"""Synthetic subprocess protocol and bounded descendant ownership witnesses."""
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

# Even a deliberately blocked fixture cannot outlive this watchdog.
threading.Thread(target=lambda: (time.sleep(12), os._exit(91)), daemon=True).start()
mode, backend, marker = sys.argv[1:4]
reply = b'[]' if backend == 'kiji' else b'{"detected_spans":[]}'

def publish(suffix, value='ready'):
    p = Path(marker + suffix)
    tmp = p.with_suffix(p.suffix + '.tmp')
    tmp.write_text(value)
    tmp.replace(p)

if mode.startswith('witness'):
    fd = int(mode[-1])
    publish('.ready')
    # Delay consumption until after the caller's deadline for blocked stdin.
    if fd in (0, 1):
        time.sleep(3)
    if fd != 0:
        try:
            os.set_blocking(fd, False)
        except OSError as error:
            publish('.error', repr(error))
            os._exit(94)
    count = 0
    end = time.monotonic() + 7
    while time.monotonic() < end:
        try:
            if fd == 0:
                chunk = os.read(fd, 8192)
                if not chunk:
                    publish('.closed', str(count))
                    os._exit(0)
                count += len(chunk)
            else:
                os.write(fd, b'w' * 4096)
        except BrokenPipeError:
            publish('.closed')
            os._exit(0)
        except BlockingIOError:
            pass
        except OSError as error:
            publish('.error', repr(error))
            os._exit(95)
        time.sleep(0.005)
    os._exit(92)

if mode.startswith('hold'):
    fd = int(mode[-1])
    subprocess.Popen([sys.executable, __file__, 'witness' + str(fd), backend, marker],
                     stdin=sys.stdin if fd == 0 else subprocess.DEVNULL,
                     stdout=sys.stdout if fd == 1 else subprocess.DEVNULL,
                     stderr=sys.stderr if fd == 2 else subprocess.DEVNULL)
    # Wait for proof of inheritance before the direct child exits.
    while not Path(marker + '.ready').exists():
        time.sleep(0.005)
    if fd != 1:
        os.write(1, reply)
    os._exit(0)

if mode == 'broken-stdin':
    os.close(0)
    time.sleep(5)
    os._exit(0)

received = sys.stdin.buffer.read()
if mode == 'unicode':
    os.write(2, ('safe ' * 40 + 'aliceé' + 'x' * 60 + '@example.invalid').encode())
    os._exit(7)
if mode == 'noisy' or mode == 'echo-count':
    for _ in range(256):
        os.write(2, b'warning ' * 1024)
if mode == 'echo-count' and len(received) != 2 * 1024 * 1024:
    os._exit(93)
if mode == 'stdout-cap':
    os.write(1, b'w' * 4096)
    time.sleep(5)
if mode == 'invalid-json':
    os.write(1, b'not-json')
elif mode == 'invalid-utf8':
    os.write(1, b'\xff')
else:
    os.write(1, reply)
os._exit(0)
