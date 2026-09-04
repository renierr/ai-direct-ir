# __APPNAME__ Verification

## Baseline

```bash
host-rs build
host-rs check
__RUN_COMMAND__
host-rs dist
```

## Automated Checks

- [ ] Replace with commands and expected output/exit status.

## Manual Checks

- [ ] Replace with visible behavior to exercise for this target.

## Failure And Safety Checks

- [ ] Invalid input and provider errors are visible and do not corrupt state.
- [ ] No secrets, private data, generated output, or local vendor cache appear
      in `git status` before commit.
