package manager

import (
	"bytes"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"log"
	"mime"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"
)

const cookieName = "botstral_session"
const contentPolicy = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"

type sessionJSON struct {
	ID        sessionID `json:"id"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}
type sessionView struct {
	Owner struct {
		Username string `json:"username"`
	} `json:"owner"`
	Session   sessionJSON `json:"session"`
	CSRFToken string      `json:"csrfToken"`
}
type botJSON struct {
	ID        botID     `json:"id"`
	Name      string    `json:"name"`
	CreatedAt time.Time `json:"createdAt"`
}

func sessionResponse(value session, profile owner, token sessionToken) sessionView {
	view := sessionView{Session: sessionJSON{value.id, value.createdAt, value.expiresAt}, CSRFToken: token.csrf()}
	view.Owner.Username = profile.username
	return view
}
func botResponse(value bot) botJSON { return botJSON{value.id, value.name.value, value.createdAt} }

func respond(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if value != nil {
		json.NewEncoder(w).Encode(value)
	}
}

func problem(w http.ResponseWriter, status int, code, message string) {
	type detail struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	}
	respond(w, status, struct {
		Error detail `json:"error"`
	}{detail{code, message}})
}

func reportError(w http.ResponseWriter, err error) {
	var invalid inputError
	switch {
	case errors.As(err, &invalid):
		problem(w, 400, "invalid_input", invalid.Error())
	case errors.Is(err, errCredentials):
		problem(w, 401, "invalid_credentials", errCredentials.Error())
	case errors.Is(err, errUnauthorized):
		problem(w, 401, "unauthorized", errUnauthorized.Error())
	case errors.Is(err, errOwnerExists):
		problem(w, 409, "owner_exists", errOwnerExists.Error())
	case errors.Is(err, errThrottled):
		w.Header().Set("Retry-After", "6")
		problem(w, 429, "throttled", errThrottled.Error())
	case errors.Is(err, errUnavailable):
		problem(w, 503, "unavailable", errUnavailable.Error())
	default:
		id := randomID()
		log.Printf("request %s failed", id)
		w.Header().Set("X-Request-ID", id)
		problem(w, 503, "unavailable", "The manager could not complete this request. Reference: "+id)
	}
}

func readJSON(w http.ResponseWriter, r *http.Request, value any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, 8*1024)
	body, err := io.ReadAll(r.Body)
	if err != nil {
		var limit *http.MaxBytesError
		if errors.As(err, &limit) {
			problem(w, 413, "body_too_large", "The request body exceeds 8 KiB.")
		} else {
			problem(w, 400, "invalid_input", "Could not read the request body.")
		}
		return false
	}
	body = bytes.TrimSpace(body)
	if len(body) == 0 || body[0] != '{' || !utf8.Valid(body) {
		problem(w, 400, "invalid_input", "Send one JSON object with the expected fields.")
		return false
	}
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	err = decoder.Decode(value)
	if err == nil {
		var extra any
		if next := decoder.Decode(&extra); next != io.EOF {
			if next == nil {
				err = inputError("Send one JSON object.")
			} else {
				err = next
			}
		}
	}
	if err == nil {
		return true
	}
	var limit *http.MaxBytesError
	if errors.As(err, &limit) {
		problem(w, 413, "body_too_large", "The request body exceeds 8 KiB.")
	} else {
		problem(w, 400, "invalid_input", "Send one JSON object with the expected fields.")
	}
	return false
}

func mutationAllowed(w http.ResponseWriter, r *http.Request, origin string) bool {
	if r.Header.Get("Origin") != origin || r.Header.Get("Sec-Fetch-Site") == "cross-site" || r.Header.Get("X-Botstral-Request") != "1" {
		problem(w, 403, "forbidden", "This request must come from the manager interface.")
		return false
	}
	return jsonMediaAllowed(w, r)
}

func jsonMediaAllowed(w http.ResponseWriter, r *http.Request) bool {
	media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || media != "application/json" {
		problem(w, 415, "unsupported_media_type", "Use application/json.")
		return false
	}
	return true
}

func requestToken(r *http.Request) (sessionToken, error) {
	var raw string
	count := 0
	for _, cookie := range r.Cookies() {
		if cookie.Name == cookieName {
			raw = cookie.Value
			count++
		}
	}
	if count != 1 {
		return sessionToken{}, errUnauthorized
	}
	return parseSessionCookie(raw)
}

func csrfAllowed(w http.ResponseWriter, r *http.Request, token sessionToken) bool {
	if subtle.ConstantTimeCompare([]byte(r.Header.Get("X-CSRF-Token")), []byte(token.csrf())) != 1 {
		problem(w, 403, "forbidden", "The session request token is invalid. Refresh and try again.")
		return false
	}
	return true
}

func setSessionCookie(w http.ResponseWriter, token sessionToken, value session) {
	http.SetCookie(w, &http.Cookie{Name: cookieName, Value: token.cookie(), Path: "/api/v1", HttpOnly: true, SameSite: http.SameSiteStrictMode, Expires: value.expiresAt, MaxAge: int(sessionLifetime.Seconds())})
}
func clearSessionCookie(w http.ResponseWriter) {
	http.SetCookie(w, &http.Cookie{Name: cookieName, Path: "/api/v1", HttpOnly: true, SameSite: http.SameSiteStrictMode, Expires: time.Unix(1, 0).UTC(), MaxAge: -1})
}

func allowMethod(w http.ResponseWriter, r *http.Request, methods ...string) bool {
	for _, method := range methods {
		if r.Method == method {
			return true
		}
	}
	w.Header().Set("Allow", strings.Join(methods, ", "))
	problem(w, 405, "method_not_allowed", "This method is not supported.")
	return false
}

func (m *manager) mainHandler(origin string, assets fs.FS) http.Handler {
	u, _ := url.Parse(origin)
	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/health", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "GET") {
			return
		}
		if err := m.store.db.PingContext(r.Context()); err != nil {
			reportError(w, errUnavailable)
			return
		}
		respond(w, 200, struct {
			Status string `json:"status"`
		}{"ok"})
	})
	mux.HandleFunc("/api/v1/sign-in", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "POST") || !mutationAllowed(w, r, origin) {
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
		var previous *sessionToken
		if token, err := requestToken(r); err == nil {
			previous = &token
		} else if len(r.CookiesNamed(cookieName)) > 0 {
			clearSessionCookie(w)
			reportError(w, errUnauthorized)
			return
		}
		token, value, profile, err := m.signIn(r.Context(), credentials, previous)
		if err != nil {
			reportError(w, err)
			return
		}
		setSessionCookie(w, token, value)
		respond(w, 200, sessionResponse(value, profile, token))
	})
	mux.HandleFunc("/api/v1/sign-out", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "POST") || !mutationAllowed(w, r, origin) {
			return
		}
		token, err := requestToken(r)
		if err == nil {
			_, _, err = m.currentSession(r.Context(), token)
		}
		if err != nil && !errors.Is(err, errUnauthorized) {
			reportError(w, err)
			return
		}
		valid := err == nil
		if valid && !csrfAllowed(w, r, token) {
			return
		}
		var input struct{}
		if !readJSON(w, r, &input) {
			return
		}
		if valid {
			err = m.signOut(r.Context(), token)
			if err != nil && !errors.Is(err, errUnauthorized) {
				reportError(w, err)
				return
			}
		}
		clearSessionCookie(w)
		w.WriteHeader(204)
	})
	protected := func(w http.ResponseWriter, r *http.Request) (sessionToken, session, owner, bool) {
		token, err := requestToken(r)
		var value session
		var profile owner
		if err == nil {
			value, profile, err = m.currentSession(r.Context(), token)
		}
		if err != nil {
			reportError(w, err)
			return token, value, profile, false
		}
		if r.Method != "GET" && (!mutationAllowed(w, r, origin) || !csrfAllowed(w, r, token)) {
			return token, value, profile, false
		}
		return token, value, profile, true
	}
	mux.HandleFunc("/api/v1/session", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "GET") {
			return
		}
		token, value, profile, ok := protected(w, r)
		if ok {
			respond(w, 200, sessionResponse(value, profile, token))
		}
	})
	mux.HandleFunc("/api/v1/bots", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "GET", "POST") {
			return
		}
		token, _, _, ok := protected(w, r)
		if !ok {
			return
		}
		if r.Method == "GET" {
			values, err := m.listBots(r.Context(), token)
			if err != nil {
				reportError(w, err)
				return
			}
			bots := make([]botJSON, 0, len(values))
			for _, value := range values {
				bots = append(bots, botResponse(value))
			}
			respond(w, 200, struct {
				Bots []botJSON `json:"bots"`
			}{bots})
			return
		}
		var input struct {
			Name string `json:"name"`
		}
		if !readJSON(w, r, &input) {
			return
		}
		name, err := parseBotName(input.Name)
		if err != nil {
			reportError(w, err)
			return
		}
		value, err := m.createBot(r.Context(), token, name)
		if err != nil {
			reportError(w, err)
			return
		}
		respond(w, 201, botResponse(value))
	})
	mux.HandleFunc("/api/v1/sessions", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "GET") {
			return
		}
		token, current, _, ok := protected(w, r)
		if !ok {
			return
		}
		values, err := m.listSessions(r.Context(), token)
		if err != nil {
			reportError(w, err)
			return
		}
		type item struct {
			sessionJSON
			Current bool `json:"current"`
		}
		sessions := make([]item, 0, len(values))
		for _, value := range values {
			sessions = append(sessions, item{sessionJSON{value.id, value.createdAt, value.expiresAt}, value.id == current.id})
		}
		respond(w, 200, struct {
			Sessions []item `json:"sessions"`
		}{sessions})
	})
	mux.HandleFunc("/api/v1/sessions/{id}", func(w http.ResponseWriter, r *http.Request) {
		if !allowMethod(w, r, "DELETE") {
			return
		}
		token, current, _, ok := protected(w, r)
		if !ok {
			return
		}
		id := r.PathValue("id")
		if len(id) != 32 || strings.ContainsAny(id, "ABCDEFGHIJKLMNOPQRSTUVWXYZ") {
			reportError(w, inputError("Invalid session identifier."))
			return
		}
		for _, c := range id {
			if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
				reportError(w, inputError("Invalid session identifier."))
				return
			}
		}
		var input struct{}
		if !readJSON(w, r, &input) {
			return
		}
		if err := m.revokeSession(r.Context(), token, sessionID(id)); err != nil {
			reportError(w, err)
			return
		}
		if current.id == sessionID(id) {
			clearSessionCookie(w)
		}
		w.WriteHeader(204)
	})
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		path := r.URL.Path
		if strings.HasPrefix(path, "/api/") || path == "/api" {
			problem(w, 404, "not_found", "This API route does not exist.")
			return
		}
		if path == "/owner" || path == "/setup" || path == "/setup.js" {
			http.NotFound(w, r)
			return
		}
		if !allowMethod(w, r, "GET", "HEAD") {
			return
		}
		name := strings.TrimPrefix(path, "/")
		if path == "/" {
			name = "index.html"
		}
		if !fs.ValidPath(name) || strings.HasSuffix(path, "/") && path != "/" || strings.Contains(name, "/") && !strings.HasPrefix(name, "assets/") {
			http.NotFound(w, r)
			return
		}
		body, err := fs.ReadFile(assets, name)
		if err != nil {
			http.NotFound(w, r)
			return
		}
		if strings.HasPrefix(name, "assets/") {
			w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
		} else {
			w.Header().Set("Cache-Control", "no-store")
		}
		http.ServeContent(w, r, name, time.Time{}, bytes.NewReader(body))
	})
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Content-Security-Policy", contentPolicy)
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("Cache-Control", "no-store")
		if r.Host != u.Host {
			problem(w, 403, "forbidden", "The request host is invalid.")
			return
		}
		mux.ServeHTTP(w, r)
	})
}
