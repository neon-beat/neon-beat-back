# Changelog

All notable changes to this project will be documented in this file.

## [v0.9.0] - Expand blindtests into quiz question sequences

### Added
- **Quiz question model**: Games now use questions sequences instead of blindtest-only playlists.
  A sequence can contain blindtest, multiple-choice, and open-text questions.
- **Question variants**:
  - Blindtest questions keep media playback metadata and now store answers as ID-addressable maps.
  - Multiple-choice questions support prompts, optional media URLs, up to four answers, correctness flags, and hints.
  - Open questions support prompts, optional media URLs, accepted answers, and hints.
- **Auto-assigned IDs**: Imported answers and hints receive zero-based `u8` IDs so gameplay events can reference stable answer and hint identifiers.
- **Question gameplay routes**:
  - `POST /admin/game/question/answer-found`
  - `POST /admin/game/question/hint`
  - `POST /admin/game/question/submit-validation`
- **Questions sequence routes**:
  - `GET /admin/questions-sequence`
  - `POST /admin/questions-sequence`
  - `POST /admin/games/with-questions-sequence`
  - `GET /public/question`
- **Question SSE events**:
  - `question.found_answers`
  - `question.hints`
  - `question.validation`
- **Public question snapshots**: Public phase and current-question responses now expose independent question snapshot DTOs for API stability.
- **Blindtest hint support**: Blindtest hints reveal answer IDs without marking those answers as found.

### Changed
- **Version**: Bumped crate version from `0.8.3` to `0.9.0`.
- **Core naming**: Renamed runtime, DTO, route, storage, and documentation concepts from playlist/song/field terminology to questions sequence/question/answer terminology.
- **Persistence schema**: Replaced playlist/song persistence entities with questions sequence/question entities for MongoDB and CouchDB stores.
- **Game state**: Games now persist `questions_sequence_id`, `question_order`, `current_question_index`, and current question reveal state.
- **Gameplay tracking**: Current progress now tracks found answer IDs and revealed hint IDs instead of point and bonus field keys.
- **REST responses**:
  - Game list items now expose `questions_sequence`.
  - Start/next responses now use `question` / `NextQuestionResponse`.
  - Public current question responses include `question`, `answers_ids`, and `hints_ids`.
- **Validation**:
  - Questions sequence inputs are validated at the HTTP boundary with nested question validation.
  - Multiple-choice imports reject more than four answers.
  - Multiple-choice and open question prompts and durations are validated.
  - Blindtest and open questions require at least one answer.
- **Multiple-choice public data**: Public multiple-choice answers currently expose both `text` and `is_correct`.
- **OpenAPI documentation**: Registered the new question, answer, hint, snapshot, and legacy schemas in the generated API documentation.
- **Docker build image**: Updated the Rust Docker base image from `1.90-slim` to `1.95-slim` and simplified `cargo-chef` reuse across build stages.
- **README, protocols, and roadmap**: Updated project wording and examples from blindtest/playlist/song language to quiz/questions sequence/question language.

### Migration notes for REST and SSE clients
- **REST route replacements**:
  - `GET /public/song` -> `GET /public/question`
  - `POST /admin/game/answer` -> `POST /admin/game/question/submit-validation`
  - `POST /admin/game/fields/found` -> `POST /admin/game/question/answer-found`
  - `GET /admin/playlists` -> `GET /admin/questions-sequence`
  - `POST /admin/playlists` -> `POST /admin/questions-sequence`
  - `POST /admin/games/with-playlist` -> `POST /admin/games/with-questions-sequence`
- **REST payload and response field replacements**:
  - `playlist` -> `questions_sequence`
  - `playlist_id` -> `questions_sequence_id`
  - `song` -> `question`
  - `song_id` -> `question_id`
  - `current_song_index` -> `current_question_index`
  - `song_found` / `current_song_found` concepts -> `question_revealed` / `current_question_revealed`
  - found point and bonus field keys -> `answers_ids`
  - revealed hint identifiers are exposed through `hints_ids`
- **Question import format**: New sequence imports use `questions` with tagged items:
  - `type: "blind_test"` for blindtest questions
  - `type: "multiple_choice"` for multiple-choice questions
  - `type: "open"` for open-text questions
- **Answer addressing**: Clients should store and send numeric answer IDs instead of blindtest field keys when marking progress.
  The backend assigns answer and hint IDs during questions sequence import.
- **SSE event replacements**:
  - `fields_found` -> `question.found_answers` with `{question_id, answers_ids}`
  - `answer_validation` -> `question.validation` with `{question_id, valid}`
  - `question.hints` is new and emits `{question_id, hints_ids}`
- **Compatibility window**: Deprecated playlist routes still accept legacy playlist payloads, but new persisted documents use the questions sequence schema only.
  Existing stored playlist/song documents must be migrated or re-imported.

### Deprecated
- **Legacy playlist routes** remain available as compatibility aliases but are marked deprecated in Rust and OpenAPI/Swagger:
  - `GET /admin/playlists` -> use `GET /admin/questions-sequence`
  - `POST /admin/playlists` -> use `POST /admin/questions-sequence`
  - `POST /admin/games/with-playlist` -> use `POST /admin/games/with-questions-sequence`
- **Legacy DTOs** are marked deprecated:
  - `LegacyPlaylistListItem`
  - `LegacyCreateGameWithPlaylistRequest`
  - `LegacyPlaylistInput`
  - `LegacySongInput`
  - `LegacyPointFieldInput`
- **Legacy service helpers** are marked deprecated and kept only for compatibility:
  - `list_legacy_playlists`
  - `is_legacy_playlist_sequence`

### Removed
- Replaced old public/admin route names in the main API surface:
  - `GET /public/song`
  - `POST /admin/game/answer`
  - `POST /admin/game/fields/found`
- Old stored playlist/song documents are no longer supported by the new persisted schema.

### Testing
- Added and updated tests for importing all question variants, auto ID assignment, empty sequence rejection, URL validation, multiple-choice answer-count validation, legacy playlist conversion, question validation events, answer-found events, and hint reveal validation.

## [v0.8.3] - Replace panics and expects by errors

### Changed
- Replace panics and expects by errors
- Rewrite the README file

## [v0.8.2] - API documentation on GitHub Pages

### Added
- **GitHub Pages deployment**: OpenAPI/Swagger documentation automatically deployed on push to `main`
- **`openapi-generator` binary**: Generates OpenAPI 3.1 spec from Rust code

### Changed
- **Library crate**: Added `src/lib.rs` to expose modules for binaries and tests

## [v0.8.1] - Buzzer reliability & state management improvements

### Added
- **Buzzer reconnection**: Buzzers restore their last known pattern on reconnection instead of defaulting to WaitingForPairing

### Changed
- **Team persistence**: Removed `buzzer_id` from database storage (session-only, teams must be re-paired after game load)
- **State clearing**: Added `clear_game_state()` method to clear all persistence state and buzzer patterns (fixes throttling bug)
- **Error handling**: Buzzer communication failures now log warnings instead of blocking operations
- **Type safety**: Simplified `TryFrom<MongoTeamDocument>` to `From`

## [v0.8.0] - Reject unknown query param & add fallback route

### Added
- **Input validation** using `validator` crate with `axum-valid` for HTTP routes
  - Buzzer IDs: 12 lowercase hexadecimal characters
  - Song URLs: valid URL format
  - Team colors: HSV saturation/value range 0.0-1.0
  - WebSocket messages validated via `BuzzerInboundMessage::from_json_str()`
- Fallback route handler for unmatched endpoints (404 JSON response)

### Changed
- Fix the `POST /admin/games/:id/load` route
- HTTP routes use `Valid<Json<T>>` extractors for automatic validation
- `BuzzerInboundMessage` no longer has `Unknown` variant (rejected during parsing)
- Query parameters reject unknown fields via `#[serde(deny_unknown_fields)]`

### Removed
- Redundant `sanitize_buzzer_id()` validation (now handled at boundaries)

## [v0.7.1] - Shuffle playlist at game creation/loading (instead of starting)

- Shuffle on `POST /admin/games`, `POST /admin/games/with-playlist` and also (but only if game is not started or has a completed playlist) `POST /admin/games/{id}/load` (instead of `POST /admin/games/start`)
- Return an error if shuffle is requested but not possible

## [v0.7.0] - Persistence Layer Improvements

### Added
- **Debouncing mechanism** for persistence operations (200ms cooldown) to prevent data loss during rapid updates while reducing database load
- **Graceful shutdown** handler that flushes all pending updates before termination
- **Per-team locking** to prevent concurrent write conflicts while allowing parallel team updates
- **Optimistic retry** for CouchDB write operations with exponential backoff
- **Comprehensive documentation** of persistence architecture (see README)

### Changed
- **Separated team documents** from game documents to avoid write hotspots and improve scalability
- **Persistence coordinator** now centralizes all locking, throttling, and debouncing logic

See the [Persistence Architecture](README.md#persistence-architecture) section in the README for detailed information.

## [v0.6.0] - Send patterns to buzzers through BuzzerOutboundMessage

- Add a `BuzzerOutboundMessage` for sending patterns to the WebSocket clients
- Add defaults patterns for the "buzzer phases": WaitingForPairing, Standby, Playing, Waiting and Answering
- Add an entry in the config to override any pattern (for a "buzzer phase")
- Remove unused `BuzzerAck` and `BuzzFeedback` WebSocket messages

## [v0.5.6] - Add config (colors set for now) for app & use colors set to give color to teams at creation

- Add a config (JSON file + Rust struct) for the app, containing only the colors set for now. The config file location is `config/app.json` by default, but can set by the `NEON_BEAT_BACK_CONFIG_PATH` environment variable. If the file is absent or malformed the backend continues with the built-in colors set.
- When teams are created without an explicit color, the first unused color from the colors set is assigned automatically.
- Replace some `unwrap_or` by `unwrap_or_else` or `match` (lazy evaluation instead of leager avaluation).

## [v0.5.5] - Add a color for teams (defaults to white for now) & update REST requests for teams

- Add color for teams (defaults to white for now)
- Update teams attribute for `POST /admin/games` and `POST /admin/games/with-playlist` and request body for `POST /admin/teams` and `PUT /admin/teams/{id}` (common now):
   - buzzer_id: optional ; if null is specified, removes the buzzer ID ; if a string is specified, sets the buzzer ID to this string.
   - color: optional ; if a color is specified, sets the team's color.
   - name: required
   - score: optional ; if a score is specified, sets the team's score.

## [v0.5.4] - Add an outer helper tool to generate colors

- Add an outer helper tool to generate colors

## [v0.5.3] - Bugfix when an unpaired buzzer buzzes in PrepReady & save song finished info

- Fix the bug when an unpaired buzzer buzzes in PrepReady maked the game stucked
- Rubustify GameSession concurent access
- Send the team who buzzed in the GET phase route and the SSE event
- Save the information that a song has been found (to be able to switch to next song if the game restarts) => Needs to clear the database to use this version !

## [v0.5.2] - Keep playlist song order

- Keep playlist song order (from JSON) if no shuffle => Needs to clear the database to use this version !
- Log a warning if a connected buzzer is not paired while launching the game
- Implement TryFrom instead of From to convert (GameListItemEntity, PlaylistEntity) into GameListItem
- Remove unecessary pub(crate) functions
- Replace Vec<Team> by an IndexMap<Team> in GameSession

## [v0.5.1] - Add optional shuffle query parameter for POST /admin/game/start

- `POST /admin/game/start` accepts an optional `shuffle` query parameter to reshuffle the playlist when it hasn't started yet or after completion.

## [v0.5.0] - Change the answer validation from a boolean to a tri-state (correct, incomplete or wrong)

- Change POST /admin/game/answer request body's valid field from a boolean to a tri-state (correct, incomplete or wrong)
- Change SSE answer_validation data's valid field from a boolean to a tri-state (correct, incomplete or wrong)

## [v0.4.0] - Change POST /admin/game/score into /admin/teams/{id}/score

- Change POST /admin/game/score into /admin/teams/{id}/score, remove buzzer_id field from request body, and change buzzer_id field of response body into team_id

## [v0.3.2] - Add DELETE /admin/games/:id route

- Added `DELETE /admin/games/{id}` to remove stored games (fails if the game is currently running).

## [v0.3.1] - Don't modify the game when it is manually stopped & Allow New Game + sessions

- Don't modify the game when it is manually stopped (bugfix)
- Allow New Game + sessions for playlist completed games : after a game with a completed playlist is loaded, starting it will treat the game as a fresh session (and stopping it will show the scores as usual)

## [v0.3.0] - Add authentication for admin routes

- All `/admin/**` routes now require the `X-Admin-Token` header. The value is issued via the admin SSE handshake (`/sse/admin`).

## [v0.2.1] - Set default tower_http (and every other module) log verbosity level to info

- Set default tower_http (and every other module) log verbosity level to info

## [v0.2.0] - Harmonize naming between teams and players (team chosen)

- Replace player/players occurences by team/teams:
   - GET /admin/games route: teams field replaces players attribute in response body items
   - POST /admin/games & POST /admin/games/with-playlist routes: teams field replaces players attribute in request and response bodies

## [v0.1.5] - Add GET /admin/games/:id route and add game_id to GET /public/phase route response

- Add GET /admin/games/:id route
- Add game_id to GET /public/phase route response

## [v0.1.4] - Add more fields to the GET /admin/games response

- Add players (names and ids), playlist (name and id), created_at and updated_at to the GET /admin/games response

## [v0.1.3] - Fix game creation (without players or with players with no buzzer ID)

- PlayerInput: `buzzer_id` is now optional (changed to `Option<String>`).
- Game creation and startup validation tightened:
	- `create_game` will accept empty player lists and build an empty player vector.
	- `start_game` now returns an error when attempting to start a game with zero players.

## [v0.1.2] - Add team/buzzer pairing and fix GET /admin/playlists

### Interface changes

#### REST
- Added admin team management endpoints: `POST /admin/teams` to create teams, `PUT /admin/teams/{id}` to update them, and `DELETE /admin/teams/{id}` to remove them.
- Added pairing workflow endpoints: `POST /admin/teams/pairing` to start pairing and `POST /admin/teams/pairing/abort` to abort pairing. The abort endpoint now returns the restored roster (`Vec<TeamSummary>`).
- Game bootstrap endpoints (`POST /admin/games`, `POST /admin/games/with-playlist`, `POST /admin/games/{id}/load`) now trigger a `game.session` SSE snapshot after completion.

#### SSE
- Introduced `team.updated` and `team.deleted` events on the public stream so UIs can track roster mutations without refetching.
- Added `game.session` (public-only) to broadcast a full game snapshot whenever a game is created or loaded.
- Pairing events (`pairing.waiting`, `pairing.assigned`, `pairing.restored`) are now emitted on both public and admin streams.

#### WebSocket
- Buzzers continue to exchange `identification`, `buzz`, and `BuzzFeedback` messages; the documentation now specifies the expected acknowledgement flow and reconnection behaviour.

### Other changes
- Fixed CouchDB playlist deserialisation so playlists created via the REST API can be listed without errors.
- Updated README realtime documentation to match the new SSE and WebSocket payloads.

- State-machine driven pairing – entering pairing mode (`POST /admin/teams/pairing`) now transitions the game FSM, guaranteeing that pairing actions only occur during prep. Aborting via `POST /admin/teams/pairing/abort` restores the saved snapshot automatically.
- Incremental pairing updates – buzzer assignments emit `pairing.assigned` while the next team in the queue is announced through `pairing.waiting`. When the final team is paired the state machine exits pairing without additional API calls.
- Targeted roster updates – removing a team with `DELETE /admin/teams/{id}` now broadcasts the compact `team.deleted` SSE payload (team UUID only). Clients should remove the team locally instead of waiting for a full roster refresh.
- Buzzer feedback loop – WebSocket buzzers receive an explicit `BuzzFeedback` acknowledgement after each pairing buzz so devices can signal success or rejection immediately.
- Shared pairing events – pairing events (`pairing.waiting`, `pairing.assigned`, `pairing.restored`) are now published on both admin and public SSE channels, ensuring every UI stays synchronised.

See the [Pairing workflow (v0.1.2+)](README.md#pairing-workflow-v012) section in the README for end-to-end examples and payload formats.

## [v0.1.1] - Initial release

- First public release of the Neon Beat backend, shipping the REST API, WebSocket buzzers, SSE streams, MongoDB/CouchDB stores, and the gameplay state machine.
