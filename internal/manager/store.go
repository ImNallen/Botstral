package manager

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"embed"
	"errors"
	"fmt"
	"io/fs"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

//go:embed migrations/*.sql
var migrations embed.FS

var (
	errUnauthorized = errors.New("session ended; sign in again")
	errCredentials  = errors.New("username or password is incorrect")
	errOwnerExists  = errors.New("owner is already configured")
	errThrottled    = errors.New("too many attempts; try again shortly")
	errUnavailable  = errors.New("manager storage is unavailable")
)

type inputError string

func (e inputError) Error() string { return string(e) }

type owner struct {
	username  string
	createdAt time.Time
}
type botID string
type botName struct{ value string }
type bot struct {
	id        botID
	name      botName
	createdAt time.Time
}
type sessionID string
type session struct {
	id                   sessionID
	createdAt, expiresAt time.Time
}
type store struct{ db *sql.DB }

func openStore(ctx context.Context, dataDir string) (*store, error) {
	path, err := filepath.Abs(dataDir)
	if err != nil {
		return nil, err
	}
	if err = os.MkdirAll(path, 0700); err != nil {
		return nil, fmt.Errorf("create data directory: %w", err)
	}
	if err = os.Chmod(path, 0700); err != nil {
		return nil, fmt.Errorf("protect data directory: %w", err)
	}
	dbPath := filepath.Join(path, "botstral.sqlite")
	file, err := os.OpenFile(dbPath, os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		return nil, fmt.Errorf("open database file: %w", err)
	}
	err = file.Chmod(0600)
	closeErr := file.Close()
	if err != nil {
		return nil, err
	}
	if closeErr != nil {
		return nil, closeErr
	}
	u := url.URL{Scheme: "file", Path: dbPath}
	q := u.Query()
	q.Add("_pragma", "foreign_keys(1)")
	q.Add("_pragma", "busy_timeout(5000)")
	q.Add("_pragma", "synchronous(FULL)")
	q.Set("_txlock", "immediate")
	u.RawQuery = q.Encode()
	db, err := sql.Open("sqlite", u.String())
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	s := &store{db: db}
	var mode string
	if err = db.QueryRowContext(ctx, "PRAGMA journal_mode=WAL").Scan(&mode); err == nil && mode != "wal" {
		err = errors.New("database did not enable WAL")
	}
	if err == nil {
		err = s.migrate(ctx)
	}
	if err == nil {
		_, err = db.ExecContext(ctx, "DELETE FROM sessions WHERE expires_at <= ?", time.Now().Unix())
	}
	if err != nil {
		db.Close()
		return nil, fmt.Errorf("initialize database: %w", err)
	}
	return s, nil
}

func (s *store) migrate(ctx context.Context) error { return s.applyMigrations(ctx, migrations) }

func (s *store) applyMigrations(ctx context.Context, files fs.FS) error {
	if _, err := s.db.ExecContext(ctx, `CREATE TABLE IF NOT EXISTS schema_migrations (
		version INTEGER PRIMARY KEY, checksum TEXT NOT NULL, applied_at INTEGER NOT NULL
	) STRICT`); err != nil {
		return err
	}
	names, err := fs.Glob(files, "migrations/*.sql")
	if err != nil {
		return err
	}
	known := make(map[int]bool)
	for _, name := range names {
		version, err := strconv.Atoi(strings.SplitN(filepath.Base(name), "_", 2)[0])
		if err != nil || version < 1 || known[version] {
			return errors.New("invalid migration version")
		}
		known[version] = true
		body, err := fs.ReadFile(files, name)
		if err != nil {
			return err
		}
		checksum := fmt.Sprintf("%x", sha256.Sum256(body))
		tx, err := s.db.BeginTx(ctx, nil)
		if err != nil {
			return err
		}
		var applied string
		err = tx.QueryRowContext(ctx, "SELECT checksum FROM schema_migrations WHERE version = ?", version).Scan(&applied)
		switch {
		case err == nil && applied != checksum:
			err = fmt.Errorf("migration %d checksum changed", version)
		case errors.Is(err, sql.ErrNoRows):
			_, err = tx.ExecContext(ctx, string(body))
			if err == nil {
				_, err = tx.ExecContext(ctx, "INSERT INTO schema_migrations VALUES (?, ?, ?)", version, checksum, time.Now().Unix())
			}
		}
		if err != nil {
			tx.Rollback()
			return err
		}
		if err = tx.Commit(); err != nil {
			return err
		}
	}
	rows, err := s.db.QueryContext(ctx, "SELECT version FROM schema_migrations")
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var version int
		if err = rows.Scan(&version); err != nil {
			return err
		}
		if !known[version] {
			return fmt.Errorf("database migration %d is newer than this binary", version)
		}
	}
	return rows.Err()
}

func (s *store) configured(ctx context.Context) (bool, error) {
	var exists bool
	err := s.db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM owner WHERE id = 1)").Scan(&exists)
	return exists, err
}

func (s *store) createOwner(ctx context.Context, username string, hash passwordHash, now time.Time) error {
	return s.insertOwner(ctx, username, hash, now, func() error { return nil })
}

func (s *store) insertOwner(ctx context.Context, username string, hash passwordHash, now time.Time, active func() error) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return errUnavailable
	}
	defer tx.Rollback()
	if err = active(); err != nil {
		return err
	}
	var exists bool
	if err = tx.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM owner)").Scan(&exists); err != nil {
		return errUnavailable
	}
	if exists {
		return errOwnerExists
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO owner(id, username, password_hash, created_at) VALUES (1, ?, ?, ?)", username, hash.encoded, now.Unix()); err != nil {
		return errUnavailable
	}
	if err = tx.Commit(); err != nil {
		return errUnavailable
	}
	return nil
}

func (s *store) withSession(ctx context.Context, token sessionToken, now func() time.Time, operation func(*sql.Tx, session) error) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return errUnavailable
	}
	defer tx.Rollback()
	digest := token.digest()
	var value session
	var created, expires int64
	err = tx.QueryRowContext(ctx, "SELECT id, created_at, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?", digest[:], now().Unix()).Scan(&value.id, &created, &expires)
	if errors.Is(err, sql.ErrNoRows) {
		return errUnauthorized
	}
	if err != nil {
		return errUnavailable
	}
	value.createdAt, value.expiresAt = time.Unix(created, 0).UTC(), time.Unix(expires, 0).UTC()
	if err = operation(tx, value); err != nil {
		return err
	}
	if err = tx.Commit(); err != nil {
		return errUnavailable
	}
	return nil
}

func (m *manager) currentSession(ctx context.Context, token sessionToken) (session, owner, error) {
	var current session
	var profile owner
	err := m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, value session) error {
		current = value
		var created int64
		if err := tx.QueryRowContext(ctx, "SELECT username, created_at FROM owner WHERE id = 1").Scan(&profile.username, &created); err != nil {
			return errUnavailable
		}
		profile.createdAt = time.Unix(created, 0).UTC()
		return nil
	})
	return current, profile, err
}

func (m *manager) listBots(ctx context.Context, token sessionToken) ([]bot, error) {
	values := make([]bot, 0)
	err := m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, _ session) error {
		rows, err := tx.QueryContext(ctx, "SELECT id, name, created_at FROM bots ORDER BY created_at, id")
		if err != nil {
			return errUnavailable
		}
		defer rows.Close()
		for rows.Next() {
			var value bot
			var created int64
			if err = rows.Scan(&value.id, &value.name.value, &created); err != nil {
				return errUnavailable
			}
			value.createdAt = time.Unix(created, 0).UTC()
			values = append(values, value)
		}
		if rows.Err() != nil {
			return errUnavailable
		}
		return nil
	})
	return values, err
}

func (m *manager) createBot(ctx context.Context, token sessionToken, name botName) (bot, error) {
	value := bot{id: botID(randomID()), name: name}
	err := m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, _ session) error {
		value.createdAt = m.now().UTC().Truncate(time.Second)
		if _, err := tx.ExecContext(ctx, "INSERT INTO bots(id, name, created_at) VALUES (?, ?, ?)", value.id, name.value, value.createdAt.Unix()); err != nil {
			return errUnavailable
		}
		return nil
	})
	return value, err
}

func (m *manager) listSessions(ctx context.Context, token sessionToken) ([]session, error) {
	values := make([]session, 0)
	err := m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, _ session) error {
		rows, err := tx.QueryContext(ctx, "SELECT id, created_at, expires_at FROM sessions WHERE expires_at > ? ORDER BY created_at, id", m.now().Unix())
		if err != nil {
			return errUnavailable
		}
		defer rows.Close()
		for rows.Next() {
			var value session
			var created, expires int64
			if err = rows.Scan(&value.id, &created, &expires); err != nil {
				return errUnavailable
			}
			value.createdAt, value.expiresAt = time.Unix(created, 0).UTC(), time.Unix(expires, 0).UTC()
			values = append(values, value)
		}
		if rows.Err() != nil {
			return errUnavailable
		}
		return nil
	})
	return values, err
}

func (m *manager) revokeSession(ctx context.Context, token sessionToken, target sessionID) error {
	return m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, _ session) error {
		if _, err := tx.ExecContext(ctx, "DELETE FROM sessions WHERE id = ? AND owner_id = 1", target); err != nil {
			return errUnavailable
		}
		return nil
	})
}

func (m *manager) signOut(ctx context.Context, token sessionToken) error {
	return m.store.withSession(ctx, token, m.now, func(tx *sql.Tx, value session) error {
		if _, err := tx.ExecContext(ctx, "DELETE FROM sessions WHERE id = ?", value.id); err != nil {
			return errUnavailable
		}
		return nil
	})
}
