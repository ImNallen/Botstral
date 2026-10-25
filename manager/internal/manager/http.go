package manager

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"log"
	"mime"
	"net/http"
	"path"
	"strings"
	"time"
	"unicode/utf8"
)

type sessionJSON struct {
	ID        sessionID `json:"id"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}
type ownerJSON struct {
	Username string `json:"username"`
}
type sessionView struct {
	Owner   ownerJSON   `json:"owner"`
	Session sessionJSON `json:"session"`
}
type signInView struct {
	Token string `json:"token"`
	sessionView
}
type botJSON struct {
	ID        botID     `json:"id"`
	Name      string    `json:"name"`
	CreatedAt time.Time `json:"createdAt"`
}

func sessionResponse(value session, profile owner) sessionView {
	return sessionView{ownerJSON{profile.username}, sessionJSON{value.id, value.createdAt, value.expiresAt}}
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
	problem(w, 400, "invalid_input", "Send one JSON object with the expected fields.")
	return false
}

func jsonMediaAllowed(w http.ResponseWriter, r *http.Request) bool {
	media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || media != "application/json" {
		problem(w, 415, "unsupported_media_type", "Use application/json.")
		return false
	}
	return true
}

func headerValues(r *http.Request, name string) ([]string, bool) {
	var values []string
	present := false
	for key, entries := range r.Header {
		if strings.EqualFold(key, name) {
			present = true
			values = append(values, entries...)
		}
	}
	return values, present
}

func bearerSecret(r *http.Request) (string, error) {
	values, _ := headerValues(r, "Authorization")
	if len(values) != 1 {
		return "", errUnauthorized
	}
	scheme, secret, ok := strings.Cut(values[0], " ")
	if !ok || !strings.EqualFold(scheme, "Bearer") {
		return "", errUnauthorized
	}
	return secret, nil
}

func requestToken(r *http.Request) (sessionToken, error) {
	raw, err := bearerSecret(r)
	if err != nil {
		return sessionToken{}, err
	}
	return parseSessionToken(raw)
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

func emptyBodyAllowed(w http.ResponseWriter, r *http.Request) bool {
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 8*1024))
	var limit *http.MaxBytesError
	if errors.As(err, &limit) {
		problem(w, 413, "body_too_large", "The request body exceeds 8 KiB.")
		return false
	}
	if err != nil || len(body) != 0 {
		problem(w, 400, "invalid_input", "This request takes no body.")
		return false
	}
	return true
}

func localBoundary(host string, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		if _, present := headerValues(r, "Origin"); present {
			problem(w, 403, "forbidden", "Origin headers are not allowed.")
			return
		}
		if r.Host != host {
			problem(w, 403, "forbidden", "The request host is invalid.")
			return
		}
		// Route directly so escaped or unclean paths never acquire ServeMux redirects.
		if r.URL.EscapedPath() != r.URL.Path || path.Clean(r.URL.Path) != r.URL.Path {
			notFound(w)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func notFound(w http.ResponseWriter) {
	problem(w, 404, "not_found", "This API route does not exist.")
}

func (m *manager) mainHandler(host string) http.Handler {
	return localBoundary(host, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1/health":
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
		case "/api/v1/sign-in":
			if !allowMethod(w, r, "POST") {
				return
			}
			if _, present := headerValues(r, "Authorization"); present {
				problem(w, 400, "invalid_request", "Sign-in takes no Authorization header.")
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
			token, value, profile, err := m.signIn(r.Context(), credentials)
			if err != nil {
				reportError(w, err)
				return
			}
			respond(w, 200, signInView{token.encode(), sessionResponse(value, profile)})
		case "/api/v1/session":
			if !allowMethod(w, r, "GET") {
				return
			}
			token, err := requestToken(r)
			if err != nil {
				reportError(w, err)
				return
			}
			value, profile, err := m.currentSession(r.Context(), token)
			if err != nil {
				reportError(w, err)
				return
			}
			respond(w, 200, sessionResponse(value, profile))
		case "/api/v1/sign-out":
			if !allowMethod(w, r, "POST") {
				return
			}
			token, ok := m.authorizedToken(w, r)
			if !ok || !emptyBodyAllowed(w, r) {
				return
			}
			if err := m.signOut(r.Context(), token); err != nil {
				reportError(w, err)
				return
			}
			w.WriteHeader(204)
		case "/api/v1/bots":
			if !allowMethod(w, r, "GET", "POST") {
				return
			}
			token, ok := m.authorizedToken(w, r)
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
			if !jsonMediaAllowed(w, r) {
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
		case "/api/v1/sessions":
			if !allowMethod(w, r, "GET") {
				return
			}
			token, err := requestToken(r)
			if err != nil {
				reportError(w, err)
				return
			}
			values, err := m.listSessions(r.Context(), token)
			if err != nil {
				reportError(w, err)
				return
			}
			sessions := make([]sessionJSON, 0, len(values))
			for _, value := range values {
				sessions = append(sessions, sessionJSON{value.id, value.createdAt, value.expiresAt})
			}
			respond(w, 200, struct {
				Sessions []sessionJSON `json:"sessions"`
			}{sessions})
		default:
			id, ok := strings.CutPrefix(r.URL.Path, "/api/v1/sessions/")
			if !ok || id == "" || strings.Contains(id, "/") {
				notFound(w)
				return
			}
			if !allowMethod(w, r, "DELETE") {
				return
			}
			token, ok := m.authorizedToken(w, r)
			if !ok {
				return
			}
			if len(id) != 32 || strings.Trim(id, "0123456789abcdef") != "" {
				reportError(w, inputError("Invalid session identifier."))
				return
			}
			if !emptyBodyAllowed(w, r) {
				return
			}
			if err := m.revokeSession(r.Context(), token, sessionID(id)); err != nil {
				reportError(w, err)
				return
			}
			w.WriteHeader(204)
		}
	}))
}

func (m *manager) authorizedToken(w http.ResponseWriter, r *http.Request) (sessionToken, bool) {
	token, err := requestToken(r)
	if err == nil {
		_, _, err = m.currentSession(r.Context(), token)
	}
	if err != nil {
		reportError(w, err)
		return sessionToken{}, false
	}
	return token, true
}
