.PHONY: deps dev build test verify
.NOTPARALLEL:

deps:
	go -C manager mod download

dev:
	go -C manager run ./cmd/botstral --data-dir ../data --setup

build:
	go -C manager build -o ../bin/botstral ./cmd/botstral

test:
	go -C manager test -race ./...
	go -C manager vet ./...

verify: test build
	go -C manager run ./cmd/acceptance --binary ../bin/botstral
