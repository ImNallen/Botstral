# Botstral

Botstral is a self-hosted personal assistant project. It aims to match Grok Bot's personal features using Mistral models. Its bots will browse websites, manage files, and write code.

The project is in early planning.

## How it will work

One manager serves one owner and their paired devices. Your bots share a persistent virtual machine, with access to its filesystem and command line.

Each bot has its own screen and conversation. Files, installed tools, and browser logins are shared across bots.

Desktop and mobile apps connect securely to the manager. Tasks continue when you close the app.

Model Context Protocol plugins connect bots to external tools.

## Planned stack

- The manager uses Go and SQLite.
- Desktop and mobile apps use Tauri 2, React, and TypeScript.
- Mistral provides hosted reasoning, and Vibe handles coding.

The manager and shared computer run on your infrastructure. Model inference uses the configured Mistral service.

See [PLAN.md](PLAN.md) for milestones and feature-parity tracking.
