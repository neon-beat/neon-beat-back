# Neon Beat back

Neon Beat back is the Rust backend powering Neon Beat, a homemade blind test experience built around playlists, teams, and fast-paced buzzer rounds.

## Highlights

- **RESTful API**: Provides a well-defined RESTful API for programmatic access to its functionalities.
- **Real-time communications**: Real-time communications via WebSockets for buzzers and Server-Sent Events for the public and admin UIs.
- **MongoDB persistence**: Uses a MongoDB connection to keep playlists, teams, and game progress in sync. Playlists are stored in their own collection so games can reuse curated track lists without re-importing them each time.
- **Swagger UI**: The full OpenAPI document is generated with utoipa and served through Swagger UI (`/docs`) for quick manual testing.

## Architecture Overview

### Module layout
The Neon Beat back project follows a layered architecture, separating concerns into distinct modules:
- **`routes`**: This layer handles incoming HTTP requests and defines the API endpoints. It is responsible for parsing requests, calling the appropriate service methods, and returning HTTP responses.
- **`services`**: This layer contains the business logic of the application. It orchestrates operations, interacts with the `dao` layer to retrieve or store data, and applies any necessary transformations or validations.
- **`dao` (Data Access Object)**: This layer is responsible for interacting with external data sources or systems, such as a MongoDB database. It abstracts the details of data persistence and retrieval from the service layer.
  - **`models`**: This submodule within the `dao` layer defines the data models that represent the entities and structures used when interacting with external systems. These models ensure consistent data representation across the application's interactions with various data sources.
- **`dto` (Data Transfer Object)**: This layer defines the data structures used for transferring data between different layers of the application, particularly between the `routes` and `services` layers, and for external API communication. These structures ensure consistent data formats.
- **`state`**: Centralises runtime state kept in memory while the server runs. It exposes the finite-state machine that coordinates gameplay, the in-memory `GameSession`/playlist data used by services and DTOs, the SSE hubs, and shared resources such as buzzer connections.

### System interactions
```mermaid
flowchart LR
    subgraph Neon Beat Backend
        REST(REST API routes) --> StateMachine(STATE MACHINE)
        WS(WebSocket Connection) <--> StateMachine
        StateMachine --> MongoDbDao(MongoDB DAO)
        StateMachine --> SSE(SSE Connection)
    end

    subgraph Frontends
        PublicFront[Public Frontend] --> REST
        AdminFront[Admin Frontend] --> REST
        SSE --> PublicFront
        SSE --> AdminFront
    end

    subgraph Buzzers
        Buzzer1[Buzzer 1] <--> WS
        Buzzer2[Buzzer 2] <--> WS
    end

    MongoDbDao --> MongoDbInstance[MongoDB Instance]
```

### Game state flow
```mermaid
stateDiagram-v2
    [*] --> idle

    note right of [*]
        GM: Game Master
    end note
    note left of idle
        Playlist and players management. Visible in admin front
    end note

    idle --> game_running: start_game
    state game_running {
        prep --> playing_game: game_configured
        playing_game --> pause: GM_triggers_pause
        playing_game --> pause: buzz
        pause --> reveal: game_master_triggers_reveal
        pause --> playing_game: game_master_triggers_continue
        reveal --> playing_game: GM_triggers_next
        playing_game --> reveal: GM_triggers_reveal
    }
    game_running --> show_scores: playlist_end_or_GM_stops
    show_scores --> idle: GM_ends_game
```

## Core gameplay features

- **Playlist import & persistence**: JSON playlists where each song has some basic properties and are persisted atomically:
   - Timestamp at which the song should start
   - Time to identify a song
   - Song URL
   - "Point fields" are fields to find for the song, that can give points to a team: for example, song name and artist (this list of field is dynamic and not empty)
   - "Bonus point fields" are optional fields to find for the song, that can give bonus points to a team (this list of field is dynamic and may be empty)
   - During game bootstrap the playlist song order is shuffled once to create a random play sequence; persisted games must provide the same identifiers to guarantee consistency.
- **Game bootstrap**: Game can be created or loaded (from database) during the idle state:
   - the game contains a list of players (players have a unique buzzer, a name and a score)
   - the game references a persisted playlist entity (shared across games) which is embedded into the runtime session when the game starts
   - the game contains a game state (frequently saved in database), which contains a playlist state (the playlist state remembers whether a song has been played or not) and must match the playlist identifiers exactly
- **State machine execution**: Gameplay transitions follow the diagram above (`Game state flow`), persisting progress and orchestrating pauses, reveals, and scoring.
- **Admin controls (REST)**:
   - create/load games return a `GameSummary` payload bundling players, shuffled playlist ordering, and timestamps
   - pause the current song
   - resume the current song
   - add/remove points to a team
   - reveal the current song
   - mark a field as "found"
   - validate/invalidate an answer
- **Public API (REST)**: 
   - get teams infos
   - get current song infos
- **WebSocket connection for buzzers**
   - Buzzers connect to `GET /ws` and identify themselves by sending:
     ```json
     { "type": "identification", "id": "<mac_address>" }
     ```
     where `<mac_address>` is a 12-character lowercase hex string.
   - Buzz events reuse the same id:
     ```json
     { "type": "buzz", "id": "<mac_address>" }
     ```
   - Buzzers need to be informed when:
      - Game is paused and it's the team's turn to answer
      - Team's turn is finished and the game resumes
- **SSE connection for frontends**: Admin and public frontends subscribe via `/sse/admin` and `/sse/public`. The admin stream issues a one-time token and enforces a single active admin connection.

## Getting started

### Prerequisites
- Rust toolchain (1.85+ recommended)
- MongoDB instance reachable at `MONGO_URI`

### Build from source
```bash
cargo build
```
Binaries live under `target/debug/neon-beat-back` or `target/release/neon-beat-back`.

### Environment

The server relies on a running MongoDB instance and the following optional environment variables:

| Variable     | Default                     | Description |
|--------------|-----------------------------|-------------|
| `MONGO_URI`  | `mongodb://localhost:27017` | Connection string used to create the MongoDB client. |
| `MONGO_DB`   | `neon_beat`                 | Database name (only used when provided). |
| `PORT`       | `8080`                      | TCP port the HTTP server binds to. `SERVER_PORT` is also honoured for compatibility. |

### Run locally
1. Ensure MongoDB is running (configure `MONGO_URI`/`MONGO_DB` if needed).
2. Start the backend:
   ```bash
   cargo run
   ```
3. Try a few entry points:
   - Healthcheck: `curl http://localhost:8080/healthcheck`
   - Public SSE stream: `curl -N http://localhost:8080/sse/public`
   - Admin SSE stream (one active client): `curl -N http://localhost:8080/sse/admin`
   - WebSocket buzzer (after `cargo install websocat`):
     ```bash
     ( printf '{"type":"identification","id":"deadbeef0001"}\n'; cat ) | websocat -t ws://localhost:8080/ws
     ```
   - Swagger UI: open `http://localhost:8080/docs`

### Docker

Build the release image with Docker:

```bash
docker build -t neon-beat-back .
```

For cross-compilation, provide a Rust target triple via the optional build argument:

```bash
docker build -t neon-beat-back --build-arg BUILD_TARGET=aarch64-unknown-linux-gnu .
```

When using docker-compose (e.g. after copying `docker-compose.example.yaml` as `docker-compose.yaml`), the same argument can be passed through an environment variable:

```bash
BUILD_TARGET=aarch64-unknown-linux-gnu docker compose build
```

## Roadmap

- [x] Implement the Rest API with a /healthcheck route
- [x] Implement the WebSocket connection
- [x] Implement the SSE connection
- [x] Implement the MongoDB connection bootstrap
- [x] Implement the Swagger UI documentation
- [x] MongoDB auto-reconnection
- [x] Admin SSE deconnexion management
- [ ] Validate the Rest API /healthcheck route
- [ ] Validate the WebSocket connection
- [ ] Validate the SSE connection
- [ ] Validate the MongoDB connection
- [x] Do not block start of app if connexion to MongoDB fails (Authorize "degraded mode" → info to send by SSE)
- [x] Implement Game State Machine
- [ ] Use Game State Machine
- [x] Implement Game & Playlist State save in DB (only found songs, not every answered field)
- [ ] Implement admin routes (pause, resume, add/remove points, mark field as found, reveal, validate/invalidate answer)
- [ ] Implement admin routes to manage persisted playlists (DAO support available)
- [ ] Implement public routes (teams, scores, countdown, song url)
- [ ] Implement buzzer feedback
- [ ] Remove useless features of dependencies if found
- [ ] Allow to switch buzzer_id for a player
- [ ] Update `mongo` value of `AppState ` to None (and send False to `degraded` watcher) each time a mongo function returns a connection error
- [ ] Implement tests

## Questions

- Do we want to manage public SSE deconnexion ? No
- Do we want to modify a playlist when it is already imported in the backend ? No, we import again

TODO: Manage MongoDB disconnection for mongo function errors
