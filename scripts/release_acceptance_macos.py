"""Exercise real DMG replacement and synthetic Keychain continuity in a temporary app path."""
import json
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import uuid

from release_acceptance import run
from release_artifacts import inspect_app
from release_model import require
from release_upgrade import replace_app, stopped


def accept(args, old, new, old_artifacts, artifacts):
    require(sys.platform == 'darwin' and platform.machine() == 'arm64', 'Apple silicon acceptance required')
    with tempfile.TemporaryDirectory(prefix='botstral-native-upgrade-') as directory:
        root = Path(directory).resolve()
        app = root / 'Botstral.app'
        old_dmg = args.old.resolve() / old_artifacts[0].filename
        new_dmg = args.new.resolve() / artifacts[0].filename
        replace_app(old_dmg, app, old, args.production)
        executable = inspect_app(app, old, args.production)
        require(json.loads(subprocess.check_output([str(executable), '--version-json']))['identity'] == old.json(), 'old packaged desktop identity differs')
        slot = str(uuid.uuid4())
        command = [executable, '--release-keychain-check', slot]
        run([*command, 'write'])
        try:
            # A running packaged executable must prevent replacement.
            process = subprocess.Popen([str(executable), '--release-acceptance-hold'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            try:
                require(process.stdout.readline().strip() == 'release acceptance process ready', 'packaged executable readiness missing')
                require(process.poll() is None, 'packaged application did not start')
                try:
                    stopped({str(executable)})
                except ValueError as error:
                    require('active desktop' in str(error), 'unexpected active replacement failure')
                else:
                    raise ValueError('running desktop replacement was not refused')
            finally:
                process.terminate()
                process.wait(timeout=15)
            replace_app(new_dmg, app, new, args.production)
            executable = inspect_app(app, new, args.production)
            require(json.loads(subprocess.check_output([str(executable), '--version-json']))['identity'] == new.json(), 'new packaged desktop identity differs')
            run([executable, '--release-keychain-check', slot, 'verify'])
            # Model an interrupted copy, then repair from the same verified immutable DMG.
            shutil.rmtree(app / 'Contents/MacOS')
            retired = app.with_name('Botstral.app.retired-' + new.build_id)
            if retired.exists():
                shutil.rmtree(retired)
            replace_app(new_dmg, app, new, args.production)
            run([executable, '--release-keychain-check', slot, 'verify'])
            return {'real_dmg_old_to_new': True, 'packaged_cli_identities': True,
                    'active_replacement_refused': True, 'interrupted_copy_repaired': True,
                    'synthetic_native_keychain_continuity': True,
                    'scope': 'temporary native app bundles and synthetic Keychain entry; paired workspace state is covered by manager acceptance'}
        finally:
            run([executable, '--release-keychain-check', slot, 'delete'])
