.PHONY: deps dev build test verify
.NOTPARALLEL:

deps:
	go mod download
	npm --prefix web ci

build:
	npm --prefix web run build
	go build -o bin/botstral ./cmd/botstral

dev:
	npm --prefix web run build
	node scripts/dev.mjs

test:
	npm --prefix web run build
	go test -race ./...
	go vet ./...
	npm --prefix web run typecheck

verify: test build
	node scripts/acceptance.mjs
	npm --prefix web run test:e2e
