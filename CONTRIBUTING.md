# Contributing

Thank you for helping. Two rules matter more than any other.

## 1. No proprietary material, ever

Never commit, attach or paste:

- game files of any kind: fastfiles, zones, rawfiles, `.gscc`/`.gscbin`, sound banks, images;
- game scripts, original or decompiled, even a few lines of them;
- executable dumps, memory dumps or anything extracted from them;
- Activision documentation, mod-tools sources or headers;
- name databases built from game data (`.wni`, hash CSVs). Tests read these from local paths
  given in environment variables, never from the repository.

Bug reports and tests use reduced fixtures you wrote yourself. Hash constants and algorithm
descriptions are facts and are fine. If you are unsure whether something is acceptable, ask first.

## 2. Developer Certificate of Origin

Every commit must be signed off, certifying the [DCO 1.1](https://developercertificate.org/):
that you wrote the change or have the right to submit it under this project's licence.

```sh
git commit -s
```

This adds `Signed-off-by: Your Name <you@example.com>` to the message. Commits without it are
not merged.

## Licence

Contributions are licensed GPL-3.0-or-later, like the rest of the project.

## Checks

Before you open a pull request:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

A new hash family needs a stable id, its games, its canonicalisation and width, its source, and a
known-vector unit test.

## Proposing a rule

Open an issue with the **Rule proposal** template before writing code. It asks for:

- the code and name you suggest (a category letter plus digits, a kebab-case name);
- what it reports and why that is a bug, with a short example you wrote yourself;
- the games it applies to, and the data it needs (a profile feature, a builtin or field catalogue);
- the false-positive risk: where shipped scripts or common mod code would trip it, and how you
  measured it (counts on your local corpus are welcome; the corpus itself is not);
- the fix, if any, and whether it is safe (it never changes behaviour) or unsafe.

Once accepted, a rule lands with its page in `docs/rules/<CODE>.md` (what it does, why, an
example, the fix, how to suppress it), snapshot tests on hand-written fixtures and an entry in
`docs/rules/README.md`. New rules start in **preview** (`--preview`) and stay there for at least
one minor release; promotion to stable is a changelog entry. A missed diagnostic costs less than a
false positive: when the data says `unknown`, the rule stays silent.

## Data

Dialect profiles, builtin and field catalogues live in the separate gruff-data repository (CC0).
Corrections to the data go there, with their source and method; see its CONTRIBUTING.md.

## Reporting a takedown request

See the legal notice, `docs/legal.md` ("Takedown requests"), also in the documentation book.
