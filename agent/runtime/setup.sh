#!/usr/bin/env bash
set -euo pipefail
umask 077
[[ $(uname -sm) == 'Linux x86_64' ]] || { echo 'Linux x86_64 required' >&2; exit 1; }
runtime_root=${1:?Provide a fresh absolute runtime directory.}
[[ $runtime_root == /* && ! -e $runtime_root ]] || exit 1
mkdir -m 700 "$runtime_root"
source_root=$(cd "$(dirname "$0")" && pwd)
cp "$source_root"/{package.json,package-lock.json,run.mjs,seal.py} "$runtime_root/"
cd "$runtime_root"
curl -fL --retry 3 https://nodejs.org/dist/v24.22.1/node-v24.22.1-linux-x64.tar.xz -o node.tar.xz
printf '%s\n' '6c172a9ab6718f7b801fb5a7bd486af2ee5e32758086a04b84fcce2aa77095e6  node.tar.xz' | sha256sum --check
tar xf node.tar.xz
mv node-v24.22.1-linux-x64 node
rm node.tar.xz
export PATH="$runtime_root/node/bin:$PATH"
export PLAYWRIGHT_BROWSERS_PATH="$runtime_root/browsers"
npm ci --ignore-scripts
node node_modules/playwright/cli.js install-deps chromium
node node_modules/playwright/cli.js install chromium --no-remove
python3 seal.py
