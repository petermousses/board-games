ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'chess';
ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'battleship';
ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'clue';
ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'connect_four';
ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'reversi';
ALTER TYPE game_type ADD VALUE IF NOT EXISTS 'tic_tac_toe';

-- Retain legacy seats and snapshots while allowing six-player tables.
ALTER TABLE session_participants DROP CONSTRAINT session_participants_seat_check;
ALTER TABLE session_participants ADD CONSTRAINT session_participants_seat_check
  CHECK (seat IN ('solitaire', 'red', 'black', 'player1', 'player2', 'player3', 'player4', 'player5', 'player6'));
