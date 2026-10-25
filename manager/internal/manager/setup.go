package manager

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"time"
)

const setupLifetime = 15 * time.Minute

var errSetupExpired = errors.New("setup activation expired")

type bootstrapCapability struct{ raw [32]byte }

func newCapability() bootstrapCapability {
	var capability bootstrapCapability
	rand.Read(capability.raw[:])
	return capability
}
func (c bootstrapCapability) encode() string   { return base64.RawURLEncoding.EncodeToString(c.raw[:]) }
func (c bootstrapCapability) digest() [32]byte { return sha256.Sum256(c.raw[:]) }

type bootstrapRecord struct {
	BootstrapEndpoint string    `json:"bootstrapEndpoint"`
	Capability        string    `json:"capability"`
	ExpiresAt         time.Time `json:"expiresAt"`
}
type setupActivation struct {
	host           string
	capabilityHash [32]byte
	expiresAt      time.Time
	budget         attemptBudget
	completed      chan struct{}
}

func (m *manager) serveSetup(ctx context.Context, terminal io.Writer) error {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("listen on setup: %w", err)
	}
	defer listener.Close()
	capability := newCapability()
	activation := &setupActivation{host: listener.Addr().String(), capabilityHash: capability.digest(), expiresAt: m.now().Add(setupLifetime).UTC(), completed: make(chan struct{}, 1)}
	server := httpServer(m.setupHandler(ctx, activation))
	done := make(chan error, 1)
	go func() { done <- server.Serve(listener) }()
	if err = json.NewEncoder(terminal).Encode(bootstrapRecord{"http://" + activation.host + "/owner", capability.encode(), activation.expiresAt}); err != nil {
		server.Close()
		<-done
		return fmt.Errorf("write bootstrap record: %w", err)
	}
	timer := time.NewTimer(setupLifetime)
	defer timer.Stop()
	select {
	case <-ctx.Done():
	case <-activation.completed:
	case <-timer.C:
	case err = <-done:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	}
	err = drain(server)
	if errors.Is(err, context.DeadlineExceeded) {
		return nil
	}
	return err
}

func (m *manager) setupHandler(ctx context.Context, activation *setupActivation) http.Handler {
	active := func() error {
		if ctx.Err() != nil || !m.now().Before(activation.expiresAt) {
			return errSetupExpired
		}
		return nil
	}
	return localBoundary(activation.host, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/owner" {
			notFound(w)
			return
		}
		if !allowMethod(w, r, "POST") {
			return
		}
		raw, err := bearerSecret(r)
		var secret [32]byte
		if err == nil {
			secret, err = parseSecret(raw)
		}
		digest := (bootstrapCapability{raw: secret}).digest()
		if err != nil || subtle.ConstantTimeCompare(digest[:], activation.capabilityHash[:]) != 1 {
			problem(w, 403, "forbidden", "The setup capability is invalid.")
			return
		}
		if err = active(); err != nil {
			problem(w, 410, "setup_expired", errSetupExpired.Error())
			return
		}
		if !activation.budget.allow(m.now()) {
			reportError(w, errThrottled)
			return
		}
		if !jsonMediaAllowed(w, r) {
			return
		}
		var input struct {
			Username string `json:"username"`
			Password string `json:"password"`
		}
		if !readJSON(w, r, &input) {
			return
		}
		credentials, err := parseCredentials(input.Username, input.Password)
		if err != nil {
			reportError(w, err)
			return
		}
		if err = active(); err != nil {
			problem(w, 410, "setup_expired", errSetupExpired.Error())
			return
		}
		hash, err := hashPassword(credentials.password)
		if err == nil {
			err = active()
		}
		if err == nil {
			err = m.store.insertOwner(r.Context(), credentials.username, hash, m.now(), active)
		}
		if errors.Is(err, errSetupExpired) {
			problem(w, 410, "setup_expired", errSetupExpired.Error())
			return
		}
		if err != nil {
			reportError(w, err)
			return
		}
		respond(w, 201, struct {
			Owner ownerJSON `json:"owner"`
		}{ownerJSON{credentials.username}})
		if flusher, ok := w.(http.Flusher); ok {
			flusher.Flush()
		}
		select {
		case activation.completed <- struct{}{}:
		default:
		}
	}))
}
