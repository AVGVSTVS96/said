#!/usr/bin/env python3
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
BASE = Path(os.environ.get('VOICE_LINUX_TEST_DIR', Path.home() / '.cache/voice-linux-tests')).resolve()


def wait_for(predicate, processes, timeout=20):
    deadline = time.monotonic() + timeout
    while not predicate():
        if any(process.poll() is not None for process in processes):
            raise RuntimeError('A scratch process exited; inspect its log in ' + str(STATE))
        if time.monotonic() > deadline:
            raise TimeoutError('Scratch startup timed out; inspect ' + str(STATE))
        time.sleep(0.05)


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()


def inside():
    processes = []
    try:
        binary = Path(os.environ.get('VOICE_APP_BINARY', BASE / 'harness/target/debug/voice'))
        (STATE / 'niri.kdl').write_text(f'''input {{ keyboard {{ xkb {{ layout "us"; }}; }}; }};
prefer-no-csd
animations {{ off; }}
binds {{
    Ctrl+Alt+Space repeat=false allow-inhibiting=false {{ spawn "{binary}" "hotkey"; }}
    Ctrl+Alt+Shift+Space repeat=false allow-inhibiting=false {{ spawn "{binary}" "hotkey" "interactive"; }}
    Ctrl+Alt+V repeat=false allow-inhibiting=false {{ spawn "{binary}" "hotkey" "insert-last"; }}
}}
''')
        cage = subprocess.Popen(['cage', '-d', '--', sys.executable, __file__, '--niri-child'],
                                start_new_session=True, stdout=(STATE / 'cage.log').open('w'), stderr=subprocess.STDOUT)
        processes.append(cage)
        wait_for(lambda: (STATE / 'environment.json').exists(), processes)
        session = json.loads((STATE / 'environment.json').read_text())
        assert Path(session['XDG_RUNTIME_DIR']).resolve() == STATE / 'runtime'
        assert Path(session['NIRI_SOCKET']).parent == STATE / 'runtime'
        assert session['WAYLAND_DISPLAY'] != session['VOICE_TEST_PARENT_DISPLAY']
        env = dict(os.environ, **session, GDK_BACKEND='wayland',
                   VOICE_TEST_ROOT=str(STATE), VOICE_SCRATCH_OUTPUT=str(STATE / 'electron.txt'))
        for key in ['DISPLAY', 'WAYLAND_SOCKET', 'ELECTRON_RUN_AS_NODE']:
            env.pop(key, None)
        commands = [
            [sys.executable, str(HERE / 'gtk_scratch.py'), str(STATE / 'gtk.txt')],
            ['ghostty', '--config-default-files=false', '--gtk-single-instance=false', '-e',
             sys.executable, str(HERE / 'terminal_scratch.py'), str(STATE / 'terminal.txt')],
            ['electron', '--ozone-platform=wayland', '--disable-gpu', '--no-sandbox',
             '--user-data-dir=' + str(STATE / 'electron-profile'), str(HERE / 'electron_scratch.cjs')],
        ]
        for index, command in enumerate(commands):
            processes.append(subprocess.Popen(command, env=env, start_new_session=True,
                                               stdout=(STATE / f'target-{index}.log').open('w'), stderr=subprocess.STDOUT))
        wait_for(lambda: all((STATE / f'{name}.ready').exists() for name in ['gtk', 'terminal', 'electron']), processes)
        command = json.loads(os.environ['VOICE_APP_E2E']) if 'VOICE_APP_E2E' in os.environ else [str(BASE / 'harness/target/debug/e2e')]
        result = subprocess.run(command, env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=180)
        (STATE / 'results.log').write_text(result.stdout)
        print(result.stdout, end='', flush=True)
        if result.returncode:
            raise RuntimeError('E2E failed; evidence in ' + str(STATE))
        print('Evidence: ' + str(STATE), flush=True)
    finally:
        for process in reversed(processes):
            stop(process)


if '--record-environment' in sys.argv:
    STATE = Path(os.environ['VOICE_LINUX_STATE'])
    keys = ['XDG_RUNTIME_DIR', 'WAYLAND_DISPLAY', 'NIRI_SOCKET', 'DBUS_SESSION_BUS_ADDRESS', 'VOICE_TEST_PARENT_DISPLAY']
    (STATE / 'environment.json').write_text(json.dumps({key: os.environ[key] for key in keys}))
elif '--niri-child' in sys.argv:
    STATE = Path(os.environ['VOICE_LINUX_STATE'])
    os.environ['VOICE_TEST_PARENT_DISPLAY'] = os.environ['WAYLAND_DISPLAY']
    raise SystemExit(subprocess.call(['niri', '--config', str(STATE / 'niri.kdl'), '--',
                                     sys.executable, __file__, '--record-environment'],
                                    stdout=(STATE / 'niri.log').open('w'), stderr=subprocess.STDOUT))
elif '--inside' in sys.argv:
    STATE = Path(os.environ['VOICE_LINUX_STATE'])
    inside()
else:
    BASE.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, VOICE_LINUX_TEST_DIR=str(BASE), CARGO_BUILD_JOBS='2')
    if 'VOICE_APP_E2E' not in env:
        subprocess.run([str(HERE / 'build.sh')], env=env, check=True)
    STATE = Path(tempfile.mkdtemp(prefix='session-', dir=BASE))
    runtime = STATE / 'runtime'
    runtime.mkdir(mode=0o700)
    env.update(VOICE_LINUX_STATE=str(STATE), XDG_RUNTIME_DIR=str(runtime),
               WLR_BACKENDS='headless', WLR_RENDERER='pixman', NIRI_DISABLE_SYSTEM_MANAGER_NOTIFY='1')
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'NIRI_SOCKET', 'DBUS_SESSION_BUS_ADDRESS',
                'XDG_SESSION_ID', 'ELECTRON_RUN_AS_NODE']:
        env.pop(key, None)
    for variable, directory in [('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache')]:
        path = STATE / directory
        path.mkdir()
        env[variable] = str(path)
    raise SystemExit(subprocess.call(['dbus-run-session', '--', sys.executable, __file__, '--inside'], env=env))
