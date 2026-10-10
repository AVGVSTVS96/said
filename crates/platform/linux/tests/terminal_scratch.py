import os, pathlib, sys, termios, tty
output = pathlib.Path(sys.argv[1])
output.write_bytes(b"")
fd = sys.stdin.fileno()
previous = termios.tcgetattr(fd)
start, end = b'\x1b[200~', b'\x1b[201~'
pending, text = b'', b''
try:
    tty.setraw(fd)
    os.write(sys.stdout.fileno(), b'\x1b[?2004hScratch editor: bracketed paste only.\r\n')
    output.with_suffix('.ready').touch()
    while True:
        pending += os.read(fd, 4096)
        while pending:
            if pending.startswith(start):
                stop = pending.find(end, len(start))
                if stop < 0: break
                text += pending[len(start):stop]
                output.write_bytes(text)
                pending = pending[stop + len(end):]
            elif start.startswith(pending): break
            else:
                with output.with_suffix('.keys').open('ab') as log: log.write(pending[:1])
                pending = pending[1:]
finally:
    os.write(sys.stdout.fileno(), b'\x1b[?2004l')
    termios.tcsetattr(fd, termios.TCSANOW, previous)
