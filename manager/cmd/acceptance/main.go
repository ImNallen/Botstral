package main

import (
	"bufio"
	"bytes"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

type bootstrap struct {
	Endpoint   string    `json:"bootstrapEndpoint"`
	Capability string    `json:"capability"`
	ExpiresAt  time.Time `json:"expiresAt"`
}
type metadata struct {
	ID        string    `json:"id"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}
type signIn struct {
	Token string `json:"token"`
	Owner struct {
		Username string `json:"username"`
	} `json:"owner"`
	Session metadata `json:"session"`
}
type bot struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	CreatedAt time.Time `json:"createdAt"`
}
type process struct {
	command   *exec.Cmd
	done      chan error
	endpoint  string
	bootstrap *bootstrap
	lines     <-chan string
	logs      []string
	stopped   bool
}
type driver struct {
	client                                  *http.Client
	binary, directory, dataDir, credentials string
	secrets                                 []string
}

func main() {
	binary := flag.String("binary", "../bin/botstral", "built manager executable")
	flag.Parse()
	if err := verify(*binary); err != nil {
		fmt.Fprintln(os.Stderr, "Acceptance failed. "+err.Error())
		os.Exit(1)
	}
	fmt.Println("Acceptance passed. Bot identity, original session, revocation, sign-out, and setup isolation survived the required lifecycle.")
}

func verify(binary string) error {
	absolute, err := filepath.Abs(binary)
	if err != nil {
		return errors.New("Cannot resolve executable path.")
	}
	directory, err := os.MkdirTemp("", "botstral-acceptance-")
	if err != nil {
		return errors.New("Cannot create acceptance directory.")
	}
	defer os.RemoveAll(directory)
	copied := filepath.Join(directory, "botstral")
	source, err := os.Open(absolute)
	if err != nil {
		return errors.New("Cannot open built executable.")
	}
	destination, err := os.OpenFile(copied, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0700)
	if err != nil {
		source.Close()
		return errors.New("Cannot copy executable.")
	}
	_, copyErr := io.Copy(destination, source)
	source.Close()
	closeErr := destination.Close()
	if copyErr != nil || closeErr != nil {
		return errors.New("Cannot copy executable.")
	}
	password := make([]byte, 32)
	rand.Read(password)
	passwordText := base64.RawURLEncoding.EncodeToString(password)
	credentials, _ := json.Marshal(struct {
		Username string `json:"username"`
		Password string `json:"password"`
	}{"owner", passwordText})
	transport := &http.Transport{Proxy: nil, DisableKeepAlives: true}
	defer transport.CloseIdleConnections()
	d := &driver{binary: copied, directory: directory, dataDir: filepath.Join(directory, "persistent"), credentials: string(credentials), secrets: []string{passwordText}, client: &http.Client{Transport: transport, Timeout: 5 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
	first, err := d.start()
	if err != nil {
		return err
	}
	defer first.stop()
	if first.bootstrap == nil {
		return errors.New("Unconfigured manager did not activate bootstrap.")
	}
	activation := first.bootstrap
	d.secrets = append(d.secrets, activation.Capability)
	if !activation.ExpiresAt.After(time.Now()) || activation.ExpiresAt.After(time.Now().Add(15*time.Minute)) {
		return errors.New("Bootstrap expiry differs.")
	}
	for _, route := range []string{"/", "/setup.js", "/api/v1/sign-in", "/api/v1/bots", "/%6fwner", "/x/../owner"} {
		root := strings.TrimSuffix(activation.Endpoint, "/owner")
		if _, err = d.request("POST", root+route, d.credentials, activation.Capability, 404); err != nil {
			return fmt.Errorf("Setup route isolation failed. %w", err)
		}
	}
	if _, err = d.request("POST", activation.Endpoint, d.credentials, "invalid", 403); err != nil {
		return err
	}
	body, err := d.request("POST", activation.Endpoint, d.credentials, activation.Capability, 201)
	if err != nil {
		return err
	}
	if strings.TrimSpace(string(body)) != `{"owner":{"username":"owner"}}` {
		return errors.New("Bootstrap response differs or created a session.")
	}
	if err = d.waitClosed(activation.Endpoint); err != nil {
		return err
	}
	if err = d.isolation(first.endpoint); err != nil {
		return err
	}
	a, err := d.signIn(first.endpoint)
	if err != nil {
		return err
	}
	body, err = d.request("POST", first.endpoint+"/api/v1/bots", `{"name":"Research"}`, a.Token, 201)
	if err != nil {
		return err
	}
	var created bot
	if json.Unmarshal(body, &created) != nil || len(created.ID) != 32 || created.Name != "Research" || created.CreatedAt.IsZero() {
		return errors.New("Bot creation response differs.")
	}
	b, err := d.signIn(first.endpoint)
	if err != nil {
		return err
	}
	if a.Token == b.Token || a.Session.ID == b.Session.ID {
		return errors.New("Sign-ins are not independent.")
	}
	if err = d.metadata(first.endpoint, a.Token); err != nil {
		return err
	}
	if _, err = d.request("DELETE", first.endpoint+"/api/v1/sessions/"+b.Session.ID, "", a.Token, 204); err != nil {
		return err
	}
	if err = d.rejected(first.endpoint, b.Token); err != nil {
		return err
	}
	if err = first.stop(); err != nil {
		return err
	}
	if err = d.checkLogs(first); err != nil {
		return err
	}
	fmt.Println("First process exited. Restarting the copied executable with the same absolute data directory.")
	second, err := d.start()
	if err != nil {
		return err
	}
	defer second.stop()
	if second.bootstrap != nil {
		return errors.New("Configured restart reopened setup.")
	}
	if err = d.waitClosed(activation.Endpoint); err != nil {
		return err
	}
	if err = d.sameBot(second.endpoint, a.Token, created.ID); err != nil {
		return err
	}
	if err = d.rejected(second.endpoint, b.Token); err != nil {
		return err
	}
	c, err := d.signIn(second.endpoint)
	if err != nil {
		return err
	}
	if err = d.sameBot(second.endpoint, c.Token, created.ID); err != nil {
		return err
	}
	if err = d.isolation(second.endpoint); err != nil {
		return err
	}
	if _, err = d.request("POST", second.endpoint+"/api/v1/sign-out", "", c.Token, 204); err != nil {
		return err
	}
	if err = d.rejected(second.endpoint, c.Token); err != nil {
		return err
	}
	if err = second.stop(); err != nil {
		return err
	}
	if err = d.checkLogs(second); err != nil {
		return err
	}
	return nil
}

func (d *driver) start() (*process, error) {
	command := exec.Command(d.binary, "--data-dir", d.dataDir, "--port", "0", "--setup")
	command.Dir = d.directory
	output, writer := io.Pipe()
	command.Stderr = writer
	command.Stdout = writer
	if err := command.Start(); err != nil {
		writer.Close()
		output.Close()
		return nil, errors.New("Cannot start copied executable.")
	}
	lines := make(chan string, 32)
	go func() {
		defer close(lines)
		defer output.Close()
		scanner := bufio.NewScanner(output)
		for scanner.Scan() {
			lines <- scanner.Text()
		}
	}()
	p := &process{command: command, done: make(chan error, 1), lines: lines}
	go func() {
		err := command.Wait()
		writer.Close()
		p.done <- err
	}()
	deadline := time.NewTimer(10 * time.Second)
	defer deadline.Stop()
	ready := false
	for p.endpoint == "" || !ready {
		select {
		case line, ok := <-lines:
			if !ok {
				p.stop()
				return nil, errors.New("Manager exited before startup completed.")
			}
			if strings.HasPrefix(line, "Manager: ") {
				p.endpoint = strings.TrimPrefix(line, "Manager: ")
				if !strings.HasPrefix(p.endpoint, "http://127.0.0.1:") {
					p.stop()
					return nil, errors.New("Main did not bind numeric loopback.")
				}
			} else if strings.HasPrefix(line, "{") {
				var record bootstrap
				if json.Unmarshal([]byte(line), &record) != nil || !strings.HasPrefix(record.Endpoint, "http://127.0.0.1:") || !strings.HasSuffix(record.Endpoint, "/owner") || record.Capability == "" || p.bootstrap != nil {
					p.stop()
					return nil, errors.New("Bootstrap record differs.")
				}
				p.bootstrap = &record
				ready = true
			} else {
				p.logs = append(p.logs, line)
				if line == "Owner already configured. Setup remains closed." {
					ready = true
				}
			}
		case <-deadline.C:
			p.stop()
			return nil, errors.New("Manager startup timed out.")
		}
	}
	if _, err := d.request("GET", p.endpoint+"/api/v1/health", "", "", 200); err != nil {
		p.stop()
		return nil, err
	}
	return p, nil
}

func (p *process) stop() error {
	if p.stopped {
		return nil
	}
	p.stopped = true
	_ = p.command.Process.Signal(os.Interrupt)
	select {
	case err := <-p.done:
		if err != nil {
			return errors.New("Manager did not exit cleanly.")
		}
	case <-time.After(12 * time.Second):
		_ = p.command.Process.Kill()
		<-p.done
		return errors.New("Manager shutdown timed out.")
	}
	for line := range p.lines {
		p.logs = append(p.logs, line)
	}
	return nil
}

func (d *driver) request(method, endpoint, body, token string, status int) ([]byte, error) {
	request, err := http.NewRequest(method, endpoint, strings.NewReader(body))
	if err != nil {
		return nil, errors.New("Cannot construct acceptance request.")
	}
	if token != "" {
		request.Header.Set("Authorization", "Bearer "+token)
	}
	if body != "" {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := d.client.Do(request)
	if err != nil {
		return nil, errors.New("Manager HTTP request failed.")
	}
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, 64*1024))
	if err != nil {
		return nil, errors.New("Cannot read manager response.")
	}
	if response.StatusCode != status {
		return nil, fmt.Errorf("Unexpected status %d, wanted %d for %s.", response.StatusCode, status, method)
	}
	if response.Header.Get("Cache-Control") != "no-store" || len(response.Cookies()) != 0 {
		return nil, errors.New("Response cache or authentication headers differ.")
	}
	if status == 204 {
		if len(data) != 0 {
			return nil, errors.New("Empty response contains a body.")
		}
	} else if response.Header.Get("Content-Type") != "application/json" || !json.Valid(data) {
		return nil, errors.New("Response is not JSON.")
	}
	return data, nil
}

func (d *driver) signIn(endpoint string) (signIn, error) {
	var result signIn
	body, err := d.request("POST", endpoint+"/api/v1/sign-in", d.credentials, "", 200)
	if err != nil {
		return result, err
	}
	if json.Unmarshal(body, &result) != nil {
		return result, errors.New("Invalid sign-in response.")
	}
	raw, err := base64.RawURLEncoding.Strict().DecodeString(result.Token)
	if err != nil || len(raw) != 32 || base64.RawURLEncoding.EncodeToString(raw) != result.Token || len(result.Session.ID) != 32 || result.Owner.Username != "owner" || result.Session.ExpiresAt.Sub(result.Session.CreatedAt) != 7*24*time.Hour {
		return result, errors.New("Sign-in fields differ.")
	}
	d.secrets = append(d.secrets, result.Token)
	return result, nil
}
func (d *driver) metadata(endpoint, token string) error {
	for _, path := range []string{"/api/v1/session", "/api/v1/sessions"} {
		body, err := d.request("GET", endpoint+path, "", token, 200)
		if err != nil {
			return err
		}
		for _, secret := range d.secrets {
			if bytes.Contains(body, []byte(secret)) {
				return errors.New("Session metadata contains a secret.")
			}
		}
		if bytes.Contains(body, []byte(`"token"`)) {
			return errors.New("Session metadata returns a token field.")
		}
	}
	return nil
}
func (d *driver) sameBot(endpoint, token, id string) error {
	body, err := d.request("GET", endpoint+"/api/v1/bots", "", token, 200)
	if err != nil {
		return err
	}
	var result struct {
		Bots []bot `json:"bots"`
	}
	if json.Unmarshal(body, &result) != nil || len(result.Bots) != 1 || result.Bots[0].ID != id || result.Bots[0].Name != "Research" {
		return errors.New("Original bot identity did not survive restart.")
	}
	return nil
}
func (d *driver) rejected(endpoint, token string) error {
	if _, err := d.request("GET", endpoint+"/api/v1/bots", "", token, 401); err != nil {
		return err
	}
	_, err := d.request("POST", endpoint+"/api/v1/bots", `{"name":"Rejected"}`, token, 401)
	return err
}
func (d *driver) isolation(endpoint string) error {
	if err := d.rejected(endpoint, ""); err != nil {
		return err
	}
	for _, path := range []string{"/", "/index.html", "/assets/app.js", "/owner", "/setup.js", "/%6fwner", "/api/v1/%62ots", "/api//v1/bots", "/api/v1/bots/"} {
		if _, err := d.request("GET", endpoint+path, "", "", 404); err != nil {
			return err
		}
	}
	return nil
}
func (d *driver) waitClosed(endpoint string) error {
	address := strings.TrimSuffix(strings.TrimPrefix(endpoint, "http://"), "/owner")
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		connection, err := net.DialTimeout("tcp4", address, 100*time.Millisecond)
		if err != nil {
			return nil
		}
		connection.Close()
		time.Sleep(10 * time.Millisecond)
	}
	return errors.New("Setup listener stayed reachable.")
}
func (d *driver) checkLogs(p *process) error {
	for _, line := range p.logs {
		if strings.HasPrefix(line, "{") {
			return errors.New("Unexpected additional bootstrap record.")
		}
		for _, secret := range d.secrets {
			if strings.Contains(line, secret) {
				return errors.New("Ordinary logs contain a credential.")
			}
		}
	}
	return nil
}
