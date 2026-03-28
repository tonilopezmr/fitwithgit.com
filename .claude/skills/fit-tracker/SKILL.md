---
name: fit-tracker
description: Track fitness data using the fit MCP server. Use when the user wants to log workouts, check fitness stats, sync from Garmin/Whoop, or query their exercise history. Triggers on "log my workout", "sync garmin", "sync whoop", "fitness summary", "add exercise", "how many runs", "my steps".
metadata:
  author: tonilopezmr
  version: "1.0.0"
---

# Fit Tracker

Interact with the user's fitness data through the `fit` MCP server tools.

## Available MCP Tools

All tools are prefixed with `mcp__fit__`:

| Tool | Purpose |
|------|---------|
| `mcp__fit__read_fit_log` | Read and filter fit.log entries |
| `mcp__fit__add_entry` | Add a manual entry to fit.log |
| `mcp__fit__garmin_sync` | Sync data from Garmin Connect |
| `mcp__fit__whoop_sync` | Sync data from Whoop |
| `mcp__fit__get_summary` | Get fitness statistics |

## fit.log Format

Entries use the format `<code>,<YYMMDD>,<fields>`:

| Code | Activity | Fields | Example |
|------|----------|--------|---------|
| `S` | Steps | total_steps, goal | `S,260312,8500,10000` |
| `R` | Run | duration_min, distance_km, pace_min_per_km | `R,260312,32,5.1,6.3` |
| `W` | Swim | duration_min, distance_m, laps | `W,260312,45,1500,30` |
| `B` | Bike | duration_min, distance_km, avg_speed_kmh | `B,260312,60,25.3,25.3` |
| `G` | Gym | sessions | `G,260312,1` |
| `X` | Stretch | _(none)_ | `X,260312` |
| `K` | Ski | duration_min, runs | `K,260312,180,12` |
| `H` | Hike | duration_min, distance_km, elevation_m | `H,260312,95,8.4,650` |
| `Z` | Sleep | duration_min, score | `Z,260312,462,85` |
| `V` | Recovery | recovery_pct, hrv_ms, rhr_bpm | `V,260312,78,65,52` |

Date format is `YYMMDD` (e.g. `260320` = 2026-03-20).

## How to Use

### Logging a workout
When the user says they did an activity, convert it to the correct format and use `mcp__fit__add_entry`:
- "I ran 5k in 30 minutes" -> calculate pace (6.0 min/km), format today's date as YYMMDD, call `add_entry` with `R,260320,30,5.0,6.0`
- "I went to the gym" -> `G,260320,1`
- "I stretched today" -> `X,260320`

### Querying data
- Use `mcp__fit__read_fit_log` with filters to answer questions about specific activities or date ranges
- Use `mcp__fit__get_summary` for aggregate statistics like totals and averages
- Dates in tool params use `YYYY-MM-DD` format (e.g. `2026-03-20`), NOT the YYMMDD format used in entries

### Syncing
- `mcp__fit__garmin_sync` pulls steps + activities from Garmin Connect (needs GARMIN_USERNAME/GARMIN_PASSWORD env vars)
- `mcp__fit__whoop_sync` pulls workouts, sleep, and recovery from Whoop (needs WHOOP_CLIENT_ID/WHOOP_CLIENT_SECRET env vars)
- Both default to syncing from the latest logged day to today, so same-day records can refresh
- Use `dry_run: true` to preview what would be synced

### Important rules
- Always validate the date is correct before adding an entry
- Use today's date when the user says "today" — convert to YYMMDD format
- Pace is calculated as `duration_min / distance_km`
- A day can have multiple records (steps + gym + stretch is normal)
- When the user asks about fitness in natural language, translate to the right tool call — don't ask them to format the entry themselves
