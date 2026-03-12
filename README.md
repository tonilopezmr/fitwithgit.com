<div align="center">

# Fit with Git

**by [Toni Lopez](https://tonilopezmr.com)**

Track your fitness with a simple text file. See your progress as a GitHub-style contribution graph.

---

</div>

## What is Fit with Git?

Fit with Git turns a plain text file into a beautiful exercise activity graph. You log your workouts in a tiny file called `fit.log`, commit it to a Git repository, and Fit with Git renders your activity for the world to see.

No accounts. No apps. No syncing. Just a file and Git.

## How It Works

**1. You create a `fit.log` file in a Git repository**

```
S,260312,8500,10000
R,260312,32,5.1,6.3
G,260312,1
X,260312
```

Each line is one activity. One character for the type, a compact date, and the numbers that matter. That's it.

**2. You commit and push**

```bash
echo "S,260313,9200,10000" >> fit.log
git add fit.log && git commit -m "morning walk" && git push
```

Your fitness log lives in version control. Every commit is a record. Every diff tells a story.

**3. Fit with Git reads your repository and displays your activity**

Point Fit with Git to any public repository that contains a `fit.log`, and it renders a contribution-style heatmap of your exercise history.

## The `fit.log` Format

A compact, line-based format designed for minimal file size and clean Git diffs.

```
<activity>,<YYMMDD>,<fields...>
```

| Code | Activity | Example |
|------|----------|---------|
| `S` | Steps | `S,260312,8500,10000` |
| `R` | Run | `R,260312,32,5.1,6.3` |
| `W` | Swim | `W,260312,45,1500,30` |
| `B` | Bike | `B,260312,60,25.3,25.3` |
| `G` | Gym | `G,260312,1` |
| `X` | Stretch | `X,260312` |
| `K` | Ski | `K,260312,180,12` |
| `H` | Hike | `H,260312,95,8.4,650` |

Dates use `YYMMDD` format (e.g. `260312` = March 12, 2026). A full year of daily logging takes roughly **11 KB**.

See [FIT_LOG_SPEC.md](FIT_LOG_SPEC.md) for the complete format specification.

## Getting Started

**1.** Create a `fit.log` in any Git repository:

```bash
touch fit.log
```

**2.** Log your first activity:

```bash
echo "S,$(date +%y%m%d),7500,10000" >> fit.log
```

**3.** Commit and push:

```bash
git add fit.log && git commit -m "first log" && git push
```

**4.** Visit [fitwithgit.com](https://fitwithgit.com) and enter your repository handle to see your activity graph.

## Tech Stack

- **Rust** + **Axum** for the backend
- **Askama** for compile-time templates
- **htmx** for interactivity
- Zero JavaScript frameworks. Zero build steps.

## Development

```bash
cargo run    # Start the dev server at http://localhost:3000
```

---

<div align="center">

**Your fitness. Your data. Your repository.**

[fitwithgit.com](https://fitwithgit.com)

</div>
