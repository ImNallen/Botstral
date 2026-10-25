package manager

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"testing/fstest"
	"time"
)

const testOrigin = "http://127.0.0.1:8080"

func testAssets() fstest.MapFS {
	return fstest.MapFS{"index.html": &fstest.MapFile{Data: []byte("<!doctype html><title>test app</title>")}, "assets/app-123.js": &fstest.MapFile{Data: []byte("'use strict';")}}
}

func apiRequest(handler http.Handler, method, path, body string, token *sessionToken, headers map[string]string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, testOrigin+path, strings.NewReader(body))
	if token != nil {
		r.AddCookie(&http.Cookie{Name: cookieName, Value: token.cookie()})
	}
	if method != "GET" {
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("Origin", testOrigin)
		r.Header.Set("X-Botstral-Request", "1")
		if token != nil {
			r.Header.Set("X-CSRF-Token", token.csrf())
		}
	}
	for name, value := range headers {
		if name == "Host" {
			r.Host = value
		} else {
			r.Header.Set(name, value)
		}
	}
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	return w
}

func requireStatus(t *testing.T, response *httptest.ResponseRecorder, status int) {
	t.Helper()
	if response.Code != status {
		t.Fatalf("status %d, want %d: %s", response.Code, status, response.Body.String())
	}
}

func TestAnonymousAndMainRouteIsolation(t *testing.T) {
	m := testManager(t)
	token, value := testSession(t, m)
	name, _ := parseBotName("private research")
	if _, err := m.createBot(t.Context(), token, name); err != nil {
		t.Fatal(err)
	}
	handler := m.mainHandler(testOrigin, testAssets())
	for _, request := range []struct{ method, path, body string }{
		{"GET", "/api/v1/session", ""}, {"GET", "/api/v1/bots", ""}, {"POST", "/api/v1/bots", `{"name":"x"}`},
		{"GET", "/api/v1/sessions", ""}, {"DELETE", "/api/v1/sessions/" + string(value.id), "{}"},
	} {
		response := apiRequest(handler, request.method, request.path, request.body, nil, nil)
		requireStatus(t, response, 401)
		if strings.Contains(response.Body.String(), "private research") || strings.Contains(response.Body.String(), "bots") {
			t.Fatal("anonymous response contains private data")
		}
	}
	for _, path := range []string{"/owner", "/setup", "/setup.js", "/api/v1/setup"} {
		for _, method := range []string{"GET", "POST", "DELETE"} {
			response := apiRequest(handler, method, path, "{}", &token, map[string]string{"Forwarded": "for=127.0.0.1;host=127.0.0.1", "X-Forwarded-For": "127.0.0.1", "X-Forwarded-Host": "127.0.0.1"})
			requireStatus(t, response, 404)
			if strings.Contains(response.Body.String(), "<!doctype") {
				t.Fatal("setup miss served app")
			}
		}
	}
	for _, path := range []string{"/assets/missing.js", "/missing", "/api/v1/unknown", "/api/unknown"} {
		requireStatus(t, apiRequest(handler, "GET", path, "", &token, nil), 404)
	}
	requireStatus(t, apiRequest(handler, "GET", "/", "", nil, nil), 200)
	asset := apiRequest(handler, "GET", "/assets/app-123.js", "", nil, nil)
	requireStatus(t, asset, 200)
	if !strings.Contains(asset.Header().Get("Cache-Control"), "immutable") {
		t.Fatal("hashed asset not cached")
	}
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/health", "{}", nil, nil), 405)
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/health", "", nil, map[string]string{"Host": "evil.example"}), 403)
}

func TestCSRFAndJSONBoundaries(t *testing.T) {
	m := testManager(t)
	token, current := testSession(t, m)
	handler := m.mainHandler(testOrigin, testAssets())
	for _, request := range []struct{ method, path, body string }{
		{"POST", "/api/v1/sign-in", `{"username":"owner","password":"synthetic passphrase"}`},
		{"POST", "/api/v1/bots", `{"name":"forbidden"}`}, {"POST", "/api/v1/sign-out", "{}"},
		{"DELETE", "/api/v1/sessions/" + string(current.id), "{}"},
	} {
		for _, headers := range []map[string]string{{"Origin": ""}, {"Origin": "null"}, {"Origin": "http://evil.example"}, {"X-Botstral-Request": ""}, {"Sec-Fetch-Site": "cross-site"}} {
			requireStatus(t, apiRequest(handler, request.method, request.path, request.body, &token, headers), 403)
		}
	}
	for _, headers := range []map[string]string{{"X-CSRF-Token": ""}, {"X-CSRF-Token": newToken().csrf()}} {
		requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":"forbidden"}`, &token, headers), 403)
	}
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", "name=form", &token, map[string]string{"Content-Type": "application/x-www-form-urlencoded"}), 415)
	for _, body := range []string{`{"name":"ok","extra":true}`, `{"name":"ok"} {}`, `null`, `[]`, `{"name":`, `{"name":3}`} {
		requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", body, &token, nil), 400)
	}
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":"`+strings.Repeat("a", 9000)+`"}`, &token, nil), 413)
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/session", "", &token, nil), 200)
	bots, err := m.listBots(t.Context(), token)
	if err != nil || len(bots) != 0 {
		t.Fatalf("rejected requests mutated bots: %+v %v", bots, err)
	}
}

func TestSessionCookiesRevocationAndSignOut(t *testing.T) {
	m := testManager(t)
	handler := m.mainHandler(testOrigin, testAssets())
	response := apiRequest(handler, "POST", "/api/v1/sign-in", `{"username":"owner","password":"synthetic passphrase"}`, nil, nil)
	requireStatus(t, response, 200)
	cookies := response.Result().Cookies()
	if len(cookies) != 1 {
		t.Fatalf("cookies: %+v", cookies)
	}
	cookie := cookies[0]
	if cookie.Name != cookieName || !cookie.HttpOnly || cookie.SameSite != http.SameSiteStrictMode || cookie.Path != "/api/v1" || cookie.Domain != "" || cookie.Secure || cookie.MaxAge != int(sessionLifetime.Seconds()) {
		t.Fatalf("cookie attributes: %+v", cookie)
	}
	token, err := parseSessionCookie(cookie.Value)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(response.Body.String(), cookie.Value) {
		t.Fatal("bearer token in JSON")
	}
	var view sessionView
	if err = json.Unmarshal(response.Body.Bytes(), &view); err != nil || view.CSRFToken != token.csrf() {
		t.Fatalf("session view: %+v %v", view, err)
	}
	for _, cookie := range []string{"botstral_session=bad", "botstral_session=" + token.cookie() + "; botstral_session=" + token.cookie()} {
		requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", nil, map[string]string{"Cookie": cookie}), 401)
		requireStatus(t, apiRequest(handler, "POST", "/api/v1/sign-in", `{"username":"owner","password":"synthetic passphrase"}`, nil, map[string]string{"Cookie": cookie}), 401)
	}
	replacement, _ := testSession(t, m)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":"forbidden"}`, &replacement, map[string]string{"X-CSRF-Token": token.csrf()}), 403)
	if err := m.signOut(t.Context(), replacement); err != nil {
		t.Fatal(err)
	}
	other, second := testSession(t, m)
	response = apiRequest(handler, "GET", "/api/v1/sessions", "", &token, nil)
	requireStatus(t, response, 200)
	if !strings.Contains(response.Body.String(), `"current":true`) || strings.Contains(response.Body.String(), other.cookie()) {
		t.Fatal("invalid session list")
	}
	requireStatus(t, apiRequest(handler, "DELETE", "/api/v1/sessions/"+string(second.id), "{}", &token, nil), 204)
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &other, nil), 401)
	requireStatus(t, apiRequest(handler, "DELETE", "/api/v1/sessions/"+string(view.Session.ID), "{}", &token, nil), 204)
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &token, nil), 401)
	token, _ = testSession(t, m)
	response = apiRequest(handler, "POST", "/api/v1/sign-out", "{}", &token, nil)
	requireStatus(t, response, 204)
	if response.Result().Cookies()[0].MaxAge != -1 {
		t.Fatal("signout did not clear cookie")
	}
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &token, nil), 401)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/sign-out", "{}", &token, nil), 204)
	if _, err = m.store.db.Exec("DROP TABLE sessions"); err != nil {
		t.Fatal(err)
	}
	response = apiRequest(handler, "POST", "/api/v1/sign-out", "{}", &token, nil)
	requireStatus(t, response, 503)
	if len(response.Result().Cookies()) != 0 {
		t.Fatal("storage failure discarded cookie")
	}
}

func TestGenericLoginAndBudgets(t *testing.T) {
	m := testManager(t)
	handler := m.mainHandler(testOrigin, testAssets())
	var previous string
	for i := range 6 {
		username := "owner"
		if i%2 == 0 {
			username = "unknown"
		}
		response := apiRequest(handler, "POST", "/api/v1/sign-in", fmt.Sprintf(`{"username":%q,"password":"incorrect passphrase"}`, username), nil, nil)
		if i == 5 {
			requireStatus(t, response, 429)
			if response.Header().Get("Retry-After") == "" {
				t.Fatal("missing retry header")
			}
			continue
		}
		requireStatus(t, response, 401)
		if previous != "" && response.Body.String() != previous {
			t.Fatal("login distinguishes unknown user")
		}
		previous = response.Body.String()
	}
	kdfSlots <- struct{}{}
	kdfSlots <- struct{}{}
	_, err := hashPassword("synthetic passphrase")
	<-kdfSlots
	<-kdfSlots
	if err != errThrottled {
		t.Fatalf("KDF concurrency not bounded: %v", err)
	}
}

func TestSetupPolicyAndExpiry(t *testing.T) {
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	m := &manager{store: s, now: time.Now, origin: testOrigin}
	token := newToken()
	activation := &setupActivation{origin: testOrigin, capabilityHash: token.digest(), expiresAt: time.Now().Add(time.Minute), completed: make(chan struct{}, 1)}
	handler := m.setupHandler(t.Context(), activation)
	body := `{"username":"owner","password":"synthetic passphrase","confirmPassword":"synthetic passphrase"}`
	for _, headers := range []map[string]string{{"Origin": ""}, {"Origin": "null"}, {"Origin": "http://evil.example"}, {"Host": "evil.example"}, {"X-Setup-Token": "bad"}, {"Sec-Fetch-Site": "cross-site"}} {
		base := map[string]string{"X-Setup-Token": token.cookie()}
		for k, v := range headers {
			base[k] = v
		}
		requireStatus(t, apiRequest(handler, "POST", "/owner", body, nil, base), 403)
	}
	page := apiRequest(handler, "GET", "/", "", nil, nil)
	requireStatus(t, page, 200)
	if strings.Contains(page.Body.String(), token.cookie()) || !strings.Contains(page.Header().Get("Content-Security-Policy"), "script-src 'self'") {
		t.Fatal("setup capability leaked or script policy missing")
	}
	response := apiRequest(handler, "POST", "/owner", body, nil, map[string]string{"X-Setup-Token": token.cookie()})
	requireStatus(t, response, 201)
	if len(response.Result().Cookies()) != 0 {
		t.Fatal("setup created cookie")
	}
	requireStatus(t, apiRequest(handler, "POST", "/owner", body, nil, map[string]string{"X-Setup-Token": token.cookie()}), 409)
	activation.expiresAt = time.Now().Add(-time.Second)
	requireStatus(t, apiRequest(handler, "POST", "/owner", body, nil, map[string]string{"X-Setup-Token": token.cookie()}), 410)
	main := m.mainHandler(testOrigin, testAssets())
	requireStatus(t, apiRequest(main, "GET", "/setup", "", nil, nil), 404)
}

type terminalLines chan string

func (lines terminalLines) Write(p []byte) (int, error) { lines <- string(p); return len(p), nil }

func TestSetupActualListenerCloses(t *testing.T) {
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	m := &manager{store: s, now: time.Now, origin: testOrigin}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	lines := make(terminalLines, 4)
	done := make(chan error, 1)
	go func() { done <- m.serveSetup(ctx, lines) }()
	var line string
	select {
	case line = <-lines:
	case <-time.After(5 * time.Second):
		t.Fatal("setup did not start")
	}
	fields := strings.Fields(line)
	url := fields[len(fields)-1]
	parts := strings.Split(url, "/#")
	if len(parts) != 2 || !strings.HasPrefix(parts[0], "http://127.0.0.1:") {
		t.Fatalf("setup socket: %s", line)
	}
	request, err := http.NewRequest("POST", parts[0]+"/owner", strings.NewReader(`{"username":"owner","password":"synthetic passphrase","confirmPassword":"synthetic passphrase"}`))
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Origin", parts[0])
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Setup-Token", parts[1])
	response, err := (&http.Client{Timeout: 5 * time.Second}).Do(request)
	if err != nil {
		t.Fatal(err)
	}
	io.Copy(io.Discard, response.Body)
	response.Body.Close()
	if response.StatusCode != 201 {
		t.Fatalf("setup status %d", response.StatusCode)
	}
	select {
	case err = <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("setup listener stayed open")
	}
	connection, err := net.DialTimeout("tcp4", strings.TrimPrefix(parts[0], "http://"), time.Second)
	if err == nil {
		connection.Close()
		t.Fatal("setup accepted after success")
	}
}

func TestUninitializedRunNeverListens(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	defer listener.Close()
	cfg, err := ParseConfig([]string{"--data-dir", t.TempDir(), "--port", fmt.Sprint(port)})
	if err != nil {
		t.Fatal(err)
	}
	err = Run(t.Context(), cfg, testAssets(), io.Discard)
	if err == nil || !strings.Contains(err.Error(), "--setup") || strings.Contains(err.Error(), "address already in use") {
		t.Fatalf("startup did not fail before listen: %v", err)
	}
	for _, args := range [][]string{{"--host", "0.0.0.0"}, {"--port", "65536"}, {"--data-dir", ""}} {
		if _, err = ParseConfig(args); err == nil {
			t.Fatalf("configuration accepted: %v", args)
		}
	}
}

func TestMainIsolationBeforeSetupAndRemoteAddress(t *testing.T) {
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	m := &manager{store: s, now: time.Now}
	handler := m.mainHandler(testOrigin, testAssets())
	for _, path := range []string{"/owner", "/setup", "/setup.js", "/api/v1/setup"} {
		request := httptest.NewRequest("POST", testOrigin+path, strings.NewReader("{}"))
		request.RemoteAddr = "198.51.100.2:1234"
		request.Header.Set("X-Forwarded-For", "127.0.0.1")
		request.Header.Set("X-Forwarded-Host", "127.0.0.1")
		request.Header.Set("Forwarded", "for=127.0.0.1")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		requireStatus(t, response, 404)
	}
	configured, err := s.configured(t.Context())
	if err != nil || configured {
		t.Fatal("main initialized owner")
	}
}

func TestSetupRechecksExpiryAfterHashAndTransaction(t *testing.T) {
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	now := time.Now()
	calls := 0
	m := &manager{store: s, origin: testOrigin, now: func() time.Time {
		calls++
		if calls >= 4 {
			return now.Add(time.Hour)
		}
		return now
	}}
	token := newToken()
	activation := &setupActivation{origin: testOrigin, capabilityHash: token.digest(), expiresAt: now.Add(time.Minute), completed: make(chan struct{}, 1)}
	handler := m.setupHandler(t.Context(), activation)
	body := `{"username":"owner","password":"synthetic passphrase","confirmPassword":"synthetic passphrase"}`
	requireStatus(t, apiRequest(handler, "POST", "/owner", body, nil, map[string]string{"X-Setup-Token": token.cookie()}), 410)
	configured, err := s.configured(t.Context())
	if err != nil || configured {
		t.Fatal("expired hashing created owner")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	err = s.insertOwner(t.Context(), "owner", testHash(t), now, func() error { return ctx.Err() })
	if err != context.Canceled {
		t.Fatalf("setup ignored cancellation after transaction admission: %v", err)
	}
	configured, err = s.configured(t.Context())
	if err != nil || configured {
		t.Fatal("cancelled transaction created owner")
	}
}
