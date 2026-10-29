import json, os, pathlib, subprocess, sys, tempfile, time
root = pathlib.Path(__file__).resolve().parents[2]
evidence = pathlib.Path(os.environ.get('BOTSTRAL_P06_EVIDENCE', root / '.audit/p06-manager-switch'))
evidence.mkdir(parents=True, exist_ok=True)
processes = []
records = []
env = dict(os.environ, BOTSTRAL_P06_EVIDENCE=str(evidence))
result = None
with tempfile.TemporaryDirectory(prefix='p06-independent-native-') as temporary:
    roots = [pathlib.Path(temporary) / name for name in ['manager-a', 'manager-b']]
    try:
        for index, fixture_root in enumerate(roots):
            command = ['python3', 'desktop/fixtures/paired_manager.py', '--root', str(fixture_root), '--duration', '120']
            output = (evidence / f'native-fixture-{index}.log').open('w')
            process = subprocess.Popen(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT)
            processes.append((process, output, fixture_root, command))
        until = time.monotonic() + 90
        while not all((directory / 'fixture.json').exists() for directory in roots):
            if any(process.poll() is not None for process, *_ in processes) or time.monotonic() >= until:
                raise RuntimeError('owned fixture startup failed')
            time.sleep(.1)
        env.update(BOTSTRAL_C02_FIXTURE_ROOT=str(roots[0]), BOTSTRAL_REVIEW_OTHER_MANAGER=str(roots[1]))
        command = ['cargo', 'test', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--no-default-features', '--locked', '--test', 'journal', 'p06_native_receipt_manager_binding', '--', '--ignored', '--exact', '--nocapture']
        with (evidence / 'native-adversarial.log').open('w') as output:
            result = subprocess.run(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT)
        records.append(dict(command=command, exit_code=result.returncode, log='native-adversarial.log'))
    finally:
        for process, output, fixture_root, command in processes:
            fixture_root.mkdir(exist_ok=True)
            (fixture_root / 'stop').touch()
            try:
                exit_code = process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.terminate()
                exit_code = process.wait(timeout=10)
            output.close()
            records.append(dict(command=command, exit_code=exit_code))
        (evidence / 'native-adversarial.json').write_text(json.dumps(records, indent=2)+'\n')
print(json.dumps(records))
sys.exit(result.returncode if result is not None else 1)
