"""Synthetic subprocess protocol and bounded descendant ownership witnesses."""
import ctypes
from ctypes import wintypes
import msvcrt
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
    if fd != 0:
        # Configure before publishing readiness, while the peer is still open.
        os.set_blocking(fd, False)
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.WriteFile.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                                     ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]
        kernel.WriteFile.restype = wintypes.BOOL
        handle = msvcrt.get_osfhandle(fd)
        buffer = ctypes.create_string_buffer(b'w' * 4096)
    publish('.ready')
    # Delay stdin consumption and stdout probes until after cancellation.
    if fd in (0, 1):
        time.sleep(3)
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
                # The CRT can flatten a broken-pipe write to EINVAL. Inspect
                # the Win32 result directly, never treat generic EINVAL as proof.
                written = wintypes.DWORD()
                if not kernel.WriteFile(handle, buffer, 4096, ctypes.byref(written), None):
                    code = ctypes.get_last_error()
                    if code in (109, 232, 233):
                        publish('.closed', 'broken-pipe:' + str(code))
                        os._exit(0)
                    publish('.error', 'WriteFile:' + str(code))
                    os._exit(96)
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
    if fd != 0:
        # Complete stdin before exiting, so this case isolates a held reader.
        sys.stdin.buffer.read()
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
