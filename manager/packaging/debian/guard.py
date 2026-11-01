import json
import os
from pathlib import Path
import subprocess
import sys


def refuse(message):
    sys.exit('Botstral package refused: ' + message)


def guard(component, version, identity, operation):
    package = 'botstral-' + component
    executable = '/usr/bin/' + package
    if not Path('/proc/self/exe').exists():
        refuse('Linux procfs is required for active process checks')
    for process in Path('/proc').iterdir():
        if not process.name.isdecimal():
            continue
        try:
            running = os.readlink(str(process / 'exe')).removesuffix(' (deleted)')
            arguments = (process / 'cmdline').read_bytes().split(b'\0')
        except FileNotFoundError:
            continue
        except PermissionError:
            refuse('cannot inspect all running processes')
        protected = {executable, executable + '-supervisor-check'} if component == 'agent' else {executable}
        if running in protected or any(os.fsencode(path) in arguments for path in protected):
            refuse('component is active; stop admission, drain, stop and back up both hosts first')
    for path in ('/usr/local/bin/' + package, '/opt/' + package):
        if os.path.lexists(path):
            refuse('unmanaged installation at ' + path + '; retire it explicitly')
    if operation == 'remove':
        return
    other = 'agent' if component == 'manager' else 'manager'
    for path in ('/usr/bin/botstral-' + other, '/usr/lib/botstral-' + other, '/usr/local/bin/botstral-' + other):
        if os.path.lexists(path):
            refuse('manager and worker require separate hosts')
    native_arguments = sys.argv[2:]
    if len(native_arguments) >= 2 and native_arguments[0] == 'upgrade':
        previous = native_arguments[1]
        if subprocess.run(['dpkg', '--compare-versions', version, 'lt', previous]).returncode == 0:
            refuse('downgrade requires full backup recovery after explicit package removal')
        if previous == version:
            metadata = Path('/usr/lib/' + package + '/release.json')
            if not metadata.is_file() or metadata.is_symlink():
                refuse('installed identity metadata is missing; use explicit backup recovery')
            if json.loads(metadata.read_text()) != identity:
                refuse('same native version has a different release identity')
    unit_dir = '/usr/lib/systemd/' + ('system' if component == 'manager' else 'user')
    paths = [executable, executable + '-backup', executable + '-diagnose', '/usr/lib/' + package, '/usr/share/' + package,
             unit_dir + '/' + package + '.service', unit_dir + '/' + package + '-update.service',
             unit_dir + '/' + package + '-update.timer', unit_dir + '/' + package + '-update.path']
    if component == 'agent':
        paths.append(executable + '-supervisor-check')
    for path in paths:
        if os.path.lexists(path):
            if not native_arguments or native_arguments[0] != 'upgrade':
                refuse('existing unmanaged path ' + path + '; adoption is not supported')
            owner = subprocess.run(['dpkg-query', '-S', path], capture_output=True, text=True)
            if owner.returncode != 0 or not owner.stdout.startswith(package + ': '):
                refuse('existing unmanaged path ' + path + '; adoption is not supported')


guard(*json.loads(sys.argv[1]))
