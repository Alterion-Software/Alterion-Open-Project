# Contributing to Alterion Open Project

Thanks for your interest. Read this in full before opening anything.

---

## Before you start

Open an issue first. Describe the problem or the change you want to make and
wait for a maintainer to respond before writing any code. Merge requests opened
without a linked issue will be closed without review.

---

## What we want

- Bug fixes with the root cause identified, not the symptom patched
- Scheduling correctness: a plan where the dates come out wrong, with the plan
  attached, is one of the most useful things you can report
- Missing test coverage for behaviour that is already documented
- Platform fixes. Only Linux is tested regularly; Windows and macOS need
  attention and are the most valuable place to help
- Anything in **Not implemented** in the README

## What we do not want

**Do not open a merge request for:**

- Reformatting, renaming or "tidying" that does not change behaviour
- A new dependency, without discussing it in the issue first
- Reimplementing the scheduler. If it is wrong, show the plan that proves it
- Code copied from a GPL or LGPL project. This is Apache-2.0 and stays that way

---

## How the code is arranged

`aop-core` is the plan and the maths, with no UI at all, and is where nearly
every test lives. `aop-app` is the shell around it. A change to how scheduling
behaves belongs in `aop-core` with a test; a change to how it looks belongs in
`aop-app`.

Keeping the engine free of UI is what lets it be tested properly. Please do not
reach into Dioxus from `aop-core`.

## Rules

1. **A behavioural change needs a test.** In `aop-core` this is not negotiable.
2. **Say why, not what.** A comment repeating the code is noise; a comment
   explaining why an offset is what it is, or why an obvious approach was
   rejected, is what makes this maintainable.
3. **No warnings.** `cargo clippy --all-targets` must be clean.
4. **No panics on file input.** Files arrive from elsewhere. Degrade to a
   missing value; never unwrap something a file supplied.

## Tests

```
cargo test
cargo clippy --all-targets
cargo run -p aop-app        # and actually look at it
```

Scheduling tests are written as small plans with the expected dates spelled out,
so they read as statements about the calendar rather than as fixtures.

---

## Licence

By contributing you agree that your contribution is licensed under Apache-2.0,
and that it is your own work.
