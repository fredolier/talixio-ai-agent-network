# Security

## Reporting a vulnerability

**Do not write it in a public issue.** Use GitHub's private reporting: the repository's
"Security" tab, then "Report a vulnerability"
(<https://github.com/fredolier/talixio-ai-agent-network/security/advisories/new>).

Say what you observed, on which version (`talixio-agent --version`), and how to reproduce it.
You will get an answer, then a fix or an explanation; your name will appear in the release notes
if you wish.

## Supported versions

Only the latest published version receives fixes. The project is young and moves fast: update
before reporting.

## What counts as a vulnerability

The project promises the following, and anything that contradicts it is of interest:

- an agent reading, writing or running something outside its permissions;
- a child obtaining more permissions than its parent;
- a model key or the API token leaving the master (journal, interface, child);
- the master installing or modifying its own commands, roles or configuration;
- a pack or a pack repository accepted without the announced verification;
- the API or the interface usable without the token.

## Known limits

They are written down, and so are not discoveries: see [docs/DESIGN.md](docs/DESIGN.md),
"Known limitations". The main one: between agents of the same account, isolation is enforced by
the program and not yet by the operating system. A command you allow runs with all the rights of
the account; only allow commands written as tightly as possible, and run the master under a
dedicated account (`sudo talixio-agent service install`) or in a container.

The same document summarises each security review: findings, severity, treatment.
