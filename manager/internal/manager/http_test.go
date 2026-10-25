package manager

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const testHost = "127.0.0.1:8080"
const testEndpoint = "http://" + testHost
const testCredentials = `{"username":"owner","password":"synthetic passphrase"}`

func apiRequest(handler http.Handler, method, path, body string, token *sessionToken, headers map[string]string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, testEndpoint+path, strings.NewReader(body))
	if token != nil {
		r.Header.Set("Authorization", "Bearer "+token.encode())
	}
	if body != "" {
		r.Header.Set("Content-Type", "application/json")
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
		t.Fatalf("status %d, want %d", response.Code, status)
	}
	if len(response.Result().Cookies()) != 0 {
		t.Fatal("API issued cookies")
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("response permits caching")
	}
	if response.Code != 204 {
		if response.Header().Get("Content-Type") != "application/json" || !json.Valid(response.Body.Bytes()) {
			t.Fatal("response is not JSON")
		}
	}
	if response.Header().Get("Location") != "" {
		t.Fatal("API redirected")
	}
	for key := range response.Header() {
		if strings.HasPrefix(key, "Access-Control-") {
			t.Fatal("API issued CORS permissions")
		}
	}
}

func TestAnonymousAndMainRouteIsolation(t *testing.T) {
	m := testManager(t)
	token, value := testSession(t, m)
	name, _ := parseBotName("private research")
	if _, err := m.createBot(t.Context(), token, name); err != nil {
		t.Fatal(err)
	}
	handler := m.mainHandler(testHost)
	for _, request := range []struct{ method, path, body string }{
		{"GET", "/api/v1/session", ""}, {"GET", "/api/v1/bots", ""}, {"POST", "/api/v1/bots", `invalid JSON`},
		{"GET", "/api/v1/sessions", ""}, {"DELETE", "/api/v1/sessions/" + string(value.id), ""}, {"POST", "/api/v1/sign-out", ""},
	} {
		response := apiRequest(handler, request.method, request.path, request.body, nil, nil)
		requireStatus(t, response, 401)
		if strings.Contains(response.Body.String(), "private research") || strings.Contains(response.Body.String(), "bots") {
			t.Fatal("anonymous response contains private data")
		}
	}
	for _, path := range []string{"/", "/index.html", "/assets/app.js", "/owner", "/setup", "/setup.js", "/api/v1/setup", "/api/v1/unknown", "/api//v1/bots", "/api/v1/../v1/bots", "/api/v1/bots/", "/api/v1/%62ots", "/%6fwner", "/api/v1/sessions/..", "/api/v1/sessions/%2e%2e", "/api/v1/sessions//"} {
		for _, method := range []string{"GET", "POST", "DELETE"} {
			requireStatus(t, apiRequest(handler, method, path, "{}", &token, map[string]string{"Forwarded": "for=127.0.0.1;host=127.0.0.1", "X-Forwarded-For": "127.0.0.1", "X-Forwarded-Host": "127.0.0.1"}), 404)
		}
	}
	response := apiRequest(handler, "GET", "/api/v1/health", "", nil, nil)
	requireStatus(t, response, 200)
	if strings.TrimSpace(response.Body.String()) != `{"status":"ok"}` {
		t.Fatal("health exposes additional state")
	}
	for _, request := range []struct{ path, method, allow string }{
		{"/api/v1/health", "POST", "GET"}, {"/api/v1/bots", "PUT", "GET, POST"}, {"/api/v1/sign-in", "GET", "POST"},
		{"/api/v1/session", "POST", "GET"}, {"/api/v1/sign-out", "GET", "POST"}, {"/api/v1/sessions", "POST", "GET"},
		{"/api/v1/sessions/" + string(value.id), "POST", "DELETE"}, {"/api/v1/bots", "OPTIONS", "GET, POST"},
	} {
		response := apiRequest(handler, request.method, request.path, "", nil, nil)
		requireStatus(t, response, 405)
		if response.Header().Get("Allow") != request.allow {
			t.Fatal("incorrect allowed methods")
		}
	}
}

func TestNativeRequestPolicyAndJSONBoundaries(t *testing.T) {
	m := testManager(t)
	token, current := testSession(t, m)
	handler := m.mainHandler(testHost)
	for _, request := range []struct{ method, path, body string }{
		{"GET", "/api/v1/health", ""}, {"POST", "/api/v1/sign-in", testCredentials}, {"GET", "/api/v1/bots", ""},
		{"POST", "/api/v1/bots", `{"name":"forbidden"}`}, {"POST", "/api/v1/sign-out", ""}, {"DELETE", "/api/v1/sessions/" + string(current.id), ""},
	} {
		for _, headers := range []map[string]string{{"Origin": ""}, {"Origin": "null"}, {"Origin": testEndpoint}, {"Origin": "http://evil.example"}, {"Host": "localhost:8080"}, {"Host": "127.0.0.1"}, {"Host": "evil.example"}} {
			headers["Forwarded"] = "for=127.0.0.1;host=" + testHost
			headers["X-Forwarded-Host"] = testHost
			requireStatus(t, apiRequest(handler, request.method, request.path, request.body, &token, headers), 403)
		}
	}
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &token, map[string]string{"Sec-Fetch-Site": "cross-site", "Forwarded": "for=198.51.100.2", "X-Forwarded-Host": "evil.example"}), 200)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", "name=form", &token, map[string]string{"Content-Type": "application/x-www-form-urlencoded"}), 415)
	for _, body := range []string{`{"name":"ok","extra":true}`, `{"name":"ok"} {}`, `null`, `[]`, `{"name":`, `{"name":3}`, "{\"name\":\"\xff\"}", `{"name":"bad\nname"}`} {
		requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", body, &token, nil), 400)
	}
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":"`+strings.Repeat("a", 9000)+`"}`, &token, nil), 413)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", strings.Repeat(" ", 8192)+`{}`, nil, nil), 401)
	for _, route := range []struct{ method, path string }{{"POST", "/api/v1/sign-out"}, {"DELETE", "/api/v1/sessions/" + string(current.id)}} {
		requireStatus(t, apiRequest(handler, route.method, route.path, "{}", &token, nil), 400)
		requireStatus(t, apiRequest(handler, route.method, route.path, strings.Repeat(" ", 8193), &token, nil), 413)
	}
	bots, err := m.listBots(t.Context(), token)
	if err != nil || len(bots) != 0 {
		t.Fatal("rejected requests mutated bots")
	}
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":" Research "}`, &token, nil), 201)
	response := apiRequest(handler, "GET", "/api/v1/bots", "", &token, nil)
	requireStatus(t, response, 200)
	var result struct {
		Bots []botJSON `json:"bots"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &result); err != nil || len(result.Bots) != 1 || result.Bots[0].Name != "Research" {
		t.Fatal("bot response differs")
	}
}

func TestStrictBearerAuthentication(t *testing.T) {
	m := testManager(t)
	token, _ := testSession(t, m)
	handler := m.mainHandler(testHost)
	raw := token.encode()
	for _, authorization := range []string{"", "Basic " + raw, "Bearer", "Bearer ", "Bearer  " + raw, "Bearer\t" + raw, "Bearer " + raw + "=", "Bearer " + raw + ", Bearer " + raw, "Bearer " + raw[:42], "Bearer " + base64.RawURLEncoding.EncodeToString(make([]byte, 33)), "Bearer " + raw + "\n"} {
		requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", nil, map[string]string{"Authorization": authorization}), 401)
	}
	alphabet := "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
	index := strings.IndexByte(alphabet, raw[42])
	noncanonical := raw[:42] + string(alphabet[index+1])
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", nil, map[string]string{"Authorization": "Bearer " + noncanonical}), 401)
	for _, scheme := range []string{"Bearer", "bearer", "BEARER", "bEaReR"} {
		requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", nil, map[string]string{"Authorization": scheme + " " + raw}), 200)
	}
	for _, values := range [][]string{{"Bearer " + raw, "Bearer " + raw}, {"", "Bearer " + raw}} {
		r := httptest.NewRequest("GET", testEndpoint+"/api/v1/bots", nil)
		r.Header["Authorization"] = values
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		requireStatus(t, w, 401)
	}
	for _, query := range []string{"?token=" + raw, "?access_token=" + raw} {
		requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots"+query, "", nil, map[string]string{"Cookie": "botstral_session=" + raw}), 401)
	}
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", nil, map[string]string{"Cookie": "botstral_session=" + raw}), 401)
	for _, auth := range []string{"", "Bearer " + raw} {
		response := apiRequest(handler, "POST", "/api/v1/sign-in", testCredentials, nil, map[string]string{"Authorization": auth})
		requireStatus(t, response, 400)
		if !strings.Contains(response.Body.String(), `"code":"invalid_request"`) {
			t.Fatal("sign-in accepted previous-token contract")
		}
	}
	now := time.Now()
	m.now = func() time.Time { return now.Add(sessionLifetime) }
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &token, nil), 401)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", "invalid", &token, nil), 401)
}

func TestSessionsReturnSecretsOnceAndRevokeExplicitly(t *testing.T) {
	m := testManager(t)
	handler := m.mainHandler(testHost)
	var logs bytes.Buffer
	old := log.Writer()
	log.SetOutput(&logs)
	defer log.SetOutput(old)
	response := apiRequest(handler, "POST", "/api/v1/sign-in", testCredentials, nil, map[string]string{"Cookie": "botstral_session=obsolete"})
	requireStatus(t, response, 200)
	var first signInView
	if err := json.Unmarshal(response.Body.Bytes(), &first); err != nil {
		t.Fatal("invalid sign-in response")
	}
	token, err := parseSessionToken(first.Token)
	if err != nil || first.Owner.Username != "owner" || first.Session.ExpiresAt.Sub(first.Session.CreatedAt) != sessionLifetime {
		t.Fatal("invalid sign-in fields")
	}
	response = apiRequest(handler, "POST", "/api/v1/sign-in", testCredentials, nil, map[string]string{"Cookie": "botstral_session=" + first.Token})
	requireStatus(t, response, 200)
	var second signInView
	if err := json.Unmarshal(response.Body.Bytes(), &second); err != nil || second.Token == first.Token || second.Session.ID == first.Session.ID {
		t.Fatal("sign-ins are not independent")
	}
	other, err := parseSessionToken(second.Token)
	if err != nil {
		t.Fatal("invalid second token")
	}
	for _, path := range []string{"/api/v1/session", "/api/v1/sessions"} {
		response := apiRequest(handler, "GET", path, "", &token, nil)
		requireStatus(t, response, 200)
		for _, secret := range []string{first.Token, second.Token, "token", "csrf", "current"} {
			if strings.Contains(response.Body.String(), secret) {
				t.Fatal("metadata contains a secret or obsolete field")
			}
		}
	}
	response = apiRequest(handler, "GET", "/api/v1/sessions", "", &token, nil)
	var sessions struct {
		Sessions []sessionJSON `json:"sessions"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &sessions); err != nil || len(sessions.Sessions) != 2 {
		t.Fatal("session list differs")
	}
	if sessions.Sessions[0].CreatedAt.Equal(sessions.Sessions[1].CreatedAt) && sessions.Sessions[0].ID > sessions.Sessions[1].ID {
		t.Fatal("sessions not ordered by identifier")
	}
	for range 2 {
		requireStatus(t, apiRequest(handler, "DELETE", "/api/v1/sessions/"+string(second.Session.ID), "", &token, nil), 204)
	}
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &other, nil), 401)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/bots", `{"name":"forbidden"}`, &other, nil), 401)
	requireStatus(t, apiRequest(handler, "DELETE", "/api/v1/sessions/"+string(first.Session.ID), "", &token, nil), 204)
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/bots", "", &token, nil), 401)
	token, _ = testSession(t, m)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/sign-out", "", &token, nil), 204)
	requireStatus(t, apiRequest(handler, "POST", "/api/v1/sign-out", "", &token, nil), 401)
	if _, err := m.store.db.Exec("DROP TABLE sessions"); err != nil {
		t.Fatal(err)
	}
	requireStatus(t, apiRequest(handler, "GET", "/api/v1/session", "", &token, nil), 503)
	if strings.Contains(logs.String(), first.Token) || strings.Contains(logs.String(), second.Token) || strings.Contains(logs.String(), "synthetic passphrase") {
		t.Fatal("logs contain credentials")
	}
}

func TestGenericLoginAndBudgets(t *testing.T) {
	m := testManager(t)
	handler := m.mainHandler(testHost)
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
		t.Fatal("KDF concurrency not bounded")
	}
}

func unconfiguredManager(t *testing.T) *manager {
	t.Helper()
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.db.Close() })
	return &manager{store: s, now: time.Now}
}
func testActivation(m *manager) (bootstrapCapability, *setupActivation) {
	capability := newCapability()
	return capability, &setupActivation{host: testHost, capabilityHash: capability.digest(), expiresAt: m.now().Add(setupLifetime), completed: make(chan struct{}, 1)}
}
func setupHeaders(c bootstrapCapability) map[string]string {
	return map[string]string{"Authorization": "Bearer " + c.encode()}
}

func TestSetupPolicyExpiryAndReplay(t *testing.T) {
	m := unconfiguredManager(t)
	capability, activation := testActivation(m)
	handler := m.setupHandler(t.Context(), activation)
	for _, headers := range []map[string]string{{"Origin": ""}, {"Origin": "null"}, {"Origin": testEndpoint}, {"Host": "evil.example"}, {"Host": "localhost:8080"}, {"Authorization": "Bearer bad"}, {"Authorization": ""}} {
		base := setupHeaders(capability)
		for k, v := range headers {
			base[k] = v
		}
		base["Forwarded"] = "for=127.0.0.1;host=" + testHost
		requireStatus(t, apiRequest(handler, "POST", "/owner", testCredentials, nil, base), 403)
	}
	otherToken, _ := testSession(t, testManager(t))
	requireStatus(t, apiRequest(handler, "POST", "/owner", testCredentials, &otherToken, nil), 403)
	for _, path := range []string{"/", "/setup.js", "/api/v1/health", "/api/v1/sign-in", "/api/v1/bots", "/owner/", "/%6fwner", "//owner", "/x/../owner"} {
		requireStatus(t, apiRequest(handler, "POST", path, testCredentials, nil, setupHeaders(capability)), 404)
	}
	requireStatus(t, apiRequest(handler, "GET", "/owner", "", nil, setupHeaders(capability)), 405)
	requireStatus(t, apiRequest(handler, "POST", "/owner", `{"username":"owner","password":"short"}`, nil, setupHeaders(capability)), 400)
	response := apiRequest(handler, "POST", "/owner", testCredentials, nil, setupHeaders(capability))
	requireStatus(t, response, 201)
	if strings.TrimSpace(response.Body.String()) != `{"owner":{"username":"owner"}}` || !response.Flushed {
		t.Fatal("setup response or flush differs")
	}
	var count int
	if err := m.store.db.QueryRow("SELECT count(*) FROM sessions").Scan(&count); err != nil || count != 0 {
		t.Fatal("setup created a session")
	}
	requireStatus(t, apiRequest(handler, "POST", "/owner", testCredentials, nil, setupHeaders(capability)), 409)
	activation.expiresAt = m.now()
	requireStatus(t, apiRequest(handler, "POST", "/owner", testCredentials, nil, setupHeaders(capability)), 410)
	main := m.mainHandler(testHost)
	requireStatus(t, apiRequest(main, "GET", "/api/v1/bots", "", nil, map[string]string{"Authorization": "Bearer " + capability.encode()}), 401)
}

type terminalLines chan string

func (lines terminalLines) Write(p []byte) (int, error) { lines <- string(p); return len(p), nil }

func TestSetupActualListenerCloses(t *testing.T) {
	for _, complete := range []bool{true, false} {
		t.Run(fmt.Sprint(complete), func(t *testing.T) {
			m := unconfiguredManager(t)
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			lines := make(terminalLines, 4)
			done := make(chan error, 1)
			go func() { done <- m.serveSetup(ctx, lines) }()
			var record bootstrapRecord
			select {
			case line := <-lines:
				if err := json.Unmarshal([]byte(line), &record); err != nil {
					t.Fatal("bootstrap record is not JSON")
				}
			case <-time.After(5 * time.Second):
				t.Fatal("setup did not start")
			}
			if !strings.HasPrefix(record.BootstrapEndpoint, "http://127.0.0.1:") || !strings.HasSuffix(record.BootstrapEndpoint, "/owner") || record.ExpiresAt.Sub(time.Now()) > setupLifetime {
				t.Fatal("bootstrap record differs")
			}
			if complete {
				request, _ := http.NewRequest("POST", record.BootstrapEndpoint, strings.NewReader(testCredentials))
				request.Header.Set("Content-Type", "application/json")
				request.Header.Set("Authorization", "Bearer "+record.Capability)
				response, err := (&http.Client{Timeout: 5 * time.Second}).Do(request)
				if err != nil {
					t.Fatal("setup request failed")
				}
				body, _ := io.ReadAll(response.Body)
				response.Body.Close()
				if response.StatusCode != 201 || strings.TrimSpace(string(body)) != `{"owner":{"username":"owner"}}` {
					t.Fatal("setup success not delivered")
				}
			} else {
				cancel()
			}
			select {
			case err := <-done:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("setup listener stayed open")
			}
			address := strings.TrimSuffix(strings.TrimPrefix(record.BootstrapEndpoint, "http://"), "/owner")
			connection, err := net.DialTimeout("tcp4", address, time.Second)
			if err == nil {
				connection.Close()
				t.Fatal("setup accepted after closure")
			}
			configured, err := m.store.configured(t.Context())
			if err != nil || configured != complete {
				t.Fatal("cancelled setup wrote owner")
			}
			select {
			case <-lines:
				t.Fatal("printed more than one setup record")
			default:
			}
		})
	}
}

func TestUninitializedRunNeverListens(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	cfg, err := ParseConfig([]string{"--data-dir", t.TempDir(), "--port", fmt.Sprint(listener.Addr().(*net.TCPAddr).Port)})
	if err != nil {
		t.Fatal(err)
	}
	err = Run(t.Context(), cfg, io.Discard)
	if err == nil || !strings.Contains(err.Error(), "--setup") || strings.Contains(err.Error(), "address already in use") {
		t.Fatalf("startup did not fail before listen: %v", err)
	}
	for _, args := range [][]string{{"--host", "0.0.0.0"}, {"--port", "65536"}, {"--data-dir", ""}, {"--dev-assets", "anything"}, {"positional"}} {
		if _, err = ParseConfig(args); err == nil {
			t.Fatalf("configuration accepted: %v", args)
		}
	}
}

func TestMainIsolationBeforeSetupAndRemoteAddress(t *testing.T) {
	m := unconfiguredManager(t)
	handler := m.mainHandler(testHost)
	for _, path := range []string{"/owner", "/setup", "/setup.js", "/api/v1/setup", "/%6fwner", "/x/../owner"} {
		request := httptest.NewRequest("POST", testEndpoint+path, strings.NewReader("{}"))
		request.RemoteAddr = "198.51.100.2:1234"
		request.Header.Set("X-Forwarded-For", "127.0.0.1")
		request.Header.Set("X-Forwarded-Host", testHost)
		request.Header.Set("Forwarded", "for=127.0.0.1")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		requireStatus(t, response, 404)
	}
	configured, err := m.store.configured(t.Context())
	if err != nil || configured {
		t.Fatal("main initialized owner")
	}
}

func TestSetupRechecksExpiryAndCancellation(t *testing.T) {
	for _, stage := range []string{"before hashing", "after hashing", "transaction admission", "cancelled"} {
		t.Run(stage, func(t *testing.T) {
			m := unconfiguredManager(t)
			now := time.Now()
			capability, activation := testActivation(m)
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			var calls atomic.Int32
			m.now = func() time.Time {
				count := calls.Add(1)
				threshold := int32(3)
				if stage == "after hashing" {
					threshold = 4
				}
				if stage == "transaction admission" {
					threshold = 6
				}
				if stage != "cancelled" && count >= threshold {
					return now.Add(time.Hour)
				}
				return now
			}
			if stage == "cancelled" {
				cancel()
			}
			response := apiRequest(m.setupHandler(ctx, activation), "POST", "/owner", testCredentials, nil, setupHeaders(capability))
			requireStatus(t, response, 410)
			configured, err := m.store.configured(t.Context())
			if err != nil || configured {
				t.Fatal("inactive setup created owner")
			}
		})
	}
}

func TestSecretsAreNotStoredPlaintext(t *testing.T) {
	dir := t.TempDir()
	s, err := openStore(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.createOwner(t.Context(), "owner", testHash(t), time.Now()); err != nil {
		t.Fatal(err)
	}
	m := &manager{store: s, now: time.Now, dummy: testHash(t)}
	token, _, _, err := m.signIn(t.Context(), credentials{"owner", "synthetic passphrase"})
	if err != nil {
		t.Fatal(err)
	}
	if err = s.db.Close(); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		body, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(body, []byte(token.encode())) || bytes.Contains(body, token.raw[:]) || bytes.Contains(body, []byte("synthetic passphrase")) {
			t.Fatal("storage contains plaintext credentials")
		}
	}
}

type bodyBarrier struct {
	io.Reader
	entered chan struct{}
	release <-chan struct{}
	once    sync.Once
}

func (b *bodyBarrier) Read(p []byte) (int, error) {
	b.once.Do(func() { close(b.entered); <-b.release })
	return b.Reader.Read(p)
}

func TestBotMutationRechecksSessionAfterBodyRead(t *testing.T) {
	for _, invalidation := range []string{"revoked", "expired"} {
		t.Run(invalidation, func(t *testing.T) {
			m := testManager(t)
			var clock atomic.Int64
			clock.Store(time.Now().Unix())
			m.now = func() time.Time { return time.Unix(clock.Load(), 0) }
			token, current := testSession(t, m)
			admin, _ := testSession(t, m)
			entered, release := make(chan struct{}), make(chan struct{})
			var releaseOnce sync.Once
			t.Cleanup(func() { releaseOnce.Do(func() { close(release) }) })
			body := &bodyBarrier{Reader: strings.NewReader(`{"name":"too late"}`), entered: entered, release: release}
			request := httptest.NewRequest("POST", testEndpoint+"/api/v1/bots", body)
			request.Header.Set("Authorization", "Bearer "+token.encode())
			request.Header.Set("Content-Type", "application/json")
			response := httptest.NewRecorder()
			done := make(chan struct{})
			go func() { defer close(done); m.mainHandler(testHost).ServeHTTP(response, request) }()
			select {
			case <-entered:
			case <-time.After(5 * time.Second):
				t.Fatal("authenticated request did not reach body")
			}
			if invalidation == "revoked" {
				if err := m.revokeSession(t.Context(), admin, current.id); err != nil {
					t.Fatal(err)
				}
			} else {
				clock.Store(current.expiresAt.Unix())
			}
			releaseOnce.Do(func() { close(release) })
			select {
			case <-done:
			case <-time.After(5 * time.Second):
				t.Fatal("request did not finish")
			}
			requireStatus(t, response, 401)
			var count int
			if err := m.store.db.QueryRow("SELECT count(*) FROM bots").Scan(&count); err != nil || count != 0 {
				t.Fatal("invalidated session committed a bot")
			}
		})
	}
}

func TestSetupQueuedForTransactionCannotOutliveActivation(t *testing.T) {
	for _, invalidation := range []string{"expired", "cancelled"} {
		t.Run(invalidation, func(t *testing.T) {
			m := unconfiguredManager(t)
			var clock atomic.Int64
			clock.Store(time.Now().Unix())
			m.now = func() time.Time { return time.Unix(clock.Load(), 0) }
			capability, activation := testActivation(m)
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			tx, err := m.store.db.BeginTx(t.Context(), nil)
			if err != nil {
				t.Fatal(err)
			}
			defer tx.Rollback()
			waits := m.store.db.Stats().WaitCount
			result := make(chan *httptest.ResponseRecorder, 1)
			go func() {
				result <- apiRequest(m.setupHandler(ctx, activation), "POST", "/owner", testCredentials, nil, setupHeaders(capability))
			}()
			deadline := time.Now().Add(5 * time.Second)
			for m.store.db.Stats().WaitCount == waits && time.Now().Before(deadline) {
				time.Sleep(time.Millisecond)
			}
			if m.store.db.Stats().WaitCount == waits {
				t.Fatal("setup did not wait for transaction admission")
			}
			if invalidation == "expired" {
				clock.Store(activation.expiresAt.Unix())
			} else {
				cancel()
			}
			if err = tx.Commit(); err != nil {
				t.Fatal(err)
			}
			select {
			case response := <-result:
				requireStatus(t, response, 410)
			case <-time.After(5 * time.Second):
				t.Fatal("queued setup did not finish")
			}
			configured, err := m.store.configured(t.Context())
			if err != nil || configured {
				t.Fatal("inactive setup committed after waiting")
			}
		})
	}
}
