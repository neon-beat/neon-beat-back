# Protocol Documentation

This document specifies Neon Beat's real-time communication protocols for WebSocket and Server-Sent Events.

## Overview

The system provides two real-time channels to keep all clients (admin frontends, public displays, and physical buzzers) synchronized:

- **WebSocket (`/ws`)**: Bidirectional communication for physical buzzer devices
- **Server-Sent Events (`/sse/*`)**: Unidirectional event streams for frontends

---

## WebSocket Protocol

Physical buzzers connect via WebSocket at `ws://[host]/ws` to identify themselves, send buzz events, and receive LED pattern commands.

### Message Types

#### Client → Server Messages

**Identification** - Must be first message after connecting (10 second timeout):
```json
{"type": "identification", "id": "deadbeef0001"}
```
- `id`: 12-character lowercase hexadecimal MAC address (pattern: `^[0-9a-f]{12}$`)

**Buzz** - Sent when buzzer is pressed (during pairing or gameplay):
```json
{"type": "buzz", "id": "deadbeef0001"}
```
- `id`: Must match ID from identification

#### Server → Client Messages

The server sends LED pattern commands wrapped in a `pattern` object. Pattern details (for blink/wave):
- `duration_ms`: Pattern duration (0 = infinite loop)
- `period_ms`: Cycle period in milliseconds
- `dc`: Duty cycle (0.0 to 1.0)
- `color`: HSV object with `h` (0-360°), `s` (0-1), `v` (0-1)

**Pattern Types:**

**Blink** - LED blinks on/off (confirmation, waiting states):
```json
{
  "pattern": {
    "type": "blink",
    "details": {
      "duration_ms": 0,
      "period_ms": 200,
      "dc": 0.5,
      "color": {"h": 125.0, "s": 1.0, "v": 1.0}
    }
  }
}
```

**Wave** - LED brightness waves smoothly (breathing effect, standby):
```json
{
  "pattern": {
    "type": "wave",
    "details": {
      "duration_ms": 0,
      "period_ms": 2000,
      "dc": 0.5,
      "color": {"h": 240.0, "s": 1.0, "v": 1.0}
    }
  }
}
```

**Off** - Turn off all LEDs:
```json
{"pattern": {"type": "off"}}
```

### Color System (HSV)

- **Hue (h)**: Color angle 0-360° (0=Red, 120=Green, 240=Blue)
- **Saturation (s)**: Color intensity 0-1 (0=grayscale, 1=vivid)
- **Value (v)**: Brightness 0-1 (0=off, 1=max brightness)

Common colors: Red (0°), Yellow (60°), Green (120°), Cyan (180°), Blue (240°), Magenta (300°)

### Connection & State

- **Flow:** Connect → `identification` (within 10s) → Receive pattern → `buzz` → Pattern updates
- **Reconnection:** Auto-reconnect → Re-identify → Pattern restored
- **States:** Unidentified → Identified → Paired → Active → Disconnected
- **Persistence:** Patterns preserved per buzzer ID across disconnections; cleared on team removal or game reset

### Game Phase Integration

LED patterns change based on game phase:
- **Prep**: Off/idle (unassigned), waiting pattern (pairing), confirmation (paired)
- **Prep-Ready**: Standby with team color wave
- **Playing**: Active team color, bright on buzz, dimmed for waiting
- **Answering**: Bright for answering team, dimmed/off for others
- **Post-Answer**: Success (green) or error (red) pattern

### Error Handling & Implementation

- **Connection errors:** 10s timeout closes connection; malformed JSON ignored; mismatched IDs rejected
- **Pattern validation:** Invalid values clamped to valid ranges
- **Hardware:** Auto-reconnect, HSV→RGB conversion, pattern buffering
- **Server:** Preserve patterns per ID, newest connection wins for duplicate IDs

---

## Server-Sent Events

Two SSE streams provide real-time updates to frontend applications:
- **`/sse/public`**: 14 events, no authentication
- **`/sse/admin`**: 7 events, token authentication, single connection enforced

### Event Types

| Event | Payload | Public | Admin | Description |
|-------|---------|:------:|:-----:|-------------|
| `handshake` | `{stream, message, degraded, token?}` | ✓ | ✓ | Initial connection (admin gets token) |
| `system_status` | `{degraded}` | ✓ | ✓ | Database connection status change |
| `game.session` | `GameSummary` | ✓ | ✗ | Complete game state snapshot |
| `phase_changed` | `GamePhaseSnapshot` | ✓ | ✓ | Game phase transition |
| `team.created` | `{team: TeamSummary}` | ✓ | ✓ | New team added |
| `team.updated` | `{team: TeamSummary}` | ✓ | ✗ | Team name, buzzer, or score changed |
| `team.deleted` | `{team_id}` | ✓ | ✗ | Team removed |
| `fields_found` | `{song_id, point_fields[], bonus_fields[]}` | ✓ | ✗ | Fields marked as found |
| `answer_validation` | `{valid: "correct"\|"incomplete"\|"wrong"}` | ✓ | ✗ | Answer validation result |
| `score_adjustment` | `TeamSummary` | ✓ | ✗ | Team score manually adjusted |
| `pairing.waiting` | `{team_id}` | ✓ | ✓ | Waiting for team to pair buzzer |
| `pairing.assigned` | `{team_id, buzzer_id}` | ✓ | ✓ | Buzzer successfully paired |
| `pairing.restored` | `{snapshot: TeamSummary[]}` | ✓ | ✗ | Pairing aborted, teams restored |
| `test.buzz` | `{team_id}` | ✓ | ✓ | Buzzer pressed during prep-ready |

### Authentication & Format

- **Admin Flow:** Connect → Receive token in `handshake` → Use in `X-Admin-Token` header for REST calls
- **Format:** Standard SSE (`event:` type, `data:` JSON payload)
- **Connection:** Keep-alive every 15s; admin must re-authenticate on reconnect

---

## Protocol Comparison

| Feature | WebSocket (`/ws`) | SSE Public | SSE Admin |
|---------|-------------------|------------|-----------|
| **Direction** | Bidirectional | Server → Client | Server → Client |
| **Authentication** | Device ID | None | Token on connect |
| **Clients** | Physical buzzers | Public displays | Admin interfaces |
| **Connection limit** | Unlimited | Unlimited | Single |
| **Message types** | 2 client, 3 server | 14 events | 7 events |

---

## Testing the Protocols

### WebSocket

```bash
# Install websocat
cargo install websocat

# Test buzzer connection
( printf '{"type":"identification","id":"deadbeef0001"}\n'; \
  sleep 1; \
  printf '{"type":"buzz","id":"deadbeef0001"}\n'; \
  cat ) | websocat -t ws://localhost:8080/ws
```

### Server-Sent Events

```bash
# Public stream
curl -N http://localhost:8080/sse/public

# Admin stream
curl -N http://localhost:8080/sse/admin
```

### Browser (EventSource API)

```javascript
// Public stream
const source = new EventSource('http://localhost:8080/sse/public');
source.addEventListener('phase_changed', (e) => {
  console.log('Phase changed:', JSON.parse(e.data));
});

// Admin stream  
const admin = new EventSource('http://localhost:8080/sse/admin');
admin.addEventListener('handshake', (e) => {
  const token = JSON.parse(e.data).token;
  // Use token in X-Admin-Token header for REST calls
});
```
