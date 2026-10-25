package main

import (
	"context"
	"fmt"
	"io/fs"
	"os"
	"os/signal"
	"syscall"

	"botstral/internal/manager"
	"botstral/web"
)

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if err := run(ctx); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(ctx context.Context) error {
	cfg, err := manager.ParseConfig(os.Args[1:])
	if err != nil {
		return err
	}
	assets, err := fs.Sub(web.Files, "dist")
	if err != nil {
		return err
	}
	return manager.Run(ctx, cfg, assets, os.Stderr)
}
