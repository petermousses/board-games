CREATE TYPE game_type AS ENUM ('solitaire', 'checkers');
CREATE TYPE session_status AS ENUM ('lobby', 'active', 'complete');

CREATE TABLE game_sessions (
  id UUID PRIMARY KEY,
  game_type game_type NOT NULL,
  state JSONB NOT NULL,
  state_version BIGINT NOT NULL DEFAULT 0 CHECK (state_version >= 0),
  status session_status NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE session_participants (
  id UUID PRIMARY KEY,
  session_id UUID NOT NULL REFERENCES game_sessions(id) ON DELETE CASCADE,
  seat TEXT NOT NULL CHECK (seat IN ('solitaire', 'red', 'black')),
  display_name VARCHAR(32) NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 32),
  token_hash BYTEA NOT NULL CHECK (octet_length(token_hash) = 32),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (session_id, seat)
);

CREATE TABLE game_events (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  session_id UUID NOT NULL REFERENCES game_sessions(id) ON DELETE CASCADE,
  state_version BIGINT NOT NULL CHECK (state_version > 0),
  participant_id UUID NOT NULL REFERENCES session_participants(id),
  action JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (session_id, state_version)
);

CREATE INDEX game_events_session_version_idx ON game_events (session_id, state_version DESC);
CREATE INDEX session_participants_session_idx ON session_participants (session_id);
