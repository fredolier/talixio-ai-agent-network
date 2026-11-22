# Contributing

Thank you for your interest. The project is young: feedback from real use matters as much as
code.

## Reporting a problem, proposing an idea

Open an issue on GitHub, with:

- the version (`talixio-agent --version`) and the system;
- what you did, what you expected, what happened;
- the master's journal around the problem, **without any token or key**.

A security vulnerability is not reported in a public issue: see [SECURITY.md](SECURITY.md).

## Proposing a change

1. Read [docs/DESIGN.md](docs/DESIGN.md) (architecture, security model, decisions, open
   questions). For a substantial change, open an issue first: a decision already made and
   justified there is not undone in passing.
2. Write the change with its tests.
3. Update the documentation **in the same change**: `docs/DESIGN.md` if a mechanism or a
   decision changes, `releases.toml` if behaviour changes for the user, `README.md` (and
   `README.fr.md`) if the way to use it changes.
4. Check:

   ```bash
   cargo test
   ```

   ```bash
   cargo clippy --all-targets
   ```

   Continuous integration replays everything on Linux, macOS and Windows, and builds the static
   binary. If you touch the interface (`web/`), rebuild it (`npm ci && npm run build` in
   `web/`): `web/dist` is part of the repository and must match the sources.

## Project rules

- **Security comes first.** Everything is denied by default; a child never has more permissions
  than its parent; every tool call is checked again. Any new surface (tool, link message, API
  route, deployment mode, command) goes through a security review, summarised in
  `docs/DESIGN.md`.
- **A single binary**, with no mandatory dependency on the system.
- **Say what has not been tried.** A feature written and tested with fake programs is not
  "tried": the documentation makes the difference.
- **Language.** The repository is in English: documentation, new comments and new messages.
  Much of the existing code still carries French comments and messages; translate what you
  touch. The web interface is written in English with a French translation
  (`web/src/locales/fr/`): a new text needs both, and `npm run build` checks it.
- **Never a secret** in the repository, in an example configuration or in a journal.

## License of contributions

The project is licensed under [AGPL-3.0-only](LICENSE). By proposing a change, you agree that it
is published under this license, and you state that you have the right to propose it.
