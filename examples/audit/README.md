# Example: a group of agents that audits repositories

You give the master a list of repositories. It creates one lead per repository; each lead fetches
its repository, opens a report, and hands each step to a specialised agent. The result is one
`report.md` per repository, in the work directory. Nothing is changed in the audited repositories,
and nothing from them is ever run.

| Agent | What it does | What it may do |
|---|---|---|
| `audit/lead` | runs one audit, closes the report | fetch the repository, map it, ask git questions, create the five agents below |
| `audit/goal` | finds what the repository is for, sketches the simplest design that meets it | read the clone, write the report |
| `audit/code-reviewer` | judges the code against the goal | read the clone, write the report |
| `audit/setup-reviewer` | judges the AI-assistant setup, the CI and the contribution rules | read the clone, write the report |
| `audit/planner` | proposes how to simplify without losing what works | read the clone, write the report |
| `audit/verifier` | tries to refuse the report: every finding must cite a file and line or a commit | read only |

The commands and the roles form the "audit" pack (`packs/audit` in the repository, shipped in the
executable). You install it; the master can neither install nor change it, and checks the
fingerprint of every file when it starts, then of each script before running it.

```bash
talixio-agent pack show audit
```

- `fetch-repo <directory> <https address> [branch or commit]`: the only access to the network.
  Reading only: no hook, no submodule, no push address. The git data is kept outside the agents'
  directories (under the home directory of the account), so that no agent can alter it.
- `repo-map <clone> <file.md>`: reproducible numbers (sizes, assistant material, context
  estimate, call graph, duplication, growth). Heuristics: leads to verify, not findings.
- `repo-git <clone> <question>`: nine read-only questions (files, search, history, recent,
  commit, show, diff, blame, authors). Answers are cut at 60,000 bytes, and say so.
- `new-report <file.md>`: the empty report, nine sections.
- `check-report <clone> <report.md>`: what the report cites, checked mechanically: each file and
  line, each commit, with the cited line printed beside it; sections left empty. It does not judge
  whether the line says what the report claims: the verifier does.
- `brief.md`: the message to send to the master
  (`talixio-agent pack show audit --file brief.md`).

## Run it in Docker

The image "audit" is the master plus what the commands need (git, bash, python3). The master and
its agents share the container: one container per audit campaign is the isolation boundary.

```bash
./tools.sh build audit
```

```bash
./tools.sh pack install audit
```

```bash
TALIXIO_CONFIG=examples/audit/talixio.toml ./tools.sh run audit
```

Open the interface (`./tools.sh token` prints its token), connect a provider and activate a model,
then send the brief with your list in place of the example:

```bash
docker exec talixio /talixio-agent pack show audit --file brief.md
```

The reports are in the volume, under `/data/work/audits/<name>/report.md`:

```bash
docker exec talixio cat /data/work/audits/<name>/report.md
```

## What to know before a first run

- **The goal is confirmed by you.** Each lead asks the master, which asks you, whether the goal
  it found is the right one. Without an answer the report says "not confirmed".
- **Private repositories.** Add a read-only token on the "Secrets" page of the master: command
  `audit/fetch-repo`, variable `AUDIT_READ_TOKEN`, role `audit/lead`, and in `AUDIT_READ_URL`
  the address prefix the token is valid for, ending with `/` (`https://gitlab.com/your-group/`).
  A lead that receives the token fetches only under that prefix: tie it to the leads of your
  private repositories, or audit public ones before adding it. Only the
  `fetch-repo` command of a lead receives it, for the time of one run; no model ever reads it,
  and the command offers it to no other address. `AUDIT_READ_USER` is only needed for a deploy
  token (its user name).
- **Behind a proxy**, `fetch-repo` cannot reach the network: commands run with an emptied
  environment, proxy variables included.
- **Cost.** An audit is several agents reading a whole repository: pick a model with a large
  context for the lead and the reviewers, and set `max_tokens` in `talixio.toml` to what you
  accept to spend. There is no limit yet on the size of a clone.
- **Without Docker**: install the pack under another account than the master's
  (`talixio-agent pack install audit --dir /opt/talixio/packs`), set `packs_dir`, and make sure
  the home directory of the master's account is not inside a directory granted to agents.
