#!/usr/bin/env python3
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT = ROOT / 'manager/tests/evidence/m05'
CHECKS = {
    'm05-local-source-final': {
        'public_setup_absent_before_owner_with_forwarded_headers',
        'separate_worker_and_owner_admission',
        'native_persisted_pairing_and_authenticated_snapshot',
        'exact_pin_precedes_all_application_bytes_and_unknown_mutations_are_not_replayed',
        'concurrent_native_credential_enrollment_consumes_invitation_once',
        'bounded_device_capacity_retries_without_discarding_credentials_or_trust',
        'subscribe_before_initial_replay_preserves_the_concurrent_commit_and_wake',
        'chunked_snapshot_and_ordinary_reply_above_64k_and_retention_rollover',
        'interrupted_snapshot_preserves_cursor_and_recovers_after_retention_rollover',
        'native_dead_peer_detection_reconnects_without_tcp_eof_or_pin_reset',
        'slow_viewer_isolation_and_revocation_during_blocked_transfer',
        'revoked_credential_rejected_after_manager_restart',
        'lost_enrollment_response_recovers_with_persisted_credential',
        'same_ca_wrong_leaf_refusal_and_explicit_local_trust_recovery',
        'native_heartbeat_reconnect_notifications_and_atomic_application_cursor',
    },
    'm05-two-vm': {
        'real_native_owner_over_direct_lan',
        'unchanged_production_worker_over_same_public_listener',
        'public_relay_excludes_setup_and_local_administration_with_forwarded_headers',
        'actual_external_bore_data_path_with_unchanged_inner_tls',
        'actual_native_reconnect_and_durable_recovery_through_public_relay',
        'current_and_future_remote_owner_revocation_with_worker_progress',
        'remote_revocation_persists_after_restart',
    },
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(directory):
    files = json.loads((directory / 'files.json').read_text())
    require(bool(files), 'Evidence file manifest is empty.')
    for name, expected in files.items():
        path = pathlib.PurePosixPath(name)
        require(not path.is_absolute() and '..' not in path.parts,
                'Evidence path escapes its directory.')
        actual = hashlib.sha256((directory / name).read_bytes()).hexdigest()
        require(actual == expected, f'Evidence hash changed for {name}.')

    source = json.loads((directory / 'm05-source-manifest.json').read_text())
    require(set(source['production_objects']) == {
        'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'manager/Cargo.toml', 'manager/src',
        'manager/migrations', 'link/Cargo.toml', 'link/src',
        'link/schema/protocol.json', 'link/typescript/protocol.d.ts',
        'agent/Cargo.toml', 'agent/src',
    }, 'Production source manifest has incomplete coverage.')
    clean = subprocess.run(['git', 'diff', '--quiet', 'HEAD', '--',
                            *source['production_objects']], cwd=ROOT)
    require(clean.returncode == 0, 'Production source has uncommitted changes.')
    for name, expected in source['production_objects'].items():
        actual = subprocess.check_output(
            ['git', 'rev-parse', f'HEAD:{name}'], cwd=ROOT, text=True).strip()
        require(actual == expected, f'Tested production source changed at {name}.')

    for name, expected in CHECKS.items():
        summary = json.loads((directory / name / 'summary.json').read_text())
        require(summary['passed'] is True, f'{name} did not pass.')
        checks = summary['checks']
        names = [check if isinstance(check, str) else check['name']
                 for check in checks]
        require(set(names) == expected and len(names) == len(expected),
                f'{name} is missing required acceptance checks.')
        require(not summary.get('cleanup_errors'), f'{name} cleanup failed.')

    remote = json.loads((directory / 'm05-two-vm/summary.json').read_text())
    corrections = json.loads((directory / 'review/client-corrections/summary.json').read_text())
    require(corrections['passed'] is True and len(set(corrections['checks'])) == 2,
            'Native client correction checks did not pass.')
    require(remote['source_head'] == source['production_source_head'],
            'Remote evidence names a different production revision.')
    require(set(remote['shutdown_endpoints']) == {'lan_refuses_tcp', 'relay_refuses_tcp'}
            and all(value is True for value in remote['shutdown_endpoints'].values()),
            'A fixture endpoint remained reachable after shutdown.')
    require(set(remote['binary_sha256']) == {
        'bin/botstral-manager', 'bin/botstral-agent', 'bin/bore', 'bin/owner-probe',
    }, 'Remote binary coverage is incomplete.')
    for name, actual in remote['binary_sha256'].items():
        binary = pathlib.PurePosixPath(name).name
        if binary == 'bore':
            expected = source['bore_binary_sha256']
        elif binary == 'botstral-agent' and remote['legacy_worker']:
            expected = source['binary_sha256']['m04-botstral-agent']
        else:
            expected = source['binary_sha256'][binary]
        require(actual == expected, f'Remote binary identity changed for {name}.')


def controls(directory):
    with tempfile.TemporaryDirectory(prefix='botstral-m05-controls-') as temporary:
        target = pathlib.Path(temporary) / 'evidence'
        for name in ['changed_log', 'failed_acceptance', 'changed_source',
                     'replaced_check', 'missing_endpoint']:
            shutil.copytree(directory, target)
            if name == 'changed_log':
                with (target / 'verify-final.log').open('a') as output:
                    output.write('\nchanged\n')
            else:
                path = (target / 'm05-local-source-final/summary.json'
                        if name in {'failed_acceptance', 'replaced_check'}
                        else target / 'm05-two-vm/summary.json'
                        if name == 'missing_endpoint'
                        else target / 'm05-source-manifest.json')
                value = json.loads(path.read_text())
                if name == 'failed_acceptance':
                    value['passed'] = False
                elif name == 'replaced_check':
                    value['checks'][0] = 'unrelated_check'
                elif name == 'missing_endpoint':
                    value['shutdown_endpoints'] = {}
                else:
                    value['production_objects']['manager/src'] = '0' * 40
                path.write_text(json.dumps(value))
                files = json.loads((target / 'files.json').read_text())
                files[str(path.relative_to(target))] = hashlib.sha256(
                    path.read_bytes()).hexdigest()
                (target / 'files.json').write_text(json.dumps(files))
            try:
                verify(target)
            except ValueError as error:
                print(f'PASS {name} rejected: {error}')
            else:
                raise ValueError(f'Negative control {name} was accepted.')
            shutil.rmtree(target)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=pathlib.Path, default=DEFAULT)
    parser.add_argument('--check-controls', action='store_true')
    args = parser.parse_args()
    verify(args.evidence)
    print('PASS retained M05 acceptance, cleanup, hashes, and production source.')
    if args.check_controls:
        controls(args.evidence)


if __name__ == '__main__':
    main()
