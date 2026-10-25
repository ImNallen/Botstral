package manager

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"io/fs"
	"net"
	"net/http"
	"os"
	"strconv"
	"time"
)

type Config struct {
	dataDir   string
	port      uint16
	setup     bool
	devAssets string
}

func ParseConfig(args []string) (Config, error) {
	var cfg Config
	flags := flag.NewFlagSet("botstral", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	flags.StringVar(&cfg.dataDir, "data-dir", "./data", "persistent data directory")
	port := flags.Int("port", 8080, "loopback HTTP port, or 0 for an ephemeral port")
	flags.BoolVar(&cfg.setup, "setup", false, "open temporary local owner setup when unconfigured")
	flags.StringVar(&cfg.devAssets, "dev-assets", "", "serve a built frontend directory during development")
	if err := flags.Parse(args); err != nil {
		return cfg, err
	}
	if flags.NArg() != 0 {
		return cfg, errors.New("unexpected positional arguments")
	}
	if cfg.dataDir == "" {
		return cfg, errors.New("--data-dir must be a persistent directory")
	}
	if *port < 0 || *port > 65535 {
		return cfg, errors.New("--port must be between 0 and 65535")
	}
	cfg.port = uint16(*port)
	return cfg, nil
}

func httpServer(handler http.Handler) *http.Server {
	return &http.Server{Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second, WriteTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16 * 1024}
}

func drain(server *http.Server) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := server.Shutdown(ctx)
	if err != nil {
		server.Close()
	}
	return err
}

func Run(ctx context.Context, cfg Config, assets fs.FS, terminal io.Writer) error {
	if cfg.dataDir == "" {
		return errors.New("configuration must come from ParseConfig")
	}
	if cfg.devAssets != "" {
		assets = os.DirFS(cfg.devAssets)
	}
	if _, err := fs.Stat(assets, "index.html"); err != nil {
		return fmt.Errorf("built frontend is missing: %w", err)
	}
	s, err := openStore(ctx, cfg.dataDir)
	if err != nil {
		return err
	}
	defer s.db.Close()
	configured, err := s.configured(ctx)
	if err != nil {
		return err
	}
	if !configured && !cfg.setup {
		return fmt.Errorf("owner is not configured; run: botstral --data-dir %s --port %d --setup", strconv.Quote(cfg.dataDir), cfg.port)
	}
	m, err := newManager(s)
	if err != nil {
		return err
	}
	listener, err := net.Listen("tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(cfg.port))))
	if err != nil {
		return fmt.Errorf("listen on main: %w", err)
	}
	m.origin = "http://" + listener.Addr().String()
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	server := httpServer(m.mainHandler(m.origin, assets))
	mainDone := make(chan error, 1)
	go func() { mainDone <- server.Serve(listener) }()
	fmt.Fprintf(terminal, "Manager: %s\n", m.origin)
	var setupDone <-chan error
	if !configured {
		done := make(chan error, 1)
		setupDone = done
		go func() { done <- m.serveSetup(ctx, terminal) }()
	} else if cfg.setup {
		fmt.Fprintln(terminal, "Owner already configured. Setup remains closed.")
	}
	var runErr error
wait:
	for {
		select {
		case <-ctx.Done():
			break wait
		case err = <-mainDone:
			if !errors.Is(err, http.ErrServerClosed) {
				runErr = err
			}
			break wait
		case err = <-setupDone:
			setupDone = nil
			if err != nil {
				runErr = err
				break wait
			}
		}
	}
	cancel()
	if err = drain(server); runErr == nil {
		runErr = err
	}
	if setupDone != nil {
		if err = <-setupDone; runErr == nil {
			runErr = err
		}
	}
	return runErr
}
