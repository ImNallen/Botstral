package manager

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"testing/fstest"
	"time"
)

var hashOnce sync.Once
var ownerHash passwordHash

func testHash(t *testing.T) passwordHash {
	t.Helper()
	hashOnce.Do(func() {
		var err error
		ownerHash, err = hashPassword("synthetic passphrase")
		if err != nil {
			t.Fatal(err)
		}
	})
	return ownerHash
}

func testManager(t *testing.T) *manager {
	t.Helper()
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.db.Close() })
	m := &manager{store: s, now: time.Now, dummy: testHash(t)}
	if err = s.createOwner(t.Context(), "owner", testHash(t), m.now()); err != nil {
		t.Fatal(err)
	}
	return m
}

func testSession(t *testing.T, m *manager) (sessionToken, session) {
	t.Helper()
	token := newToken()
	digest := token.digest()
	now := m.now().UTC().Truncate(time.Second)
	value := session{id: sessionID(randomID()), createdAt: now, expiresAt: now.Add(sessionLifetime)}
	_, err := m.store.db.ExecContext(t.Context(), "INSERT INTO sessions(id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?)", value.id, digest[:], now.Unix(), value.expiresAt.Unix())
	if err != nil {
		t.Fatal(err)
	}
	return token, value
}

func TestMigrationsDurabilityAndProtection(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "data ?# percent%")
	s, err := openStore(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.migrate(ctx); err != nil {
		t.Fatal(err)
	}
	if err = s.createOwner(ctx, "owner", testHash(t), time.Now()); err != nil {
		t.Fatal(err)
	}
	m := &manager{store: s, now: time.Now}
	token, value := testSession(t, m)
	name, _ := parseBotName("Research")
	created, err := m.createBot(ctx, token, name)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.db.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = openStore(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	m.store = s
	bots, err := m.listBots(ctx, token)
	if err != nil || len(bots) != 1 || bots[0].id != created.id || bots[0].name.value != "Research" {
		t.Fatalf("durable bots: %+v, %v", bots, err)
	}
	current, profile, err := m.currentSession(ctx, token)
	if err != nil || current.id != value.id || profile.username != "owner" {
		t.Fatalf("durable session: %+v, %+v, %v", current, profile, err)
	}
	for path, want := range map[string]os.FileMode{dir: 0700, filepath.Join(dir, "botstral.sqlite"): 0600} {
		info, err := os.Stat(path)
		if err != nil || info.Mode().Perm() != want {
			t.Fatalf("permissions %s: %v, %v", path, info, err)
		}
	}
	for pragma, want := range map[string]int{"foreign_keys": 1, "synchronous": 2, "busy_timeout": 5000} {
		var got int
		if err := s.db.QueryRow("PRAGMA " + pragma).Scan(&got); err != nil || got != want {
			t.Fatalf("%s = %d: %v", pragma, got, err)
		}
	}
	var hash []byte
	if err = s.db.QueryRow("SELECT token_hash FROM sessions WHERE id = ?", value.id).Scan(&hash); err != nil {
		t.Fatal(err)
	}
	if len(hash) != 32 || bytes.Equal(hash, token.raw[:]) {
		t.Fatal("stored bearer instead of digest")
	}
}

func TestSingletonOwnerRacesAndConstraints(t *testing.T) {
	ctx := t.Context()
	dir := t.TempDir()
	first, err := openStore(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer first.db.Close()
	second, err := openStore(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer second.db.Close()
	if _, err = first.db.Exec("INSERT INTO bots(id, name, created_at) VALUES (?, 'orphan', 1)", randomID()); err == nil {
		t.Fatal("orphan bot accepted")
	}
	start := make(chan struct{})
	results := make(chan error, 2)
	hash := testHash(t)
	for _, s := range []*store{first, second} {
		go func() { <-start; results <- s.createOwner(ctx, "owner", hash, time.Now()) }()
	}
	close(start)
	a, b := <-results, <-results
	if !(a == nil && errors.Is(b, errOwnerExists) || b == nil && errors.Is(a, errOwnerExists)) {
		t.Fatalf("setup race: %v, %v", a, b)
	}
	for _, id := range []int{1, 2} {
		if _, err = first.db.Exec("INSERT INTO owner VALUES (?, 'other', 'hash', 1)", id); err == nil {
			t.Fatalf("second owner %d accepted", id)
		}
	}
	var count int
	if err = first.db.QueryRow("SELECT count(*) FROM owner WHERE username = 'owner' AND password_hash != ''").Scan(&count); err != nil || count != 1 {
		t.Fatalf("complete owners %d: %v", count, err)
	}
}

func TestMigrationIntegrityAndRollback(t *testing.T) {
	m := testManager(t)
	ctx := t.Context()
	body, err := migrations.ReadFile("migrations/001_initial.sql")
	if err != nil {
		t.Fatal(err)
	}
	files := fstest.MapFS{
		"migrations/001_initial.sql": &fstest.MapFile{Data: body},
		"migrations/002_broken.sql":  &fstest.MapFile{Data: []byte("CREATE TABLE partial(id INTEGER); INSERT INTO absent VALUES (1);")},
	}
	if err = m.store.applyMigrations(ctx, files); err == nil {
		t.Fatal("broken migration succeeded")
	}
	var count int
	if err = m.store.db.QueryRow("SELECT count(*) FROM sqlite_master WHERE name = 'partial'").Scan(&count); err != nil || count != 0 {
		t.Fatalf("partial migration survived: %d, %v", count, err)
	}
	if err = m.store.db.QueryRow("SELECT count(*) FROM schema_migrations").Scan(&count); err != nil || count != 1 {
		t.Fatalf("partial version survived: %d, %v", count, err)
	}
	files["migrations/001_initial.sql"].Data = append(body, '\n')
	if err = m.store.applyMigrations(ctx, files); err == nil || !strings.Contains(err.Error(), "checksum") {
		t.Fatalf("changed checksum: %v", err)
	}
	if _, err = m.store.db.Exec("INSERT INTO schema_migrations VALUES (999, 'future', 1)"); err != nil {
		t.Fatal(err)
	}
	if err = m.store.migrate(ctx); err == nil || !strings.Contains(err.Error(), "newer") {
		t.Fatalf("newer database: %v", err)
	}
}

func TestSetupFailureLeavesNoOwner(t *testing.T) {
	s, err := openStore(t.Context(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer s.db.Close()
	if _, err = s.db.Exec("CREATE TRIGGER fail_owner BEFORE INSERT ON owner BEGIN SELECT RAISE(ABORT, 'injected'); END"); err != nil {
		t.Fatal(err)
	}
	if err = s.createOwner(t.Context(), "owner", testHash(t), time.Now()); !errors.Is(err, errUnavailable) {
		t.Fatalf("setup failure: %v", err)
	}
	configured, err := s.configured(t.Context())
	if err != nil || configured {
		t.Fatalf("partial owner exists: %v, %v", configured, err)
	}
	if _, err = s.db.Exec("DROP TRIGGER fail_owner"); err != nil {
		t.Fatal(err)
	}
	if err = s.createOwner(t.Context(), "owner", testHash(t), time.Now()); err != nil {
		t.Fatal(err)
	}
}

func TestRevocationSerializesWithOperations(t *testing.T) {
	m := testManager(t)
	token, current := testSession(t, m)
	admin, _ := testSession(t, m)
	entered, release := make(chan struct{}), make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- m.store.withSession(context.Background(), token, m.now, func(tx *sql.Tx, _ session) error {
			close(entered)
			<-release
			_, err := tx.Exec("INSERT INTO bots(id, name, created_at) VALUES (?, 'ordered', 1)", randomID())
			return err
		})
	}()
	<-entered
	revoked := make(chan error, 1)
	go func() { revoked <- m.revokeSession(context.Background(), admin, current.id) }()
	close(release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if err := <-revoked; err != nil {
		t.Fatal(err)
	}
	name, _ := parseBotName("rejected")
	if _, err := m.createBot(t.Context(), token, name); !errors.Is(err, errUnauthorized) {
		t.Fatalf("revoked mutation: %v", err)
	}
	bots, err := m.listBots(t.Context(), admin)
	if err != nil || len(bots) != 1 || bots[0].name.value != "ordered" {
		t.Fatalf("ordered result: %+v, %v", bots, err)
	}
}

func TestExpiryFixedAndSignInRotation(t *testing.T) {
	m := testManager(t)
	ctx := t.Context()
	now := time.Now().UTC().Truncate(time.Second)
	m.now = func() time.Time { return now }
	input, err := parseCredentials(" OWNER ", "synthetic passphrase")
	if err != nil {
		t.Fatal(err)
	}
	token, value, _, err := m.signIn(ctx, input, nil)
	if err != nil {
		t.Fatal(err)
	}
	if value.expiresAt.Sub(value.createdAt) != sessionLifetime {
		t.Fatal("session lifetime differs")
	}
	other, _, _, err := m.signIn(ctx, input, nil)
	if err != nil {
		t.Fatal(err)
	}
	replacement, _, _, err := m.signIn(ctx, input, &token)
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = m.currentSession(ctx, token); !errors.Is(err, errUnauthorized) {
		t.Fatalf("old session valid: %v", err)
	}
	if _, _, err = m.currentSession(ctx, other); err != nil {
		t.Fatalf("independent session lost: %v", err)
	}
	now = value.expiresAt
	if _, _, err = m.currentSession(ctx, replacement); !errors.Is(err, errUnauthorized) {
		t.Fatalf("expired session valid: %v", err)
	}
}

func TestCredentialAndNameBoundaries(t *testing.T) {
	for _, input := range []string{"", "ab", "Üser", "owner space"} {
		if _, err := parseCredentials(input, "synthetic passphrase"); err == nil {
			t.Fatalf("username %q accepted", input)
		}
	}
	for _, input := range []string{"short", strings.Repeat("a", 129)} {
		if _, err := parseCredentials("owner", input); err == nil {
			t.Fatal("invalid password accepted")
		}
	}
	for _, input := range []string{"", " \t ", "a\nb", strings.Repeat("é", 81)} {
		if _, err := parseBotName(input); err == nil {
			t.Fatalf("name %q accepted", input)
		}
	}
	hash := testHash(t)
	if !verifyPassword(hash, "synthetic passphrase") || verifyPassword(hash, "wrong password") {
		t.Fatal("password verification failed")
	}
	for _, malformed := range []string{"", strings.Replace(hash.encoded, "m=65536", "m=999999999", 1), strings.Replace(hash.encoded, "v=19", "v=20", 1)} {
		if verifyPassword(passwordHash{encoded: malformed}, "synthetic passphrase") {
			t.Fatal("unsupported hash accepted")
		}
	}
	token := newToken()
	parsed, err := parseSessionCookie(token.cookie())
	if err != nil || parsed != token {
		t.Fatal("cookie round trip failed")
	}
	for _, raw := range []string{"", "bad", token.cookie() + "="} {
		if _, err := parseSessionCookie(raw); err == nil {
			t.Fatal("malformed cookie accepted")
		}
	}
}
