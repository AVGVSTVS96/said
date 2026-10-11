#!/usr/bin/env python3
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

BINARY = Path(os.environ['SAID_APP_BINARY']).resolve()
MODELS = Path(os.environ['SAID_MODEL_CACHE']).resolve()
STATE = Path(os.environ['SAID_FIRST_RUN_STATE'])


def wait_for(check, processes, label, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        if any(process.poll() is not None for process in processes):
            raise RuntimeError(f'Process exited while waiting for {label}; see {STATE}')
        time.sleep(0.05)
    raise TimeoutError(f'Timed out waiting for {label}; see {STATE}')


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)


def inside():
    processes = []
    try:
        (STATE / 'niri.kdl').write_text('hotkey-overlay { skip-at-startup; }\nanimations { off; }\n')
        cage = subprocess.Popen(['cage', '-d', '--', sys.executable, __file__, '--niri'],
                                start_new_session=True, stdout=(STATE / 'cage.log').open('w'), stderr=subprocess.STDOUT)
        processes.append(cage)
        wait_for(lambda: (STATE / 'environment.json').exists(), processes, 'isolated Niri')
        session = json.loads((STATE / 'environment.json').read_text())
        assert Path(session['XDG_RUNTIME_DIR']).resolve() == STATE / 'runtime'
        assert session['WAYLAND_DISPLAY'] != session['PARENT_DISPLAY']
        env = {**os.environ, **session, 'GDK_BACKEND': 'wayland'}
        env.pop('DISPLAY', None)
        data = Path(env['XDG_DATA_HOME']) / 'said'
        data.mkdir()
        (data / 'models').symlink_to(MODELS)
        config = Path(env['XDG_CONFIG_HOME']) / 'said'
        config.mkdir()

        def windows():
            result = subprocess.check_output(['niri', 'msg', '-j', 'windows'], env=env)
            all_windows = json.loads(result)
            (STATE / 'windows.json').write_text(json.dumps(all_windows, indent=2))
            return [window for window in all_windows if window.get('pid') == app.pid]

        def status():
            result = subprocess.run([str(BINARY), 'status'], env=env, capture_output=True, text=True, timeout=5)
            return json.loads(result.stdout) if result.returncode == 0 else None

        started = time.monotonic()
        app = subprocess.Popen([str(BINARY)], env=env, start_new_session=True,
                               stdout=(STATE / 'first.log').open('w'), stderr=subprocess.STDOUT)
        processes.append(app)
        initial_status = wait_for(status, processes, 'initial host snapshot')
        (STATE / 'initial-status.json').write_text(json.dumps(initial_status, indent=2))
        shown = wait_for(windows, processes, 'first-launch Setup')
        assert len(shown) == 1 and 'setup' in shown[0]['title'].lower(), shown
        elapsed = time.monotonic() - started
        first = wait_for(status, processes, 'first snapshot')
        assert first['settings']['hotkey'] == 'super+z'
        assert first['settings']['interactive_hotkey'] == 'super+x'
        assert first['settings']['insert_last_hotkey'] == 'super+a'
        assert first['host']['setup_complete'] is False
        assert first['sessions'] == []
        (STATE / 'first-status.json').write_text(json.dumps(first, indent=2))
        time.sleep(1)
        subprocess.run(['grim', str(STATE / 'first-setup.png')], env=env, check=True)
        stop(app)
        processes.remove(app)

        (config / 'setup-complete').write_text('complete\n')
        app = subprocess.Popen([str(BINARY)], env=env, start_new_session=True,
                               stdout=(STATE / 'complete.log').open('w'), stderr=subprocess.STDOUT)
        processes.append(app)
        complete = wait_for(status, processes, 'completed-setup snapshot')
        assert complete['host']['setup_complete'] is True
        time.sleep(1)
        assert windows() == [], windows()
        (STATE / 'complete-status.json').write_text(json.dumps(complete, indent=2))
        print(f'PASS: Setup appeared without a hotkey in {elapsed:.2f}s; completed setup stayed hidden; no input or capture. {STATE}', flush=True)
    finally:
        for process in reversed(processes):
            stop(process)


if '--environment' in sys.argv:
    keys = ['XDG_RUNTIME_DIR', 'WAYLAND_DISPLAY', 'NIRI_SOCKET', 'DBUS_SESSION_BUS_ADDRESS', 'PARENT_DISPLAY']
    temporary = STATE / 'environment.part'
    temporary.write_text(json.dumps({key: os.environ[key] for key in keys}))
    temporary.replace(STATE / 'environment.json')
elif '--niri' in sys.argv:
    os.environ['PARENT_DISPLAY'] = os.environ['WAYLAND_DISPLAY']
    raise SystemExit(subprocess.call(['niri', '--config', str(STATE / 'niri.kdl'), '--', sys.executable, __file__, '--environment'],
                                    stdout=(STATE / 'niri.log').open('w'), stderr=subprocess.STDOUT))
elif '--inside' in sys.argv:
    inside()
else:
    STATE.mkdir(parents=True)
    runtime = STATE / 'runtime'
    runtime.mkdir(mode=0o700)
    env = {**os.environ, 'XDG_RUNTIME_DIR': str(runtime), 'WLR_BACKENDS': 'headless',
           'WLR_RENDERER': 'pixman', 'NIRI_DISABLE_SYSTEM_MANAGER_NOTIFY': '1'}
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'NIRI_SOCKET', 'DBUS_SESSION_BUS_ADDRESS',
                'XDG_SESSION_ID', 'ELECTRON_RUN_AS_NODE', 'PIPEWIRE_RUNTIME_DIR', 'PULSE_SERVER']:
        env.pop(key, None)
    for variable, name in [('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache')]:
        directory = STATE / name
        directory.mkdir()
        env[variable] = str(directory)
    raise SystemExit(subprocess.call(['dbus-run-session', '--', sys.executable, __file__, '--inside'], env=env))
