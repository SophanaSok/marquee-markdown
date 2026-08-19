# Security policy

## Supported versions

This project is pre-1.0. Security fixes go into the latest release; there are
no maintained older lines.

## Reporting a vulnerability

Please report privately rather than in a public issue:

- Use GitHub's [private vulnerability
  reporting](https://github.com/SophanaSok/marquee-markdown/security/advisories/new),
  or
- Email **sokdevelopment@gmail.com**.

Please include what you did, what happened, and what you expected. A document
or URL that reproduces it is the most useful thing you can send.

You can expect an acknowledgement within a few days, and an assessment with a
plan and a timeline within two weeks.

## What this program does with input

Worth knowing when judging whether something is a vulnerability:

- **Markdown is rendered, never executed.** Raw HTML in a document is shown as
  literal text, not interpreted.
- **Nothing is opened without being asked.** Links are only followed when the
  reader steps to one and presses `enter`; opening hands off to the system
  handler.
- **Remote documents are fetched over HTTPS**, capped at 8 MiB, with a
  20-second timeout, and the body is treated as text throughout.
- **The renderer never emits an escape sequence it did not construct.** Text
  from a document cannot reach the terminal as a control sequence: control
  characters are stripped during fragmentation, and styling is applied
  structurally rather than by embedding escapes in text. A document that tries
  to drive the terminal through escape sequences is the main thing to look for
  here, and a way past that is worth reporting.
- **The library forbids unsafe code** (`#![forbid(unsafe_code)]`).
