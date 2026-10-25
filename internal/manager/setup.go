package manager

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"html/template"
	"io"
	"net"
	"net/http"
	"time"
)

var errSetupExpired = errors.New("setup activation expired")

type setupActivation struct {
	origin         string
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
	capability := newToken()
	activation := &setupActivation{origin: "http://" + listener.Addr().String(), capabilityHash: capability.digest(), expiresAt: m.now().Add(15 * time.Minute), completed: make(chan struct{}, 1)}
	server := httpServer(m.setupHandler(ctx, activation))
	done := make(chan error, 1)
	go func() { done <- server.Serve(listener) }()
	fmt.Fprintf(terminal, "Setup (expires in 15 minutes): %s/#%s\n", activation.origin, capability.cookie())
	timer := time.NewTimer(15 * time.Minute)
	defer timer.Stop()
	select {
	case <-ctx.Done():
	case <-activation.completed:
	case <-timer.C:
		fmt.Fprintln(terminal, "Setup expired. Restart with --setup to try again.")
	case err = <-done:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	}
	return drain(server)
}

func (m *manager) setupHandler(ctx context.Context, activation *setupActivation) http.Handler {
	active := func() error {
		if ctx.Err() != nil || !m.now().Before(activation.expiresAt) {
			return errSetupExpired
		}
		return nil
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		if !allowMethod(w, r, "GET") {
			return
		}
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		setupPage.Execute(w, nil)
	})
	mux.HandleFunc("/setup.js", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "GET") {
			return
		}
		w.Header().Set("Content-Type", "text/javascript; charset=utf-8")
		io.WriteString(w, setupScript)
	})
	mux.HandleFunc("/owner", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "POST") {
			return
		}
		if r.Header.Get("Origin") != activation.origin || r.Header.Get("Sec-Fetch-Site") == "cross-site" {
			problem(w, 403, "forbidden", "Open the setup link from the manager terminal.")
			return
		}
		if !jsonMediaAllowed(w, r) {
			return
		}
		token, err := parseSessionCookie(r.Header.Get("X-Setup-Token"))
		digest := token.digest()
		if err != nil || subtle.ConstantTimeCompare(digest[:], activation.capabilityHash[:]) != 1 {
			problem(w, 403, "forbidden", "The setup link is invalid.")
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
		var input struct {
			Username        string `json:"username"`
			Password        string `json:"password"`
			ConfirmPassword string `json:"confirmPassword"`
		}
		if !readJSON(w, r, &input) {
			return
		}
		credentials, err := parseCredentials(input.Username, input.Password)
		if err != nil {
			reportError(w, err)
			return
		}
		if input.Password != input.ConfirmPassword {
			reportError(w, inputError("Passwords do not match."))
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
			SignInURL string `json:"signInUrl"`
		}{m.origin})
		select {
		case activation.completed <- struct{}{}:
		default:
		}
	})
	styleHash := sha256.Sum256([]byte(setupStyle))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'sha256-"+base64.StdEncoding.EncodeToString(styleHash[:])+"'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
		if "http://"+r.Host != activation.origin {
			problem(w, 403, "forbidden", "The request host is invalid.")
			return
		}
		if err := active(); err != nil {
			problem(w, 410, "setup_expired", errSetupExpired.Error())
			return
		}
		mux.ServeHTTP(w, r)
	})
}

const setupStyle = `:root{font-family:system-ui,sans-serif;color:#262822;background:#f6f5ef}body{margin:0;padding:48px 24px}main{max-width:420px;margin:5vh auto}h1{font-size:36px;letter-spacing:-1px}p{line-height:1.6;color:#61645b}label{display:block;margin-top:22px;font-weight:600}input{box-sizing:border-box;width:100%;padding:13px;margin-top:8px;border:1px solid #bbbcaf;border-radius:8px;background:white;font:inherit}button{margin-top:28px;padding:14px 20px;border:0;border-radius:8px;background:#a94b2c;color:white;font:inherit;font-weight:600;width:100%}:focus-visible{outline:3px solid #b65d36;outline-offset:3px}button:disabled{opacity:.5}a{color:#914022}#message{min-height:24px}`

var setupPage = template.Must(template.New("setup").Parse(`<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Set up Botstral</title><style>` + setupStyle + `</style><script src="/setup.js" defer></script></head><body><main><p>BOTSTRAL / LOCAL SETUP</p><h1>A place for your bots.</h1><p>Create the owner of this installation. This link expires after 15 minutes. You will sign in separately once setup is complete.</p><form id="owner-form"><label for="username">Username</label><input id="username" name="username" autocomplete="username" minlength="3" maxlength="64" pattern="[a-zA-Z0-9._\-]{3,64}" required><label for="password">Password</label><input id="password" name="password" type="password" autocomplete="new-password" minlength="12" maxlength="256" required aria-describedby="password-help"><p id="password-help">Use 12 to 128 characters. Spaces are welcome.</p><label for="confirm">Confirm password</label><input id="confirm" name="confirmPassword" type="password" autocomplete="new-password" minlength="12" maxlength="256" required><button type="submit" disabled>Create owner</button></form><p id="message" role="status" aria-live="polite">JavaScript and the terminal setup link are required.</p><a id="sign-in" hidden>Continue to sign in</a></main></body></html>`))

const setupScript = `"use strict";
(() => {
  let capability = window.location.hash.slice(1);
  history.replaceState(null, "", window.location.pathname);
  const form = document.getElementById("owner-form");
  const button = form.querySelector("button");
  const message = document.getElementById("message");
  button.disabled = !capability;
  message.textContent = capability ? "Your credentials stay on this manager." : "Open the complete setup link from the manager terminal.";
  form.addEventListener("submit", async event => {
    event.preventDefault();
    button.disabled = true;
    const fields = new FormData(form);
    try {
      const response = await fetch("/owner", {method:"POST", credentials:"omit", headers:{"Content-Type":"application/json", "X-Setup-Token":capability}, body:JSON.stringify({username:fields.get("username"),password:fields.get("password"),confirmPassword:fields.get("confirmPassword")})});
      const result = await response.json();
      if (!response.ok) { message.textContent = result.error.message; button.disabled = false; return; }
      capability = "";
      form.reset();
      form.hidden = true;
      message.textContent = "Owner created. This setup listener is now closing.";
      const link = document.getElementById("sign-in");
      link.href = result.signInUrl;
      link.hidden = false;
      link.focus();
    } catch {
      message.textContent = "Could not reach setup. If the owner was created, sign in on the main manager. Otherwise restart with --setup for a new link.";
    }
  });
})();`
