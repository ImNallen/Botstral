#!/usr/bin/env python3
import argparse
import asyncio
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


async def compact(prompt, workspace):
    from vibe.core.agent_loop import AgentLoop
    from vibe.core.config import build_default_orchestrator
    from vibe.core.config.harness_files import init_harness_files_manager
    from vibe.core.prompts import UtilityPrompt
    from vibe.core.types import LLMMessage, Role
    import vibe

    home = Path(os.environ['VIBE_HOME'])
    evidence = {'trigger': 'explicit_agent_loop_compact', 'automatic_cli_trigger': False,
                'runtime_version': importlib.metadata.version('mistral-vibe'),
                'driver_sha256': hashlib.sha256(Path(__file__).read_bytes().split(b'\n', 1)[1]).hexdigest(),
                'requests': [], 'responses': []}
    package = Path(vibe.__file__).parent
    names = ('core/agent_loop/_loop.py', 'core/compaction/manager.py', 'core/compaction/context.py',
             'core/prompts/compact.md', 'core/prompts/compact_system.md', 'core/prompts/compact_summary_prefix.md',
             'core/llm/backend/generic.py', 'core/utils/retry.py', 'core/middleware.py')
    evidence['source_digests'] = {name: digest(package / name) for name in names}
    init_harness_files_manager('user')
    orchestrator = await build_default_orchestrator()
    loop = AgentLoop(orchestrator, agent_name='managed', cwd=workspace, headless=True, max_tokens=1024)
    try:
        await loop.wait_until_ready()
        model = loop.config.get_compaction_model()
        provider = loop.config.get_provider_for_model(model)
        evidence['configuration'] = {'model': model.name, 'alias': model.alias, 'provider': model.provider,
            'active_model': loop.config.get_active_model().name, 'backend': provider.backend,
            'api_style': provider.api_style, 'compaction_prompt': loop.config.compaction_prompt,
            'fallback_prompt': UtilityPrompt.COMPACT_SYSTEM.read()}
        client = loop.backend._get_client()

        async def request_hook(request):
            evidence['requests'].append({'body': request.content.decode(), 'sha256': hashlib.sha256(request.content).hexdigest(),
                                         'path': request.url.path})

        async def response_hook(response):
            await response.aread()
            evidence['responses'].append({'status': response.status_code, 'body': response.content.decode(),
                                          'sha256': hashlib.sha256(response.content).hexdigest()})

        client.event_hooks['request'].append(request_hook)
        client.event_hooks['response'].append(response_hook)
        loop.messages.append(LLMMessage(role=Role.user, content=prompt))
        await loop._save_messages()
        evidence['native_session'] = loop.session_id
        evidence['before'] = [m.model_dump(mode='json') for m in loop.messages]
        evidence['before_files'] = native_files(home)
        evidence['before_file_digests'] = native_digests(home)
        evidence['started_at_ns'] = time.time_ns()
        evidence['summary'] = await loop.compact()
        evidence['finished_at_ns'] = time.time_ns()
        evidence['after'] = [m.model_dump(mode='json') for m in loop.messages]
        evidence['after_files'] = native_files(home)
        evidence['after_file_digests'] = native_digests(home)
        evidence['context_tokens_after'] = loop.stats.context_tokens
        evidence['process'] = {'pid': os.getpid(), 'parent': os.getppid(), 'cgroup': Path('/proc/self/cgroup').read_text()}
    finally:
        await loop.aclose()
        (home / 'compaction-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    until = time.monotonic() + 90
    while not (home / 'compaction-release').exists():
        if time.monotonic() >= until:
            raise TimeoutError('compaction evidence release')
        await asyncio.sleep(.1)
    print('[]')


def native_files(home):
    files = {}
    for path in sorted((home / 'logs/session').rglob('*')):
        if path.is_file() and path.suffix in ('.json', '.jsonl') and path.stat().st_size <= 1048576:
            text = path.read_text()
            if path.name == 'meta.json':
                metadata = json.loads(text)
                for provider in metadata['config']['providers']:
                    for name in provider.get('extra_headers', {}):
                        if name.lower() == 'authorization':
                            provider['extra_headers'][name] = '<relay-token-redacted>'
                text = json.dumps(metadata, sort_keys=True)
            files[str(path.relative_to(home))] = text
    return files


def native_digests(home):
    return {str(path.relative_to(home)): digest(path) for path in sorted((home / 'logs/session').rglob('*'))
            if path.is_file() and path.suffix in ('.json', '.jsonl') and path.stat().st_size <= 1048576}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--prompt', required=True)
    parser.add_argument('--workdir', type=Path, required=True)
    args, _ = parser.parse_known_args()
    asyncio.run(compact(args.prompt, args.workdir))
