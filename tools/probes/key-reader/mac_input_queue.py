"""Whether a Mac's pty master can say how many input bytes its program has not read (T-1336).

    python3 -I tools/probes/key-reader/mac_input_queue.py

`FIONREAD` on the master answers the unread INPUT (equal to `FIONREAD` of the slave); `TIOCOUTQ` on
the master counts the program's unread OUTPUT. So a Mac pane host could wait until `FIONREAD(master)`
is 0 before it writes a part that begins with Escape, as the Windows pane host does
(`issues/an-escape-led-key-can-reach-a-harness-as-a-lone-escape.md`). Measured on macOS, 2026-11-09.
"""
import fcntl, os, pty, struct, subprocess, termios, time, select

def outq(fd):
    return struct.unpack('i', fcntl.ioctl(fd, termios.TIOCOUTQ, struct.pack('i', 0)))[0]

def nread(fd):
    return struct.unpack('i', fcntl.ioctl(fd, termios.FIONREAD, struct.pack('i', 0)))[0]

master, slave = pty.openpty()
child = subprocess.Popen(['/bin/sh', '-c', 'stty raw -echo; printf ready; sleep 2; dd bs=4096 count=1 2>/dev/null | od -An -tx1 | wc -c'], stdin=slave, stdout=slave, stderr=slave)
seen = b''
while b'ready' not in seen:
    if select.select([master], [], [], 0.2)[0]:
        seen += os.read(master, 100)
fl = fcntl.fcntl(master, fcntl.F_GETFL); fcntl.fcntl(master, fcntl.F_SETFL, fl | os.O_NONBLOCK)
print('rest        TIOCOUTQ(master)=%d FIONREAD(master)=%d FIONREAD(slave)=%d' % (outq(master), nread(master), nread(slave)))
for n in (100, 500, 400):
    os.write(master, b'x' * n)
    print('wrote %-4d  TIOCOUTQ(master)=%d FIONREAD(master)=%d FIONREAD(slave)=%d' % (n, outq(master), nread(master), nread(slave)))
time.sleep(2.5)
print('child read  TIOCOUTQ(master)=%d FIONREAD(slave)=%d' % (outq(master), nread(slave)))
child.wait()

# 2: the program's own unread OUTPUT beside unread input
master, slave = pty.openpty()
child = subprocess.Popen(['/bin/sh', '-c', 'stty raw -echo; printf ready; sleep 1; head -c 300 /dev/zero | tr "\\0" y; sleep 3'], stdin=slave, stdout=slave, stderr=slave)
seen = b''
while b'ready' not in seen:
    if select.select([master], [], [], 0.2)[0]:
        seen += os.read(master, 100)
os.write(master, b'x' * 100)
time.sleep(1.5)
print('300 bytes of output unread by the host, 100 of input unread by the program:')
print('  FIONREAD(master)=%d FIONREAD(slave)=%d TIOCOUTQ(master)=%d' % (nread(master), nread(slave), outq(master)))
os.read(master, 1000)
print('  after the host read its output: FIONREAD(master)=%d FIONREAD(slave)=%d' % (nread(master), nread(slave)))
child.kill()
