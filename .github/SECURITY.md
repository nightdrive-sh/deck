# Security

Deck signs in to your Spotify account and keeps its sign-in files and tokens on your machine, listens for
commands on a local control socket (Linux), and starts Claude Code for the weekly Curated run, so a security problem in
it matters. Thank you for reporting one.

## Supported versions

| Version                                                   | Supported                                     |
| --------------------------------------------------------- | --------------------------------------------- |
| The latest release (the newest `vX.Y.Z` tag)              | Yes                                           |
| `main`                                                    | Yes                                           |
| Older releases                                            | No: update (below)                            |

Fixes are made in `main` and reach users in the next release, sooner if the problem is serious. To update: with
Nightdrive, its System update installs the new `deck` package; built from source, run `git pull` and
`cargo install --locked --path .` in the `deck` folder.

## Reporting a vulnerability

Please don't open a public issue or discussion for a vulnerability.

- **On GitHub:** this repository's **Security** tab › **Report a vulnerability**. Only the maintainer sees the report.
- **By email**, if you can't use GitHub: security@nightdrive.sh.

Tell what you found, how to reproduce it and what it lets someone do. The version (`deck --version`) and your
system (macOS or Linux) help.

## What happens next

- You get a reply within a week.
- Once the problem is confirmed, it is fixed and released, and you hear when.
- The fix is announced in the release notes, with your name if you'd like.

How soon a fix is released, counted from the confirmation:

| Severity                                                                                                  | Fix released                           |
| --------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| Critical: someone else runs code or reads your Spotify sign-in on your machine, with no step of yours     | Within 7 days, in a release of its own |
| High: the same, but it needs a step of yours, or it reaches only what Deck itself can                     | Within 30 days                         |
| Medium and low                                                                                            | In the next release                    |

A one-person project can miss these: if it does, you hear why and when instead.

## The EU's Cyber Resilience Act and NIS2

Deck is free, open-source software, made and given away without payment, sales, paid support or data
collection. As we understand them, the Cyber Resilience Act (EU 2024/2847) doesn't apply to free software that isn't
supplied in the course of a commercial activity, and NIS2 (EU 2022/2555) concerns organisations in its sectors, not
software like this. Deck therefore makes no CRA declaration of conformity. It still handles vulnerabilities as
above, which is what the Act asks of those it applies to. This is our reading, not legal advice.
