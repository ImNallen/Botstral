package manager

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"regexp"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"golang.org/x/crypto/argon2"
)

const sessionLifetime = 7 * 24 * time.Hour

type credentials struct{ username, password string }
type passwordHash struct{ encoded string }
type sessionToken struct{ raw [32]byte }

var usernamePattern = regexp.MustCompile(`^[a-z0-9._-]{3,64}$`)
var kdfSlots = make(chan struct{}, 2)

func parseCredentials(username, password string) (credentials, error) {
	username = strings.ToLower(strings.TrimSpace(username))
	if !usernamePattern.MatchString(username) {
		return credentials{}, inputError("Use 3 to 64 letters, numbers, dots, underscores, or hyphens for the username.")
	}
	if !utf8.ValidString(password) || utf8.RuneCountInString(password) < 12 || utf8.RuneCountInString(password) > 128 || len(password) > 1024 {
		return credentials{}, inputError("Use a password with 12 to 128 characters.")
	}
	return credentials{username: username, password: password}, nil
}

func parseBotName(raw string) (botName, error) {
	name := strings.TrimSpace(raw)
	if !utf8.ValidString(name) || name == "" || utf8.RuneCountInString(name) > 80 || len(name) > 320 {
		return botName{}, inputError("Use a bot name with 1 to 80 characters.")
	}
	for _, c := range name {
		if unicode.IsControl(c) {
			return botName{}, inputError("Bot names cannot contain control characters.")
		}
	}
	return botName{value: name}, nil
}

func hashPassword(password string) (passwordHash, error) {
	select {
	case kdfSlots <- struct{}{}:
	default:
		return passwordHash{}, errThrottled
	}
	defer func() { <-kdfSlots }()
	var salt [16]byte
	rand.Read(salt[:])
	key := argon2.IDKey([]byte(password), salt[:], 3, 64*1024, 4, 32)
	return passwordHash{encoded: fmt.Sprintf("$argon2id$v=19$m=65536,t=3,p=4$%s$%s", base64.RawStdEncoding.EncodeToString(salt[:]), base64.RawStdEncoding.EncodeToString(key))}, nil
}

func verifyPassword(hash passwordHash, password string) bool {
	parts := strings.Split(hash.encoded, "$")
	if len(parts) != 6 || parts[0] != "" || parts[1] != "argon2id" || parts[2] != "v=19" || parts[3] != "m=65536,t=3,p=4" {
		return false
	}
	salt, err := base64.RawStdEncoding.Strict().DecodeString(parts[4])
	if err != nil || len(salt) != 16 {
		return false
	}
	want, err := base64.RawStdEncoding.Strict().DecodeString(parts[5])
	if err != nil || len(want) != 32 {
		return false
	}
	key := argon2.IDKey([]byte(password), salt, 3, 64*1024, 4, 32)
	return subtle.ConstantTimeCompare(key, want) == 1
}

func parseSessionCookie(raw string) (sessionToken, error) {
	var token sessionToken
	value, err := base64.RawURLEncoding.Strict().DecodeString(raw)
	if err != nil || len(value) != len(token.raw) {
		return token, errUnauthorized
	}
	copy(token.raw[:], value)
	return token, nil
}

func newToken() sessionToken {
	var token sessionToken
	rand.Read(token.raw[:])
	return token
}
func randomID() string {
	var id [16]byte
	rand.Read(id[:])
	return hex.EncodeToString(id[:])
}
func (t sessionToken) cookie() string   { return base64.RawURLEncoding.EncodeToString(t.raw[:]) }
func (t sessionToken) digest() [32]byte { return sha256.Sum256(t.raw[:]) }
func (t sessionToken) csrf() string {
	mac := hmac.New(sha256.New, t.raw[:])
	mac.Write([]byte("botstral csrf v1"))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

type attemptBudget struct {
	mu     sync.Mutex
	tokens float64
	last   time.Time
}

func (b *attemptBudget) allow(now time.Time) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.last.IsZero() {
		b.tokens = 5
	} else {
		b.tokens = min(5, b.tokens+max(0, now.Sub(b.last).Seconds())/6)
	}
	b.last = now
	if b.tokens < 1 {
		return false
	}
	b.tokens--
	return true
}

type manager struct {
	store       *store
	now         func() time.Time
	origin      string
	loginBudget attemptBudget
	dummy       passwordHash
}

func newManager(s *store) (*manager, error) {
	dummy, err := hashPassword(randomID())
	if err != nil {
		return nil, err
	}
	return &manager{store: s, now: time.Now, dummy: dummy}, nil
}

func (m *manager) signIn(ctx context.Context, input credentials, previous *sessionToken) (sessionToken, session, owner, error) {
	var token sessionToken
	var value session
	var profile owner
	if !m.loginBudget.allow(m.now()) {
		return token, value, profile, errThrottled
	}
	var username, encoded string
	var created int64
	err := m.store.db.QueryRowContext(ctx, "SELECT username, password_hash, created_at FROM owner WHERE id = 1").Scan(&username, &encoded, &created)
	if err != nil && err != sql.ErrNoRows {
		return token, value, profile, errUnavailable
	}
	hash := m.dummy
	known := err == nil && username == input.username
	if known {
		hash = passwordHash{encoded: encoded}
	}
	select {
	case kdfSlots <- struct{}{}:
	default:
		return token, value, profile, errThrottled
	}
	valid := verifyPassword(hash, input.password)
	<-kdfSlots
	if !valid || !known {
		return token, value, profile, errCredentials
	}
	token = newToken()
	digest := token.digest()
	tx, err := m.store.db.BeginTx(ctx, nil)
	if err != nil {
		return token, value, profile, errUnavailable
	}
	defer tx.Rollback()
	now := m.now().UTC().Truncate(time.Second)
	value = session{id: sessionID(randomID()), createdAt: now, expiresAt: now.Add(sessionLifetime)}
	if _, err = tx.ExecContext(ctx, "DELETE FROM sessions WHERE expires_at <= ?", now.Unix()); err != nil {
		return token, value, profile, errUnavailable
	}
	if previous != nil {
		digest := previous.digest()
		if _, err = tx.ExecContext(ctx, "DELETE FROM sessions WHERE token_hash = ?", digest[:]); err != nil {
			return token, value, profile, errUnavailable
		}
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO sessions(id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?)", value.id, digest[:], value.createdAt.Unix(), value.expiresAt.Unix()); err != nil {
		return token, value, profile, errUnavailable
	}
	if err = tx.Commit(); err != nil {
		return token, value, profile, errUnavailable
	}
	profile = owner{username: username, createdAt: time.Unix(created, 0).UTC()}
	return token, value, profile, nil
}
