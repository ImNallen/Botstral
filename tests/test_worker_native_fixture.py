import ast
import importlib.util
import inspect
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from release_acceptance_linux import agent_accept, worker_tls_probe
from release_model import ROOT, authority


CONFIG_RUNNER = r'''
import ast, json, os, sys
from pathlib import Path
home = Path(sys.argv[1])
os.environ['BOTSTRAL_ACCEPTANCE_HOME'] = str(home)
tree = ast.parse(sys.stdin.read())
namespace = {'__name__': '__main__'}
for statement in tree.body:
    module = ast.Module(body=[statement], type_ignores=[])
    ast.fix_missing_locations(module)
    exec(compile(module, '<fixture>', 'exec'), namespace)
    path = home / 'fixture-state' / 'config.json'
    if path.exists():
        print(path.read_text())
        raise SystemExit(0)
raise SystemExit('fixture did not write config.json')
'''


def load_container_acceptance():
    spec = importlib.util.spec_from_file_location(
        'worker_container_acceptance', ROOT / 'agent/packaging/container-acceptance.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assignments(function, names):
    source = textwrap.dedent(inspect.getsource(function))
    tree = ast.parse(source)
    found = []

    def visit(nodes):
        for node in nodes:
            if isinstance(node, ast.Assign) and any(
                    isinstance(target, ast.Name) and target.id in names for target in node.targets):
                found.append(node)
            for block in ('body', 'orelse', 'finalbody'):
                children = getattr(node, block, None)
                if children:
                    visit(children)
            for handler in getattr(node, 'handlers', ()):
                visit(handler.body)

    visit(tree.body)
    return found


def emit(function, names, bindings):
    nodes = assignments(function, names)
    if not nodes:
        raise AssertionError(function.__name__ + ' does not assign ' + ', '.join(sorted(names)))
    module = ast.Module(body=nodes, type_ignores=[])
    ast.fix_missing_locations(module)
    namespace = dict(function.__globals__)
    namespace.update(bindings)
    exec(compile(module, function.__code__.co_filename, 'exec'), namespace)
    return namespace


def script_prefix(script, fixture):
    if not script.endswith(fixture):
        raise AssertionError('generated script dropped the fixture body')
    return script[:-len(fixture)]


class WorkerNativeFixtureTests(unittest.TestCase):
    connector_files = {'configure.py': 'configure-fixture', 'runtime.py': 'runtime-fixture'}

    def written_config(self):
        module = load_container_acceptance()
        with tempfile.TemporaryDirectory(prefix='worker-fixture-') as directory:
            result = subprocess.run(
                [sys.executable, '-c', CONFIG_RUNNER, directory],
                input=module.FIXTURE, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            config = json.loads(result.stdout)
            return config, Path(config['client_cert']).read_text()

    def test_fixture_writes_pin_not_manager_ca(self):
        config, certificate = self.written_config()
        self.assertEqual(set(config), {'manager', 'pin', 'client_cert', 'client_key'})
        self.assertEqual(config['pin'], '0' * 64)
        self.assertEqual(config['manager'], 'wss://127.0.0.1:1')
        self.assertTrue(config['client_cert'].endswith('/fixture.pem'), config['client_cert'])
        self.assertEqual(config['client_cert'], config['client_key'])
        self.assertEqual(certificate, 'isolated invalid TLS fixture; no network authentication')

    def test_user_version_follows_injected_worker_schema(self):
        module = load_container_acceptance()
        checks = [line.strip() for line in module.FIXTURE.splitlines()
                  if 'assert' in line and 'user_version' in line]
        self.assertEqual(len(checks), 1)
        check = checks[0]
        self.assertIn('WORKER_SCHEMA', check)
        self.assertNotRegex(check, r'==\s*2\b')
        schema = authority()['data_compatibility']['worker_schema']
        sentinel = schema + 1
        self.compare_user_version(check, sentinel, sentinel)
        with self.assertRaises(AssertionError):
            self.compare_user_version(check, sentinel, schema)

    def compare_user_version(self, check, injected, recorded):
        connection = sqlite3.connect(':memory:')
        connection.execute('PRAGMA user_version=' + str(int(recorded)))
        exec(check, {'db': connection, 'WORKER_SCHEMA': injected})

    def test_native_generator_injects_authority_worker_schema(self):
        module = load_container_acceptance()
        released = authority()['data_compatibility']['worker_schema']
        self.assertNotEqual(released, 41)
        overridden = json.loads(json.dumps(authority()))
        overridden['data_compatibility']['worker_schema'] = 41
        with patch('release_acceptance_linux.authority', return_value=overridden):
            namespace = emit(agent_accept, {'source'}, {'files': self.connector_files, 'module': module})
        prefix = script_prefix(namespace['source'], module.FIXTURE)
        self.assertIn('WORKER_SCHEMA = 41\n', prefix)
        self.assertIn('CONNECTOR_FILES = ' + repr(self.connector_files) + '\n', prefix)
        self.assertNotIn('manager_ca', prefix)
        self.assertNotIn('WORKER_SCHEMA = ' + str(released) + '\n', prefix)

    def test_container_generator_injects_release_json_worker_schema(self):
        module = load_container_acceptance()
        released = json.loads((ROOT / 'release.json').read_text())['data_compatibility']['worker_schema']
        self.assertEqual(released, authority()['data_compatibility']['worker_schema'])
        self.assertNotEqual(released, 41)
        original = Path.read_text

        def read_text(path, *args, **kwargs):
            text = original(path, *args, **kwargs)
            if path.name == 'release.json':
                value = json.loads(text)
                value['data_compatibility']['worker_schema'] = 41
                return json.dumps(value)
            return text

        with patch.object(Path, 'read_text', read_text):
            namespace = emit(module.main, {'schema', 'fixture'}, {'files': self.connector_files})
        prefix = script_prefix(namespace['fixture'], module.FIXTURE)
        self.assertIn('WORKER_SCHEMA = 41\n', prefix)
        self.assertIn('CONNECTOR_FILES = ' + repr(self.connector_files) + '\n', prefix)
        self.assertNotIn('manager_ca', prefix)
        self.assertNotIn('WORKER_SCHEMA = ' + str(released) + '\n', prefix)

    def test_worker_tls_probe_writes_pin_not_manager_ca(self):
        with tempfile.TemporaryDirectory(prefix='worker-tls-probe-') as directory:
            root = Path(directory)
            state = root / 'state'
            state.mkdir(mode=0o700)
            config = state / 'config.json'
            config.write_text('{}\n')
            captured = {}

            def capture(path, value):
                captured['path'] = Path(path)
                captured['value'] = value
                raise RuntimeError('stop after config')

            with patch('release_acceptance_linux.os.chown'), \
                    patch('release_acceptance_linux.run'), \
                    patch('release_acceptance_linux.write_json', side_effect=capture):
                with self.assertRaisesRegex(RuntimeError, 'stop after config'):
                    worker_tls_probe(state, root, lambda: None, {})
            self.assertEqual(captured['path'], config)
            value = captured['value']
            self.assertEqual(set(value), {'manager', 'pin', 'client_cert', 'client_key'})
            self.assertEqual(value['pin'], '0' * 64)
            self.assertTrue(value['manager'].startswith('wss://127.0.0.1:'), value['manager'])
            self.assertTrue(str(value['client_cert']).endswith('certificate.pem'))
            self.assertTrue(str(value['client_key']).endswith('key.pem'))
            self.assertNotEqual(value['client_cert'], value['client_key'])
            self.assertEqual(config.read_text(), '{}\n')


if __name__ == '__main__':
    unittest.main()
