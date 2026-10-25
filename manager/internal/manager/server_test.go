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
	"time"
)

func TestServerOptionsStarUsesLocalBoundary(t *testing.T) {
	m := unconfiguredManager(t)
	for _, setup := range []bool{false, true} {
		t.Run(fmt.Sprint(setup), func(t *testing.T) {
			server := httptest.NewUnstartedServer(nil)
			host := server.Listener.Addr().String()
			handler := m.mainHandler(host)
			if setup {
				_, activation := testActivation(m)
				activation.host = host
				handler = m.setupHandler(t.Context(), activation)
			}
			server.Config = httpServer(handler)
			server.Start()
			defer server.Close()
			for _, policy := range []string{"valid", "host", "origin", "both"} {
				t.Run(policy, func(t *testing.T) {
					request, err := http.NewRequest("OPTIONS", server.URL, nil)
					if err != nil {
						t.Fatal(err)
					}
					request.URL.Path = "*"
					status := 404
					if policy == "host" || policy == "both" {
						request.Host = "evil.example"
						status = 403
					}
					if policy == "origin" || policy == "both" {
						request.Header.Set("Origin", "null")
						status = 403
					}
					response, err := server.Client().Do(request)
					if err != nil {
						t.Fatal(err)
					}
					defer response.Body.Close()
					body, err := io.ReadAll(response.Body)
					if err != nil {
						t.Fatal(err)
					}
					if response.StatusCode != status {
						t.Fatalf("OPTIONS * returned %d, want %d", response.StatusCode, status)
					}
					if response.Header.Get("Content-Type") != "application/json" || response.Header.Get("Cache-Control") != "no-store" || !json.Valid(body) {
						t.Fatal("OPTIONS * bypassed the JSON response policy")
					}
				})
			}
		})
	}
}

func TestSetupDrainDoesNotStopManager(t *testing.T) {
	cfg, err := ParseConfig([]string{"--data-dir", t.TempDir(), "--port", "0", "--setup"})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	lines := make(terminalLines, 4)
	done := make(chan error, 1)
	go func() { done <- Run(ctx, cfg, lines) }()
	exited := false
	t.Cleanup(func() {
		cancel()
		if exited {
			return
		}
		select {
		case err := <-done:
			if err != nil {
				t.Error(err)
			}
		case <-time.After(15 * time.Second):
			t.Error("manager did not stop after cancellation")
		}
	})
	var endpoint string
	var record bootstrapRecord
	startup := time.NewTimer(5 * time.Second)
	defer startup.Stop()
	for endpoint == "" || record.BootstrapEndpoint == "" {
		select {
		case line := <-lines:
			if strings.HasPrefix(line, "Manager: ") {
				endpoint = strings.TrimSpace(strings.TrimPrefix(line, "Manager: "))
			} else if json.Unmarshal([]byte(line), &record) != nil {
				t.Fatal("bootstrap record is not JSON")
			}
		case err := <-done:
			exited = true
			t.Fatalf("manager exited before bootstrap: %v", err)
		case <-startup.C:
			t.Fatal("manager did not activate bootstrap")
		}
	}
	address := strings.TrimSuffix(strings.TrimPrefix(record.BootstrapEndpoint, "http://"), "/owner")
	stalled, err := net.DialTimeout("tcp4", address, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer stalled.Close()
	if _, err = fmt.Fprintf(stalled, "POST /owner HTTP/1.1\r\nHost: %s\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{", address); err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Timeout: 5 * time.Second, Transport: &http.Transport{Proxy: nil, DisableKeepAlives: true}}
	defer client.CloseIdleConnections()
	setup, err := http.NewRequest("POST", record.BootstrapEndpoint, strings.NewReader(testCredentials))
	if err != nil {
		t.Fatal(err)
	}
	setup.Header.Set("Content-Type", "application/json")
	setup.Header.Set("Authorization", "Bearer "+record.Capability)
	response, err := client.Do(setup)
	if err != nil {
		t.Fatal("valid setup request failed")
	}
	io.Copy(io.Discard, response.Body)
	response.Body.Close()
	if response.StatusCode != 201 {
		t.Fatalf("setup returned %d, want 201", response.StatusCode)
	}
	select {
	case err := <-done:
		exited = true
		t.Fatalf("manager exited after successful setup: %v", err)
	case <-time.After(12 * time.Second):
	}
	response, err = client.Get(endpoint + "/api/v1/health")
	if err != nil {
		t.Fatal("main listener stopped after setup drain")
	}
	io.Copy(io.Discard, response.Body)
	response.Body.Close()
	if response.StatusCode != 200 {
		t.Fatalf("main health returned %d, want 200", response.StatusCode)
	}
	response, err = client.Post(endpoint+"/api/v1/sign-in", "application/json", strings.NewReader(testCredentials))
	if err != nil {
		t.Fatal("sign-in failed after setup drain")
	}
	io.Copy(io.Discard, response.Body)
	response.Body.Close()
	if response.StatusCode != 200 {
		t.Fatalf("sign-in returned %d, want 200", response.StatusCode)
	}
}
