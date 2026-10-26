# Botstral

Botstral aims to be a self-hosted Grok Bot using Mistral-developed models instead of US models. Its bots will browse websites, manage files, and write code.

The project is in early planning.

## How it will work

One manager serves one owner and their paired devices. It runs in its own virtual machine. Your bots share a separate persistent Computer VM, with access to its filesystem and command line.

Each bot has its own screen and conversation. Files, installed tools, and browser logins are shared across bots.

The desktop app connects securely to the manager. Tasks continue when you close the app. Mobile companions follow after the desktop workflow is stable.

Model Context Protocol plugins connect bots to external tools.

## Planned stack

- The manager and worker use Rust, with SQLite for manager state.
- The desktop app uses Tauri, React, and TypeScript.
- Later mobile apps will use React Native for iOS and Android.
- A TypeScript protocol library supports secure WebSocket connections and can be reused by mobile.
- Mistral provides hosted reasoning, and Vibe handles coding.

The manager and shared computer run on your infrastructure. Model inference uses the configured Mistral service.

See [DESIGN.md](DESIGN.md) for the architecture and [PLAN.md](PLAN.md) for milestones and feature-parity tracking.
